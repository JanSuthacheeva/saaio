use ratatui::Frame as TuiFrame;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, Padding, Paragraph};

use crate::proto::{ClientMsg, Modes, PaneInfo, PaneKey, PaneStatus, ServerMsg, Size};
use crate::ui::input::{Command, Key, Routed};
use crate::ui::layout::{self, Areas, Chrome};
use crate::ui::peek::PeekLine;
use crate::ui::screen::Screen;
use crate::ui::sidebar::{self, Sidebar};
use crate::ui::theme::Theme;
use crate::view::{Board, FlowView, State};

/// Ticks between redraws that only refresh how long ago things changed.
const TICKS_PER_AGE: u32 = 10;

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
    board: Option<Board>,
    panes: Vec<PaneInfo>,
    focus: PaneKey,
    /// The flow whose peek the sidebar shows.
    cursor: Option<String>,
    view: View,
    mirrored: Modes,
    now: u64,
    spin: usize,
    ticks: u32,
}

impl App {
    pub fn new(size: Size) -> App {
        App {
            size,
            screen: Screen::default(),
            board: None,
            panes: Vec::new(),
            focus: PaneKey::Master,
            cursor: None,
            view: View::Panes,
            mirrored: Modes::default(),
            now: 0,
            spin: 0,
            ticks: 0,
        }
    }

    /// The bottom bar while the master is alone, the sidebar once there are workers.
    pub fn areas(&self) -> Areas {
        let chrome = if self.workers().is_empty() {
            Chrome::Solo
        } else {
            Chrome::Sidebar
        };
        layout::areas(self.size, chrome)
    }

    /// The time (Unix ms) ages count to; true when the screen should be drawn again for it:
    /// a spinner turns, or an age may have changed.
    pub fn on_tick(&mut self, now: u64) -> bool {
        self.now = now;
        self.ticks = self.ticks.wrapping_add(1);
        let spinning = self
            .workers()
            .iter()
            .any(|flow| matches!(flow.state, State::Working | State::Delegated));
        if spinning {
            self.spin = self.spin.wrapping_add(1);
        }
        spinning || self.ticks.is_multiple_of(TICKS_PER_AGE)
    }

    /// True while a dialog takes the keys instead of the pane.
    pub fn is_modal(&self) -> bool {
        self.view != View::Panes
    }

    pub fn on_server(&mut self, message: ServerMsg) -> Vec<Effect> {
        match message {
            ServerMsg::Welcome { .. } | ServerMsg::Closed(_) | ServerMsg::NotOpen(_) => vec![],
            ServerMsg::Board(board) => {
                let before = self.areas().pane;
                self.now = self.now.max(board.now);
                self.board = Some(board);
                let mut effects = Vec::new();
                let after = self.areas();
                if after.pane != before {
                    effects.push(Effect::Send(ClientMsg::Resize(after.pane_size())));
                }
                if let PaneKey::Flow(id) = &self.focus
                    && !self.workers().iter().any(|flow| &flow.id == id)
                {
                    effects.extend(self.focus(PaneKey::Master));
                }
                effects
            }
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
            Routed::Click { col, row } if self.view == View::Panes => self.on_click(col, row),
            Routed::Pane(_) | Routed::Click { .. } => vec![],
            Routed::View(keys) => self.on_keys(keys),
            Routed::Command(command) => self.on_command(command),
        }
    }

    pub fn on_resize(&mut self, size: Size) -> Vec<Effect> {
        self.size = size;
        vec![Effect::Send(ClientMsg::Resize(self.areas().pane_size()))]
    }

    /// The flow under the sidebar cursor, for the peek.
    pub fn peeked(&self) -> Option<&FlowView> {
        let id = self.cursor.as_deref()?;
        self.workers().into_iter().find(|flow| flow.id == id)
    }

    pub fn draw(&self, frame: &mut TuiFrame, theme: &Theme, peek: &[PeekLine]) {
        let areas = self.areas();
        frame.render_widget(self.border(theme), areas.frame);
        frame.render_widget(&self.screen, areas.pane);
        if let Some(bar) = areas.bar {
            self.draw_bar(frame, bar, theme);
        }
        if let Some(side) = areas.side {
            frame.render_widget(self.sidebar(theme, peek), side);
        }
        match self.status(&self.focus) {
            Some(PaneStatus::Running) => {}
            Some(status) => draw_not_running(frame, areas.pane, status, theme),
            None if self.focus != PaneKey::Master => draw_no_session(frame, areas.pane, theme),
            None => {}
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
            Command::Cursor(step) => {
                self.move_cursor(step);
                vec![]
            }
            Command::Jump => match self.cursor.clone() {
                Some(id) => self.focus(PaneKey::Flow(id)),
                None => vec![],
            },
            Command::Focus(0) => self.focus(PaneKey::Master),
            Command::Focus(digit) => {
                let Some(flow) = self.workers().get(digit as usize - 1).copied() else {
                    return vec![];
                };
                self.focus(PaneKey::Flow(flow.id.clone()))
            }
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

    fn on_click(&mut self, col: u16, row: u16) -> Vec<Effect> {
        let Some(side) = self.areas().side else {
            return vec![];
        };
        let inside = col >= side.x && col < side.x + side.width && row >= side.y;
        let theme = Theme::builtin();
        match self.sidebar(&theme, &[]).key_at(row - side.y, side.height) {
            Some(key) if inside => self.focus(key),
            _ => vec![],
        }
    }

    fn focus(&mut self, key: PaneKey) -> Vec<Effect> {
        if key == self.focus {
            return vec![];
        }
        self.focus = key.clone();
        self.cursor = None;
        self.screen = Screen::default();
        vec![Effect::Send(ClientMsg::Focus(key))]
    }

    /// Moves the peek cursor over the workers not in view; past either end it goes away.
    fn move_cursor(&mut self, step: i8) {
        let ids: Vec<String> = self
            .workers()
            .into_iter()
            .filter(|flow| self.focus != PaneKey::Flow(flow.id.clone()))
            .map(|flow| flow.id.clone())
            .collect();
        let at = self
            .cursor
            .as_ref()
            .and_then(|id| ids.iter().position(|other| other == id));
        let next = match (at, step > 0) {
            (None, true) => Some(0),
            (None, false) => ids.len().checked_sub(1),
            (Some(at), true) => Some(at + 1),
            (Some(at), false) => at.checked_sub(1),
        };
        self.cursor = next.and_then(|index| ids.get(index)).cloned();
    }

    fn workers(&self) -> Vec<&FlowView> {
        self.board
            .as_ref()
            .map(sidebar::workers)
            .unwrap_or_default()
    }

    fn sidebar<'a>(&'a self, theme: &'a Theme, peek: &'a [PeekLine]) -> Sidebar<'a> {
        static EMPTY: Board = Board {
            version: crate::view::VIEW_VERSION,
            now: 0,
            count: 0,
            needs_you: 0,
            flows: Vec::new(),
        };
        Sidebar {
            board: self.board.as_ref().unwrap_or(&EMPTY),
            panes: &self.panes,
            focus: &self.focus,
            cursor: self.cursor.as_deref(),
            peek,
            now: self.now,
            spin: self.spin,
            theme,
        }
    }

    /// The rounded border around the focused pane: its name in the top edge, and for a worker
    /// its steps, branch and slot, and the way back to the master on the right.
    fn border(&self, theme: &Theme) -> Block<'static> {
        let block = Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(theme.ui.rule);
        let PaneKey::Flow(id) = &self.focus else {
            return block.title(Line::from(vec![
                Span::raw(" "),
                Span::styled(theme.glyphs.master, theme.ui.accent),
                Span::styled(" master ", theme.ui.strong),
            ]));
        };
        let Some(flow) = self.workers().into_iter().find(|flow| &flow.id == id) else {
            return block.title(Line::styled(format!(" {id} "), theme.ui.strong));
        };
        let sidebar = self.sidebar(theme, &[]);
        let mut title = vec![
            Span::raw(" "),
            Span::styled(sidebar.glyph(flow.state), theme.state(flow.state)),
            Span::styled(format!(" {}  ", flow.name), theme.ui.strong),
        ];
        title.extend(sidebar.steps(flow));
        title.push(Span::raw(" "));
        title.push(sidebar.label(flow));
        if let Some(space) = &flow.workspace {
            title.push(Span::styled(
                format!(
                    "  {} {} · slot {} ",
                    theme.glyphs.separator, space.branch, space.slot
                ),
                theme.ui.faint,
            ));
        }
        block.title(Line::from(title)).title(
            Line::from(vec![
                Span::styled(" C-space 0", theme.ui.key),
                Span::styled(" back to master ", theme.ui.faint),
            ])
            .right_aligned(),
        )
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
        frame.render_widget(Block::default().style(theme.ui.bar), area);
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

/// A notice over a worker pane flow ui has no session for, such as a flow that ended before it opened.
fn draw_no_session(frame: &mut TuiFrame, pane: Rect, theme: &Theme) {
    let text = vec![
        Line::styled("flow ui has no session for this flow.", theme.ui.text),
        Line::from(vec![
            Span::styled("C-space r", theme.ui.key),
            Span::styled(" resumes it   ", theme.ui.muted),
            Span::styled("C-space 0", theme.ui.key),
            Span::styled(" back to the master", theme.ui.muted),
        ]),
    ];
    draw_dialog(frame, pane, " not open ", theme.ui.rule, text);
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
    use crate::event::{Event, EventKind};
    use crate::fold;
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
            .draw(|frame| app.draw(frame, &Theme::builtin(), &[]))
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
            vec![Effect::Send(ClientMsg::Resize(Size { cols: 98, rows: 26 }))]
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

        assert!(
            row(&terminal, 0).starts_with("╭ ◎ master ─"),
            "{}",
            row(&terminal, 0)
        );
        assert!(
            row(&terminal, 1).starts_with("│hello"),
            "{}",
            row(&terminal, 1)
        );
        assert!(row(&terminal, 7).starts_with("╰─"));
        assert_eq!(
            row(&terminal, 8).trim(),
            "",
            "a blank row between session and bar"
        );
        let bar = row(&terminal, 9);
        assert!(bar.contains("≋ flow"), "{bar}");
        assert!(bar.contains("◎ master"), "{bar}");
        assert!(bar.contains("C-space then d detach"), "{bar}");
        let buffer = terminal.backend().buffer();
        assert!(
            (0..SIZE.cols).all(|x| buffer[(x, 9)].bg == Theme::builtin().ui.bar.bg.unwrap()),
            "the whole bar has the panel background"
        );
        terminal.backend_mut().assert_cursor_position((6, 1));
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

    const MINUTE: u64 = 60_000;
    const NOW: u64 = 1_000 * MINUTE;

    /// A started flow at `age` minutes ago, with the events after Started and Workspace.
    fn worker(id: &str, mode: crate::event::Mode, age: u64, events: Vec<EventKind>) -> FlowView {
        let at = NOW - age * MINUTE;
        let mut log = vec![
            Event::new(
                at,
                EventKind::Started {
                    name: id.into(),
                    request: id.into(),
                    mode,
                },
            ),
            Event::new(
                at,
                EventKind::Workspace {
                    path: format!("/shop/.flow/worktrees/{id}"),
                    branch: format!("flow/{id}"),
                    slot: 1,
                    window: Some("flow-ui".into()),
                    agent: "claude".into(),
                },
            ),
        ];
        log.extend(events.into_iter().map(|kind| Event::new(at, kind)));
        fold::fold(id, &log, None, NOW).unwrap()
    }

    fn fleet() -> Board {
        use crate::event::Mode::{Auto, Guided};
        use crate::event::Phase::{Design, Implement, Review};
        let phase = |phase| EventKind::Phase { phase };
        let tasks = |done, total| EventKind::Tasks { done, total };
        let flows = vec![
            worker(
                "shop-221",
                Auto,
                29,
                vec![
                    phase(Design),
                    phase(Implement),
                    tasks(4, 9),
                    EventKind::Blocked {
                        reason: "migration needs a default value for orders.exported_at".into(),
                    },
                ],
            ),
            worker(
                "shop-230",
                Guided,
                31,
                vec![
                    phase(Design),
                    EventKind::Waiting {
                        question: "Which approach: a channel column or a join table?".into(),
                    },
                ],
            ),
            worker(
                "export-csv",
                Auto,
                60,
                vec![
                    phase(Design),
                    phase(Implement),
                    phase(Review),
                    tasks(6, 6),
                    EventKind::Delegated {
                        description: "review changes".into(),
                    },
                ],
            ),
            worker(
                "shop-198",
                Auto,
                120,
                vec![EventKind::MergeRequest {
                    url: "https://gitlab.com/shop/-/merge_requests/418".into(),
                }],
            ),
        ];
        fold::board(flows, NOW)
    }

    fn with_fleet() -> App {
        let mut app = App::new(FLEET);
        app.on_server(ServerMsg::Board(fleet()));
        app.on_tick(NOW);
        app
    }

    const FLEET: Size = Size {
        cols: 150,
        rows: 44,
    };

    fn render_at(app: &App, size: Size) -> Terminal<TestBackend> {
        let mut terminal = Terminal::new(TestBackend::new(size.cols, size.rows)).unwrap();
        terminal
            .draw(|frame| app.draw(frame, &Theme::builtin(), &[]))
            .unwrap();
        terminal
    }

    fn side(terminal: &Terminal<TestBackend>, y: u16) -> String {
        row(terminal, y).chars().skip(110).collect::<String>()
    }

    #[test]
    fn makes_room_for_the_sidebar_once_the_first_worker_shows_up() {
        let mut app = App::new(FLEET);

        assert_eq!(
            app.on_server(ServerMsg::Board(fold::board(vec![], NOW))),
            vec![]
        );
        assert_eq!(app.areas().side, None);
        assert_eq!(
            app.on_server(ServerMsg::Board(fleet())),
            vec![Effect::Send(ClientMsg::Resize(Size {
                cols: 107,
                rows: 42
            }))]
        );
        assert_eq!(app.on_server(ServerMsg::Board(fleet())), vec![]);
    }

    #[test]
    fn draws_the_fleet_in_the_sidebar_grouped_by_what_it_needs() {
        let terminal = render_at(&with_fleet(), FLEET);

        assert_eq!(
            side(&terminal, 0),
            " ≋ flow                      2 need you "
        );
        assert_eq!(
            side(&terminal, 2),
            " 0 ◎ master                   ◂ in view "
        );
        assert_eq!(side(&terminal, 4).trim_end(), " NEEDS YOU");
        assert_eq!(
            side(&terminal, 5),
            " 1 ✖ shop-221                       29m "
        );
        assert_eq!(
            side(&terminal, 6),
            "     ●─◉─○─○ implement              4/9 "
        );
        assert_eq!(
            side(&terminal, 7),
            "     migration needs a default value f… "
        );
        assert_eq!(
            side(&terminal, 10),
            "     ◉─○─○─○ design              guided "
        );
        assert_eq!(side(&terminal, 13).trim_end(), " RUNNING");
        assert!(side(&terminal, 14).starts_with(" 3 ⠙ export-csv"));
        assert_eq!(side(&terminal, 16).trim_end(), "     agent: review changes");
        assert_eq!(side(&terminal, 18).trim_end(), " DONE");
        assert_eq!(side(&terminal, 20).trim_end(), "     ●─●─●─● MR !418");
        assert_eq!(side(&terminal, 41), "─".repeat(40));
        assert_eq!(
            side(&terminal, 42).trim_end(),
            " C-space j k peek  0-9 jump  r resume"
        );
        let buffer = terminal.backend().buffer();
        assert_eq!(
            buffer[(113, 5)].fg,
            Theme::builtin().state.blocked.fg.unwrap()
        );
    }

    #[test]
    fn focuses_a_worker_by_digit_or_click_and_marks_it() {
        let mut app = with_fleet();

        assert_eq!(
            app.on_input(Routed::Command(Command::Focus(2))),
            vec![Effect::Send(ClientMsg::Focus(PaneKey::Flow(
                "shop-230".into()
            )))]
        );
        let terminal = render_at(&app, FLEET);
        let title = row(&terminal, 0);
        assert!(
            title.starts_with("╭ ◆ shop-230  ◉─○─○─○ design  │ flow/shop-230 · slot 1 ─"),
            "{title}"
        );
        assert!(
            title.ends_with(" C-space 0 back to master ╮  ≋ flow                      2 need you "),
            "{title}"
        );
        assert_eq!(
            side(&terminal, 2),
            " 0 ◎ master                         you "
        );
        assert!(side(&terminal, 9).starts_with("▌2 ◆ shop-230"));
        let buffer = terminal.backend().buffer();
        let selection = Theme::builtin().ui.selection.bg.unwrap();
        assert!((110..150).all(|x| buffer[(x, 10)].bg == selection));
        assert_ne!(buffer[(120, 8)].bg, selection);

        assert_eq!(
            app.on_input(Routed::Click { col: 120, row: 6 }),
            vec![Effect::Send(ClientMsg::Focus(PaneKey::Flow(
                "shop-221".into()
            )))]
        );
        assert_eq!(app.on_input(Routed::Click { col: 120, row: 30 }), vec![]);
        assert_eq!(app.on_input(Routed::Command(Command::Focus(9))), vec![]);
        assert_eq!(
            app.on_input(Routed::Command(Command::Focus(0))),
            vec![Effect::Send(ClientMsg::Focus(PaneKey::Master))]
        );
    }

    #[test]
    fn shows_workers_that_exited_failed_or_have_no_session() {
        let mut app = with_fleet();
        app.on_server(ServerMsg::Panes(vec![
            PaneInfo {
                key: PaneKey::Flow("shop-221".into()),
                status: PaneStatus::Exited { code: Some(0) },
            },
            PaneInfo {
                key: PaneKey::Flow("shop-230".into()),
                status: PaneStatus::Failed {
                    reason: "no agent named codex".into(),
                },
            },
        ]));
        app.on_input(Routed::Command(Command::Focus(3)));

        let terminal = render_at(&app, FLEET);

        assert!(side(&terminal, 5).ends_with("exited "));
        assert!(side(&terminal, 9).ends_with("failed "));
        assert_eq!(side(&terminal, 11).trim_end(), "     no agent named codex");
        let pane: String = (1..43).map(|y| row(&terminal, y)).collect();
        assert!(
            pane.contains("flow ui has no session for this flow."),
            "{pane}"
        );
    }

    #[test]
    fn turns_the_spinner_only_while_a_worker_works() {
        let mut app = with_fleet();

        assert!(app.on_tick(NOW + 100), "export-csv is delegated");
        let mut idle = App::new(FLEET);
        idle.on_server(ServerMsg::Board(fold::board(vec![], NOW)));
        let redraws = (1..=20)
            .filter(|tick| idle.on_tick(NOW + tick * 100))
            .count();
        assert_eq!(redraws, 2, "only for the ages, once a second");
    }

    #[test]
    fn names_the_state_of_a_flow_that_has_no_phase_yet() {
        use crate::event::Mode::Auto;
        let mut app = App::new(FLEET);
        let flows = vec![
            worker("fresh", Auto, 1, vec![]),
            worker(
                "asks",
                Auto,
                2,
                vec![EventKind::Waiting {
                    question: "ready".into(),
                }],
            ),
        ];
        app.on_server(ServerMsg::Board(fold::board(flows, NOW)));
        app.on_tick(NOW);

        let terminal = render_at(&app, FLEET);

        assert_eq!(side(&terminal, 6).trim_end(), "     ○─○─○─○ waiting");
        assert_eq!(side(&terminal, 7).trim_end(), "     ready");
        assert_eq!(side(&terminal, 11).trim_end(), "     ○─○─○─○ starting");
        assert_eq!(
            side(&terminal, 12).trim_end(),
            "",
            "no note that it started"
        );
    }

    #[test]
    fn goes_back_to_the_master_when_the_focused_flow_is_removed() {
        let mut app = with_fleet();
        app.on_input(Routed::Command(Command::Focus(1)));
        let mut board = fleet();
        board.flows.retain(|flow| flow.name != "shop-221");

        let effects = app.on_server(ServerMsg::Board(board));

        assert_eq!(
            effects,
            vec![Effect::Send(ClientMsg::Focus(PaneKey::Master))]
        );
    }

    fn render_peek(app: &App, peek: &[PeekLine]) -> Terminal<TestBackend> {
        let mut terminal = Terminal::new(TestBackend::new(FLEET.cols, FLEET.rows)).unwrap();
        terminal
            .draw(|frame| app.draw(frame, &Theme::builtin(), peek))
            .unwrap();
        terminal
    }

    #[test]
    fn peeks_into_a_worker_under_the_cursor_and_jumps_in() {
        let mut app = with_fleet();
        app.on_input(Routed::Command(Command::Cursor(1)));
        app.on_input(Routed::Command(Command::Cursor(1)));
        app.on_input(Routed::Command(Command::Cursor(1)));
        assert_eq!(
            app.peeked().map(|flow| flow.name.as_str()),
            Some("export-csv")
        );
        let peek = vec![
            PeekLine::Tool {
                name: "Update".into(),
                target: "app/Http/Middleware/ThrottleExports.php".into(),
            },
            PeekLine::Result("Added 22 lines".into()),
            PeekLine::Text(
                "The limiter keys on the user id, so one export can't starve the rest.".into(),
            ),
            PeekLine::Now("Write(ThrottleExportTest.php)".into()),
        ];

        let terminal = render_peek(&app, &peek);

        assert!(
            side(&terminal, 14).starts_with("▌3 ⠙ export-csv"),
            "{}",
            side(&terminal, 14)
        );
        assert_eq!(
            side(&terminal, 15).trim_end(),
            "▌    ●─●─◉─○ review                 6/6"
        );
        assert_eq!(
            side(&terminal, 16).trim_end(),
            format!("▌ {}", "╌".repeat(37))
        );
        assert_eq!(
            side(&terminal, 17).trim_end(),
            "▌ ⏺ Update(app/Http/Middleware/Thrott…)"
        );
        assert_eq!(side(&terminal, 18).trim_end(), "▌     ⎿  Added 22 lines");
        assert_eq!(
            side(&terminal, 19).trim_end(),
            "▌ ⏺ The limiter keys on the user id, so"
        );
        assert_eq!(
            side(&terminal, 20).trim_end(),
            "▌   one export can't starve the rest."
        );
        assert_eq!(
            side(&terminal, 21).trim_end(),
            "▌ ⠙ Write(ThrottleExportTest.php)"
        );
        assert_eq!(side(&terminal, 22).trim_end(), "▌ enter jump in");
        assert_eq!(side(&terminal, 23).trim_end(), "");
        assert_eq!(side(&terminal, 24).trim_end(), " DONE");
        assert_eq!(
            side(&terminal, 42).trim_end(),
            " C-space j k peek  0-9 jump  r resume"
        );

        assert_eq!(
            app.on_input(Routed::Command(Command::Jump)),
            vec![Effect::Send(ClientMsg::Focus(PaneKey::Flow(
                "export-csv".into()
            )))]
        );
        assert_eq!(app.peeked(), None);
    }

    #[test]
    fn moves_the_cursor_past_the_ends_and_skips_the_flow_in_view() {
        let mut app = with_fleet();
        app.on_input(Routed::Command(Command::Focus(1)));

        app.on_input(Routed::Command(Command::Cursor(1)));
        assert_eq!(
            app.peeked().map(|flow| flow.name.as_str()),
            Some("shop-230")
        );
        app.on_input(Routed::Command(Command::Cursor(-1)));
        assert_eq!(app.peeked(), None);
        app.on_input(Routed::Command(Command::Cursor(-1)));
        assert_eq!(
            app.peeked().map(|flow| flow.name.as_str()),
            Some("shop-198")
        );
    }

    #[test]
    fn peeks_at_where_an_ended_flows_code_is() {
        let mut app = with_fleet();
        for _ in 0..4 {
            app.on_input(Routed::Command(Command::Cursor(1)));
        }
        assert_eq!(
            app.peeked().map(|flow| flow.name.as_str()),
            Some("shop-198")
        );
        let peek = vec![PeekLine::Code {
            branch: "flow/shop-198".into(),
            worktree: "/shop/.flow/worktrees/shop-198".into(),
            exists: false,
        }];

        let terminal = render_peek(&app, &peek);

        assert_eq!(side(&terminal, 22).trim_end(), "▌ branch   flow/shop-198");
        assert_eq!(side(&terminal, 23).trim_end(), "▌ worktree gone");
    }
}
