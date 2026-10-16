use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, Widget};

use crate::proto::PaneStatus;
use crate::ui::fit::columns;
use crate::ui::hints::{self, Hint};
use crate::ui::theme::Theme;

/// Rows between the border and the body, above and below it.
const PAD_ROWS: u16 = 1;
/// Columns between the border and the body, left and right of it.
const PAD_COLS: u16 = 2;

/// A box over the session: the screen behind it dimmed, the title in its border, one row and two
/// columns of padding, and its keys under a rule that joins the border.
#[derive(Debug)]
pub struct Dialog<'a> {
    pub title: &'a str,
    /// Right of the title in the border, faint: where a new flow starts.
    pub aside: Option<String>,
    /// The border's color: rule for a notice, waiting for a question, blocked for a failure.
    pub border: Style,
    pub body: Vec<Line<'a>>,
    /// Rows the body takes beyond its lines, for a widget that draws into it (new flow).
    pub body_rows: u16,
    pub body_width: u16,
    pub keys: Vec<Hint>,
    pub theme: &'a Theme,
}

impl<'a> Dialog<'a> {
    /// A dialog with `body` and no keys, its border in the rule color.
    pub fn new(title: &'a str, body: Vec<Line<'a>>, theme: &'a Theme) -> Dialog<'a> {
        Dialog {
            title,
            aside: None,
            border: theme.ui.rule,
            body,
            body_rows: 0,
            body_width: 0,
            keys: Vec::new(),
            theme,
        }
    }

    /// Rows the rule and the keys take under the body.
    fn key_rows(&self) -> u16 {
        if self.keys.is_empty() { 0 } else { 2 }
    }

    fn body_height(&self) -> u16 {
        u16::try_from(self.body.len())
            .unwrap_or(u16::MAX)
            .saturating_add(self.body_rows)
    }

    /// The padding above and below the body: none when `area` is too short for it.
    fn pad_rows(&self, area: Rect) -> u16 {
        let padded = self
            .body_height()
            .saturating_add(2 + 2 * PAD_ROWS + self.key_rows());
        if padded <= area.height { PAD_ROWS } else { 0 }
    }

    /// The box centered in `area`, a little above the middle, clamped to it.
    pub fn area(&self, area: Rect) -> Rect {
        let to_u16 = |columns: usize| u16::try_from(columns).unwrap_or(u16::MAX);
        let lines = self.body.iter().map(Line::width).max().unwrap_or(0);
        let keys = hints::line(&self.keys, usize::MAX, self.theme).width();
        let title = columns(self.title) + self.aside.as_deref().map_or(0, columns) + 6;
        let inner = to_u16(lines.max(keys))
            .max(self.body_width)
            .max(to_u16(title).saturating_sub(2 * PAD_COLS));
        let width = inner.saturating_add(2 + 2 * PAD_COLS).min(area.width);
        let height = self
            .body_height()
            .saturating_add(2 + 2 * self.pad_rows(area) + self.key_rows())
            .min(area.height);
        Rect::new(
            area.x + (area.width - width) / 2,
            area.y + (area.height - height) * 2 / 5,
            width,
            height,
        )
    }

    /// Where the body goes inside `area(area)`: inside the padding, above the key rule.
    pub fn body_area(&self, area: Rect) -> Rect {
        let dialog = self.area(area);
        let pad = self.pad_rows(area);
        Rect::new(
            dialog.x + 1 + PAD_COLS,
            dialog.y + 1 + pad,
            dialog.width.saturating_sub(2 + 2 * PAD_COLS),
            dialog.height.saturating_sub(2 + 2 * pad + self.key_rows()),
        )
        .intersection(dialog)
    }
}

/// Dims all of buf.area, clears the box, draws frame, title, aside, body, the ├─┤ rule and
/// hints::line(keys).
impl Widget for Dialog<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let theme = self.theme;
        buf.set_style(buf.area, Style::default().add_modifier(Modifier::DIM));
        let dialog = self.area(area);
        let body = self.body_area(area);
        Clear.render(dialog, buf);
        let mut block = Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(self.border)
            .style(theme.ui.background)
            .title_top(Line::from(vec![
                Span::styled("─ ", self.border),
                Span::styled(self.title, theme.ui.strong),
                Span::raw(" "),
            ]));
        if let Some(aside) = &self.aside {
            block = block.title_top(
                Line::from(vec![
                    Span::styled(format!(" {aside} "), theme.ui.faint),
                    Span::styled("─", self.border),
                ])
                .right_aligned(),
            );
        }
        block.render(dialog, buf);
        for (line, row) in self.body.iter().zip(body.rows()) {
            buf.set_line(row.x, row.y, line, row.width);
        }
        if self.keys.is_empty() || dialog.height < 4 {
            return;
        }
        let keys_y = dialog.bottom() - 2;
        let rule_y = keys_y - 1;
        if rule_y > dialog.y {
            let inner = usize::from(dialog.width.saturating_sub(2));
            let rule = Line::styled(format!("├{}┤", "─".repeat(inner)), self.border);
            buf.set_line(dialog.x, rule_y, &rule, dialog.width);
        }
        let room = dialog.width.saturating_sub(2 + 2 * PAD_COLS);
        let keys = hints::line(&self.keys, usize::from(room), theme);
        buf.set_line(dialog.x + 1 + PAD_COLS, keys_y, &keys, room);
    }
}

pub fn draw_dialog(
    frame: &mut Frame,
    pane: Rect,
    theme: &Theme,
    title: &str,
    border: Style,
    text: Vec<Line>,
) {
    let title = title.trim();
    frame.render_widget(
        Dialog {
            border,
            ..Dialog::new(title, text, theme)
        },
        pane,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn dialog(theme: &Theme) -> Dialog<'_> {
        Dialog {
            keys: vec![Hint::new("y", "stops it"), Hint::new("esc", "keeps it")],
            ..Dialog::new(
                "stop",
                vec![Line::raw("Stop flow? Every session in it ends.")],
                theme,
            )
        }
    }

    fn rows(buf: &Buffer) -> Vec<String> {
        (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect()
    }

    #[test]
    fn pads_the_body_and_centers_the_box_a_little_above_the_middle() {
        let theme = Theme::default();
        let area = Rect::new(1, 1, 100, 40);

        let dialog = dialog(&theme);

        let outer = dialog.area(area);
        assert_eq!(outer.width, 36 + 2 + 4);
        assert_eq!(
            outer.height,
            1 + 1 + 1 + 1 + 1 + 1 + 1,
            "border, pad, body, pad, rule, keys, border"
        );
        assert_eq!(outer.x, 1 + (100 - 42) / 2);
        assert_eq!(outer.y, 1 + (40 - 7) * 2 / 5);
        assert_eq!(
            dialog.body_area(area),
            Rect::new(outer.x + 3, outer.y + 2, 36, 1)
        );
    }

    #[test]
    fn clamps_to_a_small_area_and_gives_up_the_padding_and_the_body_before_the_keys() {
        let theme = Theme::default();
        let mut dialog = dialog(&theme);
        dialog.body_rows = 30;
        let area = Rect::new(0, 0, 20, 8);

        let outer = dialog.area(area);
        assert_eq!(outer, area);
        let body = dialog.body_area(area);
        assert_eq!(body.height, 8 - 4, "the padding goes first");
        assert!(body.width <= 14);
    }

    #[test]
    fn draws_the_title_in_the_border_and_the_keys_under_a_joined_rule() {
        let theme = Theme::default();
        let area = Rect::new(0, 0, 50, 9);
        let mut buf = Buffer::empty(area);

        dialog(&theme).render(area, &mut buf);

        let rows = rows(&buf);
        let box_rows: Vec<&str> = rows
            .iter()
            .map(|row| row.trim())
            .filter(|row| !row.is_empty())
            .collect();
        assert_eq!(
            box_rows,
            [
                "╭─ stop ─────────────────────────────────╮",
                "│                                        │",
                "│  Stop flow? Every session in it ends.  │",
                "│                                        │",
                "├────────────────────────────────────────┤",
                "│  y stops it   esc keeps it             │",
                "╰────────────────────────────────────────╯",
            ]
        );
        assert!(
            buf[(0, 0)].modifier.contains(Modifier::DIM),
            "what is behind the box is dimmed"
        );
        let inside = dialog(&theme).body_area(area);
        assert!(!buf[(inside.x, inside.y)].modifier.contains(Modifier::DIM));
    }

    #[test]
    fn shows_the_aside_at_the_right_of_the_border() {
        let theme = Theme::default();
        let area = Rect::new(0, 0, 50, 9);
        let mut buf = Buffer::empty(area);
        let mut dialog = dialog(&theme);
        dialog.aside = Some("in ~/shop".into());

        dialog.render(area, &mut buf);

        let top = rows(&buf)
            .into_iter()
            .find(|row| row.contains("stop"))
            .unwrap();
        assert!(top.trim_end().ends_with(" in ~/shop ─╮"), "{top}");
    }

    #[test]
    fn never_draws_outside_a_tiny_area() {
        let theme = Theme::default();
        for (width, height) in [(0, 0), (3, 2), (10, 3), (6, 5), (80, 4)] {
            let area = Rect::new(0, 0, width, height);
            let mut buf = Buffer::empty(area);
            dialog(&theme).render(area, &mut buf);
        }
    }
}
