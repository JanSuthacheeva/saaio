use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, Padding, Paragraph};

use crate::proto::PaneStatus;
use crate::ui::theme::Theme;

pub fn draw_dialog(
    frame: &mut Frame,
    pane: Rect,
    theme: &Theme,
    title: &str,
    border: Style,
    text: Vec<Line>,
) {
    let fit = |size: usize, edge: u16, room: u16| {
        u16::try_from(size)
            .unwrap_or(u16::MAX)
            .saturating_add(edge)
            .min(room)
    };
    let inner = text.iter().map(Line::width).max().unwrap_or(0);
    let width = fit(inner, 4, pane.width);
    let height = fit(text.len(), 2, pane.height);
    let area = Rect::new(
        pane.x + (pane.width - width) / 2,
        pane.y + (pane.height - height) / 2,
        width,
        height,
    );
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(text).block(
            Block::bordered()
                .border_type(BorderType::Rounded)
                .border_style(border)
                .style(theme.ui.background)
                .padding(Padding::horizontal(1))
                .title(title),
        ),
        area,
    );
}

/// Every key behind C-space, over the pane.
pub fn draw_help(frame: &mut Frame, pane: Rect, theme: &Theme) {
    let keys = [
        ("0-9", "jump to a session (0 is the master)"),
        ("j k", "move the peek over the workers"),
        ("enter", "jump into the peeked worker"),
        ("n", "start a new flow"),
        ("o", "the board of every session"),
        ("b", "fold the sidebar to a rail and back"),
        ("r", "resume a session that ended"),
        ("[", "scroll back through the history"),
        ("d", "detach; every session keeps running"),
        ("Q", "stop flow"),
        ("C-space", "send C-space itself"),
        ("?", "these keys"),
    ];
    let mut text = vec![Line::from(vec![
        Span::styled("C-space", theme.ui.key),
        Span::styled(" then", theme.ui.muted),
    ])];
    for (key, what) in keys {
        text.push(Line::from(vec![
            Span::styled(format!("  {key:<9}"), theme.ui.key),
            Span::styled(what, theme.ui.text),
        ]));
    }
    text.push(Line::raw(""));
    text.push(Line::styled("any key closes this", theme.ui.faint));
    draw_dialog(frame, pane, theme, " keys ", theme.ui.rule, text);
}

/// A notice over a pane whose process is gone: for a worker (`worker` is its name) what to do with
/// its flow, for the master how to bring it back.
pub fn draw_not_running(
    frame: &mut Frame,
    pane: Rect,
    status: &PaneStatus,
    worker: Option<&str>,
    theme: &Theme,
) {
    let (title, first, style) = match status {
        PaneStatus::Exited { code } => (
            " exited ",
            match code {
                Some(0) | None => "The session ended.".to_string(),
                Some(code) => format!("The session ended with code {code}."),
            },
            theme.state.waiting,
        ),
        PaneStatus::Failed { reason } => (" could not start ", reason.clone(), theme.state.blocked),
        PaneStatus::Running => return,
    };
    let keys = match worker {
        Some(_) => vec![
            Span::styled("x", theme.ui.key),
            Span::styled(" clean up: worktree, branch, flow   ", theme.ui.muted),
            Span::styled("r", theme.ui.key),
            Span::styled(" resume   ", theme.ui.muted),
            Span::styled("esc", theme.ui.key),
            Span::styled(" back to master", theme.ui.muted),
        ],
        None => vec![
            Span::styled("C-space r", theme.ui.key),
            Span::styled(" resumes it   ", theme.ui.muted),
            Span::styled("C-space Q", theme.ui.key),
            Span::styled(" stops flow", theme.ui.muted),
        ],
    };
    let text = vec![Line::styled(first, theme.ui.text), Line::from(keys)];
    draw_dialog(frame, pane, theme, title, style, text);
}

/// A notice over a worker pane flow ui has no session for, such as a flow that ended before it opened.
pub fn draw_no_session(frame: &mut Frame, pane: Rect, theme: &Theme) {
    let text = vec![
        Line::styled("flow ui has no session for this flow.", theme.ui.text),
        Line::from(vec![
            Span::styled("C-space r", theme.ui.key),
            Span::styled(" resumes it   ", theme.ui.muted),
            Span::styled("C-space 0", theme.ui.key),
            Span::styled(" back to the master", theme.ui.muted),
        ]),
    ];
    draw_dialog(frame, pane, theme, " not open ", theme.ui.rule, text);
}
