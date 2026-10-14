use ratatui::Frame as TuiFrame;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, Padding, Paragraph};

use crate::proto::{ClientMsg, Modes, PaneInfo, PaneKey, PaneStatus, ServerMsg, Size};
use crate::ui::input::{Command, Key, Routed};
use crate::ui::layout::{self, Areas, Chrome};
use crate::ui::screen::Screen;
use crate::ui::theme::Theme;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum View {
    Panes,
    ConfirmStop,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    Send(ClientMsg),
    Mirror(Modes),
    Quit(String),
}

/// What flow ui shows and what it does with input; no I/O of its own.
pub struct App {
    size: Size,
    screen: Screen,
    panes: Vec<PaneInfo>,
    focus: PaneKey,
    view: View,
    mirrored: Modes,
}

impl App {
    pub fn new(size: Size) -> App {
        App {
            size,
            screen: Screen::default(),
            panes: Vec::new(),
            focus: PaneKey::Master,
            view: View::Panes,
            mirrored: Modes::default(),
        }
    }

    pub fn areas(&self) -> Areas {
        layout::areas(self.size, Chrome::Solo)
    }

    /// True while a dialog takes the keys instead of the pane.
    pub fn is_modal(&self) -> bool {
        self.view != View::Panes
    }

    pub fn on_server(&mut self, message: ServerMsg) -> Vec<Effect> {
        match message {
            ServerMsg::Welcome { .. } | ServerMsg::Board(_) | ServerMsg::Closed(_) => vec![],
            ServerMsg::Refused { reason } => vec![Effect::Quit(reason)],
            ServerMsg::Panes(panes) => {
                self.panes = panes;
                vec![]
            }
            ServerMsg::Frame(frame) => {
                if frame.key != self.focus {
                    return vec![];
                }
                self.screen.apply(frame);
                if self.screen.modes == self.mirrored {
                    return vec![];
                }
                self.mirrored = self.screen.modes;
                vec![Effect::Mirror(self.mirrored)]
            }
            ServerMsg::Detached => vec![Effect::Quit(
                "another flow ui attached, so this one let go".to_string(),
            )],
        }
    }

    pub fn on_input(&mut self, routed: Routed) -> Vec<Effect> {
        match routed {
            Routed::Pane(bytes) if self.view == View::Panes => {
                vec![Effect::Send(ClientMsg::Input(bytes))]
            }
            Routed::Pane(_) | Routed::Click { .. } => vec![],
            Routed::View(keys) => self.on_keys(keys),
            Routed::Command(command) => self.on_command(command),
        }
    }

    pub fn on_resize(&mut self, size: Size) -> Vec<Effect> {
        self.size = size;
        vec![Effect::Send(ClientMsg::Resize(self.areas().pane_size()))]
    }

    pub fn draw(&self, frame: &mut TuiFrame, theme: &Theme) {
        let areas = self.areas();
        frame.render_widget(&self.screen, areas.pane);
        if let Some(bar) = areas.bar {
            self.draw_bar(frame, bar, theme);
        }
        if let Some(status) = self.status(&self.focus)
            && *status != PaneStatus::Running
        {
            draw_not_running(frame, areas.pane, status, theme);
        }
        match self.view {
            View::ConfirmStop => draw_confirm_stop(frame, areas.pane, theme),
            View::Panes => {
                if let Some((col, row)) = self.screen.cursor
                    && self
                        .status(&self.focus)
                        .is_none_or(|status| *status == PaneStatus::Running)
                    && self.screen.history.is_none()
                    && col < areas.pane.width
                    && row < areas.pane.height
                {
                    frame.set_cursor_position((areas.pane.x + col, areas.pane.y + row));
                }
            }
        }
    }

    fn on_command(&mut self, command: Command) -> Vec<Effect> {
        match command {
            Command::Detach => vec![Effect::Quit(
                "detached - the agents keep running; run `flow ui` to attach again".to_string(),
            )],
            Command::StopServer => {
                self.view = View::ConfirmStop;
                vec![]
            }
            Command::Reopen => vec![Effect::Send(ClientMsg::Reopen(self.focus.clone()))],
            Command::Focus(0) => self.focus(PaneKey::Master),
            _ => vec![],
        }
    }

    fn on_keys(&mut self, keys: Vec<Key>) -> Vec<Effect> {
        let Some(key) = keys.into_iter().next() else {
            return vec![];
        };
        match self.view {
            View::ConfirmStop if key == Key::Char('y') => vec![
                Effect::Send(ClientMsg::StopServer),
                Effect::Quit("stopped the flow server and every session in it".to_string()),
            ],
            View::ConfirmStop | View::Panes => {
                self.view = View::Panes;
                vec![]
            }
        }
    }

    fn focus(&mut self, key: PaneKey) -> Vec<Effect> {
        if key == self.focus {
            return vec![];
        }
        self.focus = key.clone();
        vec![Effect::Send(ClientMsg::Focus(key))]
    }

    fn status(&self, key: &PaneKey) -> Option<&PaneStatus> {
        self.panes
            .iter()
            .find(|pane| &pane.key == key)
            .map(|pane| &pane.status)
    }

    fn draw_bar(&self, frame: &mut TuiFrame, area: Rect, theme: &Theme) {
        let glyphs = &theme.glyphs;
        let mut left = vec![
            Span::raw(" "),
            Span::styled(glyphs.mark, theme.ui.accent),
            Span::styled(" flow ", theme.ui.strong),
            Span::styled(format!(" {} ", glyphs.separator), theme.ui.rule),
            Span::styled(glyphs.master, theme.ui.accent),
            Span::styled(" master", theme.ui.text),
        ];
        match self.status(&PaneKey::Master) {
            Some(PaneStatus::Exited { .. }) => left.push(Span::styled(
                "  exited - C-space r resumes it",
                theme.state.waiting,
            )),
            Some(PaneStatus::Failed { reason }) => {
                left.push(Span::styled(format!("  {reason}"), theme.state.blocked));
            }
            _ => {}
        }
        let right = Line::from(vec![
            Span::styled("C-space", theme.ui.key),
            Span::styled(" then ", theme.ui.faint),
            Span::styled("d", theme.ui.key),
            Span::styled(" detach  ", theme.ui.muted),
            Span::styled("r", theme.ui.key),
            Span::styled(" resume  ", theme.ui.muted),
            Span::styled("Q", theme.ui.key),
            Span::styled(" stop ", theme.ui.muted),
        ]);
        let left = Line::from(left);
        let fits = left.width() + right.width() < area.width as usize;
        frame.render_widget(Paragraph::new(left), area);
        if fits {
            frame.render_widget(Paragraph::new(right).alignment(Alignment::Right), area);
        }
    }
}

/// A notice over a pane whose process is gone, saying how to bring it back.
fn draw_not_running(frame: &mut TuiFrame, pane: Rect, status: &PaneStatus, theme: &Theme) {
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
    let text = vec![
        Line::styled(first, theme.ui.text),
        Line::from(vec![
            Span::styled("C-space r", theme.ui.key),
            Span::styled(" resumes it   ", theme.ui.muted),
            Span::styled("C-space Q", theme.ui.key),
            Span::styled(" stops flow", theme.ui.muted),
        ]),
    ];
    draw_dialog(frame, pane, title, style, text);
}

fn draw_dialog(frame: &mut TuiFrame, pane: Rect, title: &str, border: Style, text: Vec<Line>) {
    let inner = text.iter().map(Line::width).max().unwrap_or(0) as u16;
    let width = (inner + 4).min(pane.width);
    let height = (text.len() as u16 + 2).min(pane.height);
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
                .padding(Padding::horizontal(1))
                .title(title),
        ),
        area,
    );
}

fn draw_confirm_stop(frame: &mut TuiFrame, pane: Rect, theme: &Theme) {
    let text = vec![
        Line::styled(
            "Stop the flow server? Every session in it ends.",
            theme.ui.text,
        ),
        Line::from(vec![
            Span::styled("y", theme.ui.key),
            Span::styled(" stops it, any other key keeps it", theme.ui.muted),
        ]),
    ];
    draw_dialog(frame, pane, " stop ", theme.ui.rule, text);
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;
    use crate::proto::{Cell, Color, Flags, Frame, Mouse};

    const SIZE: Size = Size { cols: 80, rows: 10 };

    fn frame_with(text: &str, modes: Modes) -> Frame {
        Frame {
            key: PaneKey::Master,
            full: true,
            rows: vec![(
                0,
                text.chars()
                    .map(|c| Cell {
                        text: c.to_string(),
                        fg: Color::Default,
                        bg: Color::Default,
                        flags: Flags::default(),
                    })
                    .collect(),
            )],
            cursor: Some((text.len() as u16, 0)),
            modes,
            history: None,
        }
    }

    fn render(app: &App) -> Terminal<TestBackend> {
        let mut terminal = Terminal::new(TestBackend::new(SIZE.cols, SIZE.rows)).unwrap();
        terminal
            .draw(|frame| app.draw(frame, &Theme::builtin()))
            .unwrap();
        terminal
    }

    fn row(terminal: &Terminal<TestBackend>, y: u16) -> String {
        let buffer = terminal.backend().buffer();
        (0..buffer.area.width)
            .map(|x| buffer[(x, y)].symbol().to_string())
            .collect()
    }

    #[test]
    fn sends_keys_to_the_pane_and_mirrors_the_modes_it_asks_for() {
        let mut app = App::new(SIZE);

        assert_eq!(
            app.on_input(Routed::Pane(b"hi".to_vec())),
            vec![Effect::Send(ClientMsg::Input(b"hi".to_vec()))]
        );
        let modes = Modes {
            paste: true,
            mouse: Mouse::Drag,
            ..Modes::default()
        };
        assert_eq!(
            app.on_server(ServerMsg::Frame(frame_with("x", modes))),
            vec![Effect::Mirror(modes)]
        );
        assert_eq!(
            app.on_server(ServerMsg::Frame(frame_with("y", modes))),
            vec![]
        );
    }

    #[test]
    fn tells_the_server_the_pane_size_after_a_resize() {
        let mut app = App::new(SIZE);

        assert_eq!(
            app.on_resize(Size {
                cols: 100,
                rows: 30
            }),
            vec![Effect::Send(ClientMsg::Resize(Size {
                cols: 100,
                rows: 29
            }))]
        );
    }

    #[test]
    fn detaches_and_stops_only_after_a_yes() {
        let mut app = App::new(SIZE);

        assert!(matches!(
            app.on_input(Routed::Command(Command::Detach))[..],
            [Effect::Quit(_)]
        ));
        assert_eq!(app.on_input(Routed::Command(Command::StopServer)), vec![]);
        assert!(app.is_modal());
        assert_eq!(app.on_input(Routed::View(vec![Key::Char('n')])), vec![]);
        assert!(!app.is_modal());
        app.on_input(Routed::Command(Command::StopServer));
        let effects = app.on_input(Routed::View(vec![Key::Char('y')]));
        assert_eq!(effects[0], Effect::Send(ClientMsg::StopServer));
        assert!(matches!(effects[1], Effect::Quit(_)));
    }

    #[test]
    fn lets_go_when_another_ui_attaches() {
        let mut app = App::new(SIZE);

        assert!(matches!(
            app.on_server(ServerMsg::Detached)[..],
            [Effect::Quit(_)]
        ));
    }

    #[test]
    fn draws_the_master_with_a_bottom_bar_and_its_cursor() {
        let mut app = App::new(SIZE);
        app.on_server(ServerMsg::Frame(frame_with("hello", Modes::default())));

        let mut terminal = render(&app);

        assert!(row(&terminal, 0).starts_with("hello"));
        let bar = row(&terminal, 9);
        assert!(bar.contains("≋ flow"), "{bar}");
        assert!(bar.contains("◎ master"), "{bar}");
        assert!(bar.contains("C-space then d detach"), "{bar}");
        terminal.backend_mut().assert_cursor_position((5, 0));
    }

    #[test]
    fn shows_when_the_master_has_exited() {
        let mut app = App::new(SIZE);
        app.on_server(ServerMsg::Panes(vec![PaneInfo {
            key: PaneKey::Master,
            status: PaneStatus::Exited { code: Some(0) },
        }]));

        let terminal = render(&app);

        assert!(row(&terminal, 9).contains("exited - C-space r resumes it"));
        let screen: String = (0..9).map(|y| row(&terminal, y)).collect();
        assert!(screen.contains("The session ended."), "{screen}");
        assert!(screen.contains("C-space r resumes it"), "{screen}");
        assert_eq!(
            app.on_input(Routed::Command(Command::Reopen)),
            vec![Effect::Send(ClientMsg::Reopen(PaneKey::Master))]
        );
    }
}
