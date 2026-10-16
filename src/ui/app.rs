mod board_keys;
mod draw;
mod effect;
mod keys;
mod messages;

pub use effect::{Effect, Job, Removal, outcome};

use std::collections::HashMap;
use std::path::PathBuf;

use crate::launch::worktree::WORKTREES_DIR;
use crate::proto::{Modes, PaneInfo, PaneKey, PaneStatus, Size};
use crate::ui::board::BoardState;
use crate::ui::layout::{self, Areas, Chrome};
use crate::ui::newflow::NewFlowForm;
use crate::ui::peek::PeekLine;
use crate::ui::screen::Screen;
use crate::ui::sidebar::{self, Sidebar};
use crate::ui::theme::Theme;
use crate::view::{Board, FlowView, Projects};

/// How long a flow's row stands out after its state changed.
const CHANGE_SHOWN_MS: u64 = 3_000;
/// Ticks between redraws that only refresh how long ago things changed.
const TICKS_PER_AGE: u32 = 10;
/// How often the board asks `flow status --all` again while every project is in view.
const OVERVIEW_EVERY_MS: u64 = 2_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum View {
    Panes,
    Board(BoardState),
    NewFlow(NewFlowForm),
    /// Scrolled back this many lines in the pane's history (C-space [).
    History(u32),
    /// C-space Q: the ids of the workers that are not finished, which the answer keeps or ends.
    Stop(Vec<String>),
    /// C-space ?: every key.
    Help,
    /// A question before a removal loses work: y runs `then`, any other key keeps the flow.
    Confirm {
        question: String,
        then: Removal,
    },
}

/// What flow ui shows and what it does with input; no I/O of its own.
#[derive(Debug)]
pub struct App {
    size: Size,
    screen: Screen,
    board: Option<Board>,
    /// When each flow's state last changed while this UI watched, by id: its row stands out a moment.
    changed: HashMap<String, u64>,
    panes: Vec<PaneInfo>,
    focus: PaneKey,
    /// The flow whose peek the sidebar shows.
    cursor: Option<String>,
    view: View,
    mirrored: Modes,
    now: u64,
    spin: usize,
    ticks: u32,
    home: Option<String>,
    /// The local time zone's offset from UTC, for clock times.
    offset_secs: i64,
    /// Where the master runs, as shown: ~/projects/shop · main.
    master: Option<String>,
    master_cwd: Option<String>,
    /// A one-line message, such as why a flow command failed, until the next key.
    notice: Option<String>,
    /// The sidebar folded to a rail (C-space b).
    rail: bool,
    /// This project's store, to tell its flows from other projects'.
    here: Option<String>,
    /// Every project's flows, this project first, from the last `flow status --all`.
    projects: Option<Projects>,
    /// When the board last asked for them, and whether that answer is still out.
    overview_at: u64,
    overview_running: bool,
}

impl App {
    pub fn new(size: Size) -> App {
        App {
            size,
            screen: Screen::default(),
            board: None,
            changed: HashMap::new(),
            panes: Vec::new(),
            focus: PaneKey::Master,
            cursor: None,
            view: View::Panes,
            mirrored: Modes::default(),
            now: 0,
            spin: 0,
            ticks: 0,
            home: None,
            offset_secs: 0,
            master: None,
            master_cwd: None,
            notice: None,
            rail: false,
            here: None,
            projects: None,
            overview_at: 0,
            overview_running: false,
        }
    }

    /// Shows a one-line notice until the next key, such as why theme.toml could not be read.
    pub fn say(&mut self, notice: String) {
        self.notice = Some(notice);
    }

    /// say(), with the home folder in it written as ~.
    pub fn say_about_file(&mut self, notice: String) {
        let notice = match self.home.as_deref() {
            Some(home) => notice.replace(&format!("{home}/"), "~/"),
            None => notice,
        };
        self.say(notice);
    }

    /// Board times show this many seconds east of UTC.
    pub fn with_offset(mut self, offset_secs: i64) -> App {
        self.offset_secs = offset_secs;
        self
    }

    /// The store this flow ui belongs to, as `flow status --all` names its root.
    pub fn with_store(mut self, root: String) -> App {
        self.here = Some(root);
        self
    }

    /// Paths under `home` show as ~/...
    pub fn with_home(mut self, home: Option<String>) -> App {
        self.home = home;
        self
    }

    /// The bottom bar while the master is alone, the sidebar once there are workers.
    pub fn areas(&self) -> Areas {
        let chrome = if self.workers().is_empty() {
            Chrome::Solo
        } else if self.rail {
            Chrome::Rail
        } else {
            Chrome::Sidebar
        };
        layout::areas(self.size, chrome)
    }

    /// True while a dialog takes the keys instead of the pane.
    pub fn is_modal(&self) -> bool {
        self.view != View::Panes
    }

    /// The flow under the sidebar cursor, for the peek.
    pub fn peeked(&self) -> Option<&FlowView> {
        let id = self.cursor.as_deref()?;
        self.worker(id)
    }

    /// The focused worker's name when its session is no longer running.
    fn ended_worker(&self) -> Option<String> {
        let PaneKey::Flow(id) = &self.focus else {
            return None;
        };
        match self.status(&self.focus) {
            Some(PaneStatus::Exited { .. } | PaneStatus::Failed { .. }) => {
                self.worker(id).map(|flow| flow.name.clone())
            }
            _ => None,
        }
    }

    /// The repository of the pane in view: the master's directory, or the repo a worker's worktree is in.
    fn focused_repo(&self) -> PathBuf {
        let worktree = match &self.focus {
            PaneKey::Master => None,
            PaneKey::Flow(id) => self
                .worker(id)
                .and_then(|flow| flow.workspace.as_ref())
                .map(|space| space.path.clone()),
        };
        let repo = worktree.and_then(|path| {
            let marker = format!("/{WORKTREES_DIR}/");
            path.find(&marker).map(|at| path[..at].to_string())
        });
        PathBuf::from(
            repo.or_else(|| self.master_cwd.clone())
                .unwrap_or_else(|| ".".to_string()),
        )
    }

    fn tilde(&self, path: &str) -> String {
        match self
            .home
            .as_deref()
            .and_then(|home| path.strip_prefix(home))
        {
            Some(rest) if rest.is_empty() || rest.starts_with('/') => format!("~{rest}"),
            _ => path.to_string(),
        }
    }

    fn worker(&self, id: &str) -> Option<&FlowView> {
        self.workers().into_iter().find(|flow| flow.id == id)
    }

    /// Whether the flow's pane is the one in view.
    fn in_view(&self, id: &str) -> bool {
        matches!(&self.focus, PaneKey::Flow(focused) if focused == id)
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
            master: self.master.as_deref(),
            now: self.now,
            spin: self.spin,
            changed: &self.changed,
            theme,
        }
    }

    fn status(&self, key: &PaneKey) -> Option<&PaneStatus> {
        self.panes
            .iter()
            .find(|pane| &pane.key == key)
            .map(|pane| &pane.status)
    }
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use std::path::Path;

    use ratatui::style::Modifier;

    use super::*;
    use crate::branching::Branching;
    use crate::cli::output;
    use crate::event::{Event, EventKind};
    use crate::fold;
    use crate::proto::{Cell, ClientMsg, Color, Flags, Frame, Mouse, ServerMsg};
    use crate::ui::input::{Command, Key, Routed, Step};
    use crate::view::State;

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
            .draw(|frame| app.draw(frame, &Theme::default(), &[]))
            .unwrap();
        terminal
    }

    /// Row `y` of what flow draws, inside the margin.
    fn row(terminal: &Terminal<TestBackend>, y: u16) -> String {
        let buffer = terminal.backend().buffer();
        let screen = layout::screen(Size {
            cols: buffer.area.width,
            rows: buffer.area.height,
        });
        (screen.x..screen.x + screen.width)
            .map(|x| buffer[(x, screen.y + y)].symbol().to_string())
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
            vec![Effect::Send(ClientMsg::Resize(Size { cols: 98, rows: 25 }))]
        );
    }

    #[test]
    fn draws_a_question_longer_than_any_terminal() {
        let mut app = App::new(SIZE);
        app.view = View::Confirm {
            question: "x".repeat(usize::from(u16::MAX) - 2),
            then: Removal::of("shop-230"),
        };

        let terminal = render(&app);

        assert!((0..8).any(|y| row(&terminal, y).contains("xxx")));
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
            row(&terminal, 0).starts_with(" master "),
            "{}",
            row(&terminal, 0)
        );
        assert!(
            row(&terminal, 1).starts_with("hello"),
            "{}",
            row(&terminal, 1)
        );
        assert!(row(&terminal, 5).trim().is_empty(), "no box under the pane");
        assert_eq!(
            row(&terminal, 6).trim(),
            "",
            "a blank row between session and bar"
        );
        let bar = row(&terminal, 7);
        assert!(bar.contains("▚▄▞ flow"), "{bar}");
        assert!(bar.contains("│ master"), "{bar}");
        assert!(bar.contains("C-space then  n new flow   ? keys"), "{bar}");
        let buffer = terminal.backend().buffer();
        assert!(
            (1..SIZE.cols - 1).all(|x| buffer[(x, 8)].bg == Theme::default().ui.bar.bg.unwrap()),
            "the whole bar has the panel background"
        );
        terminal.backend_mut().assert_cursor_position((6, 2));
    }

    #[test]
    fn shows_when_the_master_has_exited() {
        let mut app = App::new(SIZE);
        app.on_server(ServerMsg::Panes(vec![PaneInfo {
            key: PaneKey::Master,
            status: PaneStatus::Exited { code: Some(0) },
        }]));

        let terminal = render(&app);

        assert!(row(&terminal, 7).contains("exited - C-space r resumes it"));
        let screen: String = (0..7).map(|y| row(&terminal, y)).collect();
        assert!(screen.contains("The session ended."), "{screen}");
        assert!(screen.contains("C-space r resumes it"), "{screen}");
        assert_eq!(
            app.on_input(Routed::Command(Command::Reopen)),
            vec![Effect::Send(ClientMsg::Reopen(PaneKey::Master))]
        );
    }

    #[test]
    fn offers_to_clean_up_a_worker_whose_session_ended() {
        let mut app = board_app();
        app.on_input(Routed::Command(Command::Focus(2)));
        let id = PaneKey::Flow("shop-230".into());
        app.on_server(ServerMsg::Panes(vec![PaneInfo {
            key: id.clone(),
            status: PaneStatus::Exited { code: Some(0) },
        }]));

        let terminal = render_at(&app, FLEET);
        let screen: String = (0..FLEET.rows - 2)
            .map(|y| row(&terminal, y) + "\n")
            .collect();
        assert!(
            screen.contains("x clean up: worktree, branch, flow   r resume   esc back to master"),
            "{screen}"
        );
        assert_eq!(
            app.on_input(Routed::Pane(b"a".to_vec())),
            vec![],
            "the pane is gone"
        );
        assert_eq!(
            app.on_input(Routed::Pane(b"r".to_vec())),
            vec![Effect::Send(ClientMsg::Reopen(id))]
        );
        assert_eq!(
            app.on_input(Routed::Pane(b"x".to_vec())),
            vec![Effect::Run(Job::Remove(Removal::of("shop-230")))]
        );
        assert_eq!(app.notice.as_deref(), Some("cleaning up shop-230…"));
        assert_eq!(
            app.on_input(Routed::Pane(vec![0x1b])),
            vec![Effect::Send(ClientMsg::Focus(PaneKey::Master))]
        );
    }

    #[test]
    fn asks_before_a_clean_up_loses_work() {
        let mut app = board_app();
        let first = Removal::of("shop-230");

        app.on_ran(
            Job::Remove(first.clone()),
            Err(format!("shop-230 {} in /shop", output::remove::UNCOMMITTED)),
        );
        assert!(app.is_modal());
        let screen: String = {
            let terminal = render_at(&app, FLEET);
            (0..FLEET.rows - 2)
                .map(|y| row(&terminal, y) + "\n")
                .collect()
        };
        assert!(
            screen.contains("shop-230 has uncommitted changes. Discard them?"),
            "{screen}"
        );
        let forced = Removal {
            force: true,
            ..first.clone()
        };
        assert_eq!(
            app.on_input(Routed::View(vec![Key::Char('y')])),
            vec![Effect::Run(Job::Remove(forced.clone()))]
        );

        app.on_ran(
            Job::Remove(forced.clone()),
            Err(format!(
                "flow/shop-230 has 2 commits {}",
                output::remove::UNMERGED
            )),
        );
        let View::Confirm { question, then } = &app.view else {
            panic!("expected a question: {:?}", app.view);
        };
        assert_eq!(
            question,
            "flow/shop-230 has 2 commits on no other branch. Delete it anyway?"
        );
        assert!(then.force && then.delete_unmerged);
        assert_eq!(
            Job::Remove(then.clone()).args(),
            ["remove", "shop-230", "--force", "--delete-unmerged"]
        );

        assert_eq!(app.on_input(Routed::View(vec![Key::Char('n')])), vec![]);
        assert!(!app.is_modal(), "any other key keeps the flow");
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
            .draw(|frame| app.draw(frame, &Theme::default(), &[]))
            .unwrap();
        terminal
    }

    /// Row `y` of the sidebar.
    fn side(terminal: &Terminal<TestBackend>, y: u16) -> String {
        let side = layout::areas(FLEET, layout::Chrome::Sidebar).side.unwrap();
        let screen = layout::screen(FLEET);
        let from = (side.x - screen.x) as usize;
        row(terminal, y).chars().skip(from).collect::<String>()
    }

    #[test]
    fn paints_every_cell_on_the_themes_ground() {
        let app = board_app();
        let theme = Theme::from_palette(crate::ui::theme::Palette::FLOW);
        let mut terminal = Terminal::new(TestBackend::new(FLEET.cols, FLEET.rows)).unwrap();

        terminal.draw(|frame| app.draw(frame, &theme, &[])).unwrap();

        let buffer = terminal.backend().buffer();
        let ground = theme.palette.background;
        let painted = (0..FLEET.rows)
            .flat_map(|y| (0..FLEET.cols).map(move |x| (x, y)))
            .filter(|(x, y)| buffer[(*x, *y)].bg == ground)
            .count();
        assert!(
            painted > (FLEET.cols as usize * FLEET.rows as usize) * 9 / 10,
            "{painted} cells on the ground"
        );
        assert_eq!(buffer[(5, 5)].bg, ground, "the pane's empty cells too");
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
                rows: 41
            }))]
        );
        assert_eq!(app.on_server(ServerMsg::Board(fleet())), vec![]);
    }

    #[test]
    fn draws_the_fleet_in_the_sidebar_grouped_by_what_it_needs() {
        let terminal = render_at(&with_fleet(), FLEET);

        assert_eq!(
            side(&terminal, 0),
            " ▚▄▞ flow                               "
        );
        assert_eq!(
            side(&terminal, 2),
            "▌0   master                             "
        );
        assert_eq!(side(&terminal, 4).trim_end(), " NEEDS YOU 2");
        assert_eq!(
            side(&terminal, 5),
            " 1 ● shop-221                       29m "
        );
        assert_eq!(
            side(&terminal, 6),
            "     ●───●───○───○ implement        4/9 "
        );
        assert_eq!(
            side(&terminal, 7),
            "     migration needs a default value f… "
        );
        assert_eq!(
            side(&terminal, 10),
            "     ●───○───○───○ design        guided "
        );
        assert_eq!(side(&terminal, 13).trim_end(), " RUNNING 1");
        assert!(side(&terminal, 14).starts_with(" 3 ◓ export-csv"));
        assert_eq!(side(&terminal, 16).trim_end(), "     agent: review changes");
        assert_eq!(side(&terminal, 18).trim_end(), " DONE 1");
        assert_eq!(
            side(&terminal, 20).trim_end(),
            "     ●───●───●───● MR !418        in 0s",
            "a finished flow says how long it took"
        );
        assert_eq!(side(&terminal, 40).trim_end(), "", "no rule above the hint");
        assert_eq!(side(&terminal, 41).trim_end(), " C-space ? keys");
        let buffer = terminal.backend().buffer();
        assert_eq!(
            buffer[(112, 6)].fg,
            Theme::default().state.blocked.fg.unwrap()
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
            title.starts_with("▌● shop-230  ●───○───○───○ design  · flow/shop-230 · slot 1 "),
            "{title}"
        );
        assert!(
            title.ends_with("  │ ▚▄▞ flow                               "),
            "{title}"
        );
        assert_eq!(
            side(&terminal, 2),
            " 0   master                             "
        );
        assert!(side(&terminal, 9).starts_with("▌2 ● shop-230"));
        let buffer = terminal.backend().buffer();
        let selection = Theme::default().ui.selection.bg.unwrap();
        assert!((109..149).all(|x| buffer[(x, 11)].bg == selection));
        assert_ne!(buffer[(119, 9)].bg, selection);

        assert_eq!(
            app.on_input(Routed::Click { col: 120, row: 7 }),
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

        assert_eq!(side(&terminal, 6).trim_end(), "     ○───○───○───○ waiting");
        assert_eq!(side(&terminal, 7).trim_end(), "     ready");
        assert_eq!(
            side(&terminal, 11).trim_end(),
            "     ○───○───○───○ starting"
        );
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
            .draw(|frame| app.draw(frame, &Theme::default(), peek))
            .unwrap();
        terminal
    }

    #[test]
    fn peeks_into_a_worker_under_the_cursor_and_jumps_in() {
        let mut app = with_fleet();
        app.on_input(Routed::Command(Command::Cursor(Step::Next)));
        app.on_input(Routed::Command(Command::Cursor(Step::Next)));
        app.on_input(Routed::Command(Command::Cursor(Step::Next)));
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
            side(&terminal, 14).starts_with("▌3 ◓ export-csv"),
            "{}",
            side(&terminal, 14)
        );
        assert_eq!(
            side(&terminal, 15).trim_end(),
            "▌    ●───●───●───○ review           6/6"
        );
        assert_eq!(
            side(&terminal, 16).trim_end(),
            format!("▌ {}", "─".repeat(37))
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
            "▌ ◓ Write(ThrottleExportTest.php)"
        );
        assert_eq!(side(&terminal, 22).trim_end(), "▌ enter jump in");
        assert_eq!(side(&terminal, 23).trim_end(), "");
        assert_eq!(side(&terminal, 24).trim_end(), " DONE 1");
        assert_eq!(side(&terminal, 41).trim_end(), " C-space ? keys");

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

        app.on_input(Routed::Command(Command::Cursor(Step::Next)));
        assert_eq!(
            app.peeked().map(|flow| flow.name.as_str()),
            Some("shop-230")
        );
        app.on_input(Routed::Command(Command::Cursor(Step::Prev)));
        assert_eq!(app.peeked(), None);
        app.on_input(Routed::Command(Command::Cursor(Step::Prev)));
        assert_eq!(
            app.peeked().map(|flow| flow.name.as_str()),
            Some("shop-198")
        );
    }

    #[test]
    fn peeks_at_where_an_ended_flows_code_is() {
        let mut app = with_fleet();
        for _ in 0..4 {
            app.on_input(Routed::Command(Command::Cursor(Step::Next)));
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

    #[test]
    fn a_long_place_loses_its_middle_but_keeps_the_branch_and_the_corner() {
        let mut fleet = with_fleet();
        fleet.on_server(ServerMsg::Master {
            cwd: "/private/tmp/claude-501/-Users-jan-projects-flow/9e99090c-c805-4120-a341-a400210fb25e/scratchpad/ui-look/repo".into(),
            branch: Some("main".into()),
        });

        let narrow = Size { cols: 90, rows: 20 };
        let mut solo = App::new(narrow);
        solo.on_server(ServerMsg::Master {
            cwd: "/private/tmp/claude-501/-Users-jan-projects-flow/9e99090c-c805-4120-a341-a400210fb25e/scratchpad/ui-look/repo".into(),
            branch: Some("main".into()),
        });
        let solo = render_at(&solo, narrow);
        let top = row(&solo, 0);
        assert!(top.contains("/private/…/"), "{top}");
        assert!(top.trim_end().ends_with("/repo · main"), "{top}");

        let screen = render_at(&fleet, FLEET);
        assert_eq!(
            side(&screen, 3).trim_end(),
            "     /private/…/ui-look/repo · main"
        );
    }

    #[test]
    fn shows_where_the_master_runs_with_home_as_a_tilde() {
        let master = ServerMsg::Master {
            cwd: "/Users/jan/projects/shop".into(),
            branch: Some("main".into()),
        };
        let mut solo = App::new(SIZE).with_home(Some("/Users/jan".into()));
        solo.on_server(master.clone());
        let mut fleet = with_fleet().with_home(Some("/Users/jan".into()));
        fleet.on_server(master);

        let bar = row(&render(&solo), 7);
        let sidebar = render_at(&fleet, FLEET);

        assert!(bar.contains("│ master  ~/projects/shop · main"), "{bar}");
        let mut wide = App::new(FLEET);
        wide.on_server(ServerMsg::Master {
            cwd: "/shop".into(),
            branch: None,
        });
        let wide = row(&render_at(&wide, FLEET), 41);
        assert!(
            wide.ends_with("no workers yet   │  C-space then  n new flow   ? keys "),
            "{wide}"
        );
        assert!(
            row(&sidebar, 0).starts_with(" master  main "),
            "the sidebar says where it runs: {}",
            row(&sidebar, 0)
        );
        assert_eq!(side(&sidebar, 3).trim_end(), "     ~/projects/shop · main");
        assert_eq!(side(&sidebar, 5).trim_end(), " NEEDS YOU 2");
    }

    #[test]
    fn draws_every_view_in_a_terminal_of_any_size() {
        let views = [
            View::Panes,
            View::Board(BoardState::default()),
            View::Board(BoardState {
                selected: 2,
                all: true,
            }),
            View::NewFlow(NewFlowForm::new(PathBuf::from("/shop"))),
            View::History(3),
            View::Stop(vec!["shop-221".into()]),
            View::Help,
            View::Confirm {
                question: "shop-230 has uncommitted changes. Discard them?".into(),
                then: Removal::of("shop-230"),
            },
        ];
        for view in views {
            for (cols, rows) in
                (0..12).flat_map(|cols| (0..12).map(move |rows| (cols * 7, rows * 3)))
            {
                let mut app = board_app();
                app.rail = cols % 2 == 0;
                app.on_resize(Size { cols, rows });
                app.view = view.clone();
                render_at(&app, Size { cols, rows });
            }
        }
    }

    fn board_app() -> App {
        let mut app = with_fleet().with_home(Some("/Users/jan".into()));
        app.on_server(ServerMsg::Master {
            cwd: "/Users/jan/projects/shop".into(),
            branch: Some("main".into()),
        });
        app
    }

    #[test]
    fn opens_the_board_on_the_card_in_view_and_draws_every_session() {
        let mut app = board_app();
        app.on_input(Routed::Command(Command::Focus(2)));

        app.on_input(Routed::Command(Command::Board));
        let terminal = render_at(&app, FLEET);

        assert!(app.is_modal());
        assert!(row(&terminal, 0).starts_with(" ▚▄▞ flow board "));
        assert!(row(&terminal, 0).ends_with("5 sessions · 2 need you · 1 delegated · 1 done "));
        assert!(
            row(&terminal, 3).starts_with("   0   master                              you "),
            "only the selected card is outlined"
        );
        assert!(row(&terminal, 3).contains(" 1 ● shop-221                        blocked "));
        assert!(row(&terminal, 3).contains("│ 2 ● shop-230                waiting for you │"));
        assert!(row(&terminal, 4).contains("     ~/projects/shop · main"));
        assert!(
            row(&terminal, 5).contains("   4/9 tasks"),
            "{}",
            row(&terminal, 5)
        );
        let shop_230 = row(&terminal, 5)
            .split('│')
            .nth(1)
            .unwrap_or_default()
            .to_string();
        assert!(
            shop_230.trim_start() == "31m ago ",
            "a flow without tasks draws no bar: {shop_230:?}"
        );
        assert!(row(&terminal, 10).contains(" 3 ◓ export-csv            reviewing · agent "));
        assert!(
            row(&terminal, 16)
                .starts_with(" 2 shop-230  shop-230 · guided · flow/shop-230 · slot 1"),
            "{}",
            row(&terminal, 16)
        );
        assert!(
            row(&terminal, 41)
                .starts_with(" 0-9 select   h j k l move   enter jump in   n new flow   x clear finished flow   a every project   esc back to shop-230")
        );
        let buffer = terminal.backend().buffer();
        let accent = Theme::default().ui.accent.fg.unwrap();
        assert_eq!(buffer[(100, 3)].fg, accent, "the selected card is outlined");
        let blocked_at = row(&terminal, 2 + 1).find("blocked").unwrap();
        let blocked_x = row(&terminal, 3)[..blocked_at].chars().count() as u16 + layout::MARGIN;
        assert_eq!(
            buffer[(blocked_x, 3 + layout::MARGIN)].fg,
            Theme::default().state.blocked.fg.unwrap(),
            "the state in its color"
        );
        assert_eq!(
            buffer[(blocked_x, 3 + layout::MARGIN)].bg,
            Theme::default()
                .ui
                .background
                .bg
                .unwrap_or(ratatui::style::Color::Reset),
            "no fill"
        );
    }

    #[test]
    fn an_idle_flow_is_ready_not_running() {
        let mut flows = fleet().flows;
        flows.push(worker(
            "shop-250",
            crate::event::Mode::Guided,
            5,
            vec![EventKind::Idle {
                note: "The design is ready for review.".into(),
            }],
        ));
        let mut app = App::new(FLEET);
        app.on_server(ServerMsg::Board(crate::fold::board(flows, NOW)));
        app.on_tick(NOW);
        let terminal = render_at(&app, FLEET);
        let sections: Vec<String> = (0..FLEET.rows - 2)
            .map(|y| side(&terminal, y).trim_end().to_string())
            .filter(|line| {
                line.starts_with(' ')
                    && line
                        .trim()
                        .chars()
                        .all(|c| c.is_ascii_uppercase() || c == ' ' || c.is_ascii_digit())
                    && !line.trim().is_empty()
            })
            .collect();

        assert_eq!(
            sections,
            [" NEEDS YOU 2", " READY 1", " RUNNING 1", " DONE 1"]
        );
    }

    #[test]
    fn a_row_stands_out_a_moment_after_its_flow_changed() {
        let selection = Theme::default().ui.selection.bg.unwrap();
        let mut app = with_fleet();
        let background_of = |app: &App, text: &str| {
            let terminal = render_at(app, FLEET);
            let y = (0..FLEET.rows - 2)
                .find(|&y| side(&terminal, y).contains(text))
                .unwrap();
            terminal.backend().buffer()[(120, y + layout::MARGIN)].bg
        };
        assert_ne!(background_of(&app, "shop-221"), selection);

        let mut board = fleet();
        let flow = board
            .flows
            .iter_mut()
            .find(|flow| flow.name == "shop-221")
            .unwrap();
        flow.state = State::Working;
        flow.blocker = None;
        app.on_server(ServerMsg::Board(board));

        assert_eq!(
            background_of(&app, "shop-221"),
            selection,
            "it just started working"
        );
        assert_ne!(
            background_of(&app, "shop-230"),
            selection,
            "nothing changed there"
        );
        assert!(
            app.on_tick(NOW + CHANGE_SHOWN_MS),
            "a redraw takes the mark away"
        );
        assert_ne!(background_of(&app, "shop-221"), selection);
    }

    #[test]
    fn notes_stay_behind_the_names_but_the_selected_one() {
        let theme = Theme::default();
        let mut app = with_fleet();
        app.on_input(Routed::Command(Command::Focus(2)));
        let terminal = render_at(&app, FLEET);
        let buffer = terminal.backend().buffer();
        let color_of = |text: &str| {
            let y = (0..FLEET.rows - 2)
                .find(|&y| side(&terminal, y).contains(text))
                .unwrap_or_else(|| panic!("no row with {text}"));
            let x = row(&terminal, y).find(text).unwrap();
            let x = row(&terminal, y)[..x].chars().count() as u16 + layout::MARGIN;
            buffer[(x, y + layout::MARGIN)].fg
        };

        assert_eq!(color_of("migration needs"), theme.ui.faint.fg.unwrap());
        assert_eq!(
            color_of("Which approach"),
            theme.ui.muted.fg.unwrap(),
            "shop-230 is in view"
        );
    }

    #[test]
    fn what_is_done_takes_the_color_of_where_the_flow_is() {
        let theme = Theme::default();
        let blocked = theme.state.blocked.fg.unwrap();
        let faint = theme.ui.faint.fg.unwrap();
        let terminal = render_at(&with_fleet(), FLEET);
        let buffer = terminal.backend().buffer();
        let y = (0..FLEET.rows)
            .find(|&y| side(&terminal, y).contains("●───●───○───○ implement"))
            .expect("shop-221's track")
            + layout::MARGIN;
        let x = (0..FLEET.cols)
            .find(|&x| buffer[(x, y)].symbol() == "●")
            .unwrap();

        assert_eq!(buffer[(x, y)].fg, blocked, "done");
        assert_eq!(
            buffer[(x + 1, y)].fg,
            blocked,
            "the join into the current step"
        );
        assert_eq!(buffer[(x + 4, y)].fg, blocked, "current");
        assert_eq!(buffer[(x + 8, y)].fg, faint, "open");
    }

    #[test]
    fn a_worker_that_needs_you_leads_its_title_with_its_state_color() {
        let theme = Theme::default();
        let lead = |app: &App| {
            let terminal = render_at(app, FLEET);
            let cell = terminal.backend().buffer()[(1, 1)].clone();
            (cell.symbol().to_string(), cell.fg)
        };
        let mut app = with_fleet();

        assert_eq!(lead(&app).0, " ", "the master");
        app.on_input(Routed::Command(Command::Focus(1)));
        assert_eq!(
            lead(&app),
            ("▌".to_string(), theme.state.blocked.fg.unwrap()),
            "shop-221 is blocked"
        );
        app.on_input(Routed::Command(Command::Focus(2)));
        assert_eq!(
            lead(&app),
            ("▌".to_string(), theme.state.waiting.fg.unwrap()),
            "shop-230 waits"
        );
        app.on_input(Routed::Command(Command::Focus(3)));
        assert_eq!(lead(&app).0, " ", "export-csv runs");
    }

    #[test]
    fn the_board_shows_what_needs_you_under_the_master() {
        let mut app = board_app();
        app.on_input(Routed::Command(Command::Board));
        let terminal = render_at(&app, FLEET);
        let rows: Vec<String> = (12..30).map(|y| row(&terminal, y)).collect();
        let at = rows
            .iter()
            .position(|line| line.starts_with(" 0 master"))
            .unwrap_or_else(|| panic!("{}", rows.join("\n")));

        assert!(
            rows[at].contains("~/projects/shop · main · 4 workers"),
            "{}",
            rows[at]
        );
        assert_eq!(rows[at + 2].trim_end(), "  needs you");
        assert!(
            rows[at + 3].starts_with("  ● shop-221  migration needs a default value"),
            "{}",
            rows[at + 3]
        );
        assert!(
            rows[at + 4].starts_with("  ● shop-230  Which approach: a channel column"),
            "{}",
            rows[at + 4]
        );
    }

    #[test]
    fn moves_over_the_board_and_jumps_into_a_card() {
        let mut app = board_app();
        app.on_input(Routed::Command(Command::Board));

        app.on_input(Routed::View(vec![Key::Char('l'), Key::Char('j')]));
        assert_eq!(
            app.view,
            View::Board(BoardState {
                selected: 4,
                all: false
            })
        );
        app.on_input(Routed::View(vec![Key::Char('2')]));
        let terminal = render_at(&app, FLEET);
        assert!(
            row(&terminal, 19).contains("16:09  ● started"),
            "{}",
            row(&terminal, 19)
        );

        assert_eq!(
            app.on_input(Routed::View(vec![Key::Enter])),
            vec![Effect::Send(ClientMsg::Focus(PaneKey::Flow(
                "shop-230".into()
            )))]
        );
        assert_eq!(app.view, View::Panes);
        app.on_input(Routed::Command(Command::Board));
        assert_eq!(app.on_input(Routed::View(vec![Key::Esc])), vec![]);
        assert!(!app.is_modal());
    }

    #[test]
    fn starts_a_flow_from_the_dialog_in_the_repo_in_view() {
        let mut app = board_app();
        app.on_input(Routed::Command(Command::NewFlow));
        app.on_input(Routed::View(
            "SHOP-240 rate-limit the export endpoint"
                .chars()
                .map(Key::Char)
                .collect(),
        ));

        let terminal = render_at(&app, FLEET);
        let dialog: String = (10..24).map(|y| row(&terminal, y) + "\n").collect();
        assert!(
            dialog.contains(
                "│ new flow                                                in ~/projects/shop │"
            ),
            "{dialog}"
        );
        assert!(
            dialog.contains("│ │ SHOP-240 rate-limit the export endpoint"),
            "{dialog}"
        );
        assert!(
            dialog.contains("│ creates flow/shop-240  ·  .flow/worktrees/shop-240  ·  slot 2"),
            "{dialog}"
        );
        assert!(
            terminal.backend().buffer()[(0, 30)]
                .modifier
                .contains(Modifier::DIM)
        );

        let effects = app.on_input(Routed::View(vec![Key::Enter]));
        let [Effect::Run(job)] = &effects[..] else {
            panic!("expected a job: {effects:?}");
        };
        assert_eq!(
            job.args(),
            [
                "start",
                "SHOP-240 rate-limit the export endpoint",
                "--mode",
                "guided"
            ]
        );
        assert_eq!(job.dir(), Some(Path::new("/Users/jan/projects/shop")));
        assert!(!app.is_modal());

        app.on_ran(job.clone(), Err("all 20 slots are in use".into()));
        let View::NewFlow(form) = &app.view else {
            panic!("the dialog comes back");
        };
        assert_eq!(form.request, "SHOP-240 rate-limit the export endpoint");
        assert_eq!(form.error.as_deref(), Some("all 20 slots are in use"));
    }

    #[test]
    fn names_the_branch_after_the_repos_convention_in_the_dialog() {
        let mut app = board_app();
        let repo = PathBuf::from("/Users/jan/projects/shop");

        assert_eq!(
            app.on_input(Routed::Command(Command::NewFlow)),
            vec![Effect::LoadBranching(repo.clone())]
        );
        app.on_branching(
            &repo,
            Ok(Some(Branching {
                name: "{type}/{ticket}-{slug}".into(),
                types: vec!["feature".into(), "bugfix".into()],
            })),
        );
        app.on_input(Routed::View(
            "SHOP-240 fix totals".chars().map(Key::Char).collect(),
        ));
        app.on_input(Routed::View(vec![Key::Tab, Key::Right]));

        let terminal = render_at(&app, FLEET);
        let dialog: String = (10..26).map(|y| row(&terminal, y) + "\n").collect();
        assert!(
            dialog.contains("│ type    ( ) feature   (•) bugfix"),
            "{dialog}"
        );
        assert!(
            dialog.contains("│ creates bugfix/SHOP-240-fix-totals"),
            "{dialog}"
        );
        let effects = app.on_input(Routed::View(vec![Key::Enter]));
        let [Effect::Run(job)] = &effects[..] else {
            panic!("expected a job: {effects:?}");
        };
        assert_eq!(
            job.args(),
            [
                "start",
                "SHOP-240 fix totals",
                "--mode",
                "guided",
                "--type",
                "bugfix"
            ]
        );

        app.on_input(Routed::Command(Command::NewFlow));
        app.on_branching(&repo, Err("/shop/.flow/config.toml is not valid".into()));
        let View::NewFlow(form) = &app.view else {
            panic!("expected the dialog");
        };
        assert_eq!(
            form.error.as_deref(),
            Some("/shop/.flow/config.toml is not valid")
        );
    }

    #[test]
    fn starts_from_a_workers_repo_and_clears_only_finished_flows() {
        let mut app = board_app();
        app.on_input(Routed::Command(Command::Focus(1)));
        app.on_input(Routed::Command(Command::NewFlow));
        let View::NewFlow(form) = &app.view else {
            panic!("expected the dialog");
        };
        assert_eq!(form.repo, PathBuf::from("/shop"));
        app.on_input(Routed::View(vec![Key::Esc]));

        app.on_input(Routed::Command(Command::Board));
        app.on_input(Routed::View(vec![Key::Char('1'), Key::Char('x')]));
        assert_eq!(
            app.notice.as_deref(),
            Some("only a finished flow can be cleared")
        );
        let effects = app.on_input(Routed::View(vec![Key::Char('4'), Key::Char('x')]));
        assert_eq!(
            effects,
            vec![Effect::Run(Job::Remove(Removal::of("shop-198")))]
        );
        app.on_ran(
            Job::Remove(Removal::of("shop-198")),
            Err("could not remove shop-198: permission denied".into()),
        );
        assert_eq!(
            app.notice.as_deref(),
            Some("could not remove shop-198: permission denied")
        );
    }

    fn every_project() -> Projects {
        let api = worker("api-7", crate::event::Mode::Auto, 5, vec![]);
        Projects {
            version: 1,
            now: NOW,
            projects: vec![
                crate::view::Project {
                    name: "api".into(),
                    root: "/state/projects/api".into(),
                    board: fold::board(vec![api], NOW),
                },
                crate::view::Project {
                    name: "shop".into(),
                    root: "/state/projects/shop".into(),
                    board: fleet(),
                },
            ],
        }
    }

    #[test]
    fn shows_every_project_and_jumps_only_into_this_ones_flows() {
        let mut app = board_app().with_store("/state/projects/shop".into());
        app.on_input(Routed::Command(Command::Board));

        let effects = app.on_input(Routed::View(vec![Key::Char('a')]));
        assert_eq!(effects, vec![Effect::Run(Job::Overview)]);
        assert_eq!(Job::Overview.args(), ["status", "--all", "--json"]);
        assert!(row(&render_at(&app, FLEET), 2).contains("reading every project…"));

        let json = serde_json::to_string(&every_project()).unwrap();
        app.on_ran(Job::Overview, Ok(json));
        let terminal = render_at(&app, FLEET);
        assert!(
            row(&terminal, 0).contains("flow board · every project"),
            "{}",
            row(&terminal, 0)
        );
        assert!(
            row(&terminal, 0).contains("2 projects · 5 flows · 2 need you"),
            "{}",
            row(&terminal, 0)
        );
        assert!(
            row(&terminal, 2).starts_with(" shop  4 flows · 2 need you"),
            "{}",
            row(&terminal, 2)
        );
        assert!(row(&terminal, 2).ends_with("this project "));
        let api = (0..42).find(|y| row(&terminal, *y).starts_with(" api  1 flow"));
        assert!(api.is_some(), "this project comes first, then api");
        assert!(
            row(&terminal, 41).starts_with(" h j k l move   enter jump in   n new flow   x clear finished flow   a this project"),
            "{}",
            row(&terminal, 41)
        );

        let first = fleet().flows[0].id.clone();
        assert_eq!(
            app.on_input(Routed::View(vec![Key::Enter])),
            vec![Effect::Send(ClientMsg::Focus(PaneKey::Flow(first)))]
        );
        app.on_input(Routed::Command(Command::Board));
        app.on_input(Routed::View(vec![Key::Char('a')]));
        let keys = vec![Key::Char('l'); 4];
        app.on_input(Routed::View(keys));
        assert_eq!(app.on_input(Routed::View(vec![Key::Enter])), vec![]);
        assert_eq!(app.notice.as_deref(), Some("api-7 runs in project api"));
        assert!(app.is_modal());

        app.on_input(Routed::View(vec![Key::Char('a')]));
        assert_eq!(app.view, View::Board(BoardState::default()));
    }

    #[test]
    fn asks_for_every_project_again_every_two_seconds_while_in_view() {
        let mut app = board_app().with_store("/state/projects/shop".into());
        app.on_input(Routed::Command(Command::Board));
        app.on_input(Routed::View(vec![Key::Char('a')]));

        app.on_tick(NOW + 2_500);
        assert_eq!(app.overview_due(), vec![], "the first answer is still out");
        app.on_ran(Job::Overview, Err("cannot find a state directory".into()));
        assert_eq!(
            app.notice.as_deref(),
            Some("cannot read every project: cannot find a state directory")
        );
        app.on_tick(NOW + 2_500);
        assert_eq!(app.overview_due(), vec![Effect::Run(Job::Overview)]);
        app.on_ran(
            Job::Overview,
            Ok(serde_json::to_string(&every_project()).unwrap()),
        );
        app.on_tick(NOW + 3_000);
        assert_eq!(app.overview_due(), vec![]);

        app.on_input(Routed::View(vec![Key::Esc]));
        app.on_tick(NOW + 9_000);
        assert_eq!(app.overview_due(), vec![], "not once the board is gone");
    }

    #[test]
    fn reads_what_a_flow_command_printed() {
        assert_eq!(
            outcome(true, "flow: shop-240\nid: x\n"),
            Ok("flow: shop-240".into())
        );
        assert_eq!(
            outcome(false, "error: all 20 slots are in use\nhelp[1]: ..."),
            Err("all 20 slots are in use".into())
        );
    }

    #[test]
    fn folds_the_sidebar_to_a_rail_and_back() {
        let mut app = board_app();
        app.on_input(Routed::Command(Command::Focus(1)));

        assert_eq!(
            app.on_input(Routed::Command(Command::Rail)),
            vec![Effect::Send(ClientMsg::Resize(Size {
                cols: 141,
                rows: 41
            }))]
        );
        let terminal = render_at(&app, FLEET);
        let rail = |y| row(&terminal, y).chars().skip(142).collect::<String>();

        assert!(
            row(&terminal, 0).ends_with(" ● 2 shop-230 is waiting  │  C-space b sidebar │ ▚▄▞  "),
            "{}",
            row(&terminal, 0)
        );
        assert_eq!(rail(2), " 0    ");
        assert_eq!(rail(4), "▌1 ●  ");
        assert_eq!(rail(5), " 2 ●  ");
        assert_eq!(rail(7), " 3 ◓  ");
        assert_eq!(rail(9), " 4 ✓  ");
        assert_eq!(rail(41), " C-␣  ");
        assert_eq!(
            app.on_input(Routed::Click { col: 146, row: 6 }),
            vec![Effect::Send(ClientMsg::Focus(PaneKey::Flow(
                "shop-230".into()
            )))]
        );
        assert_eq!(
            app.on_input(Routed::Command(Command::Rail)),
            vec![Effect::Send(ClientMsg::Resize(Size {
                cols: 107,
                rows: 41
            }))]
        );
    }

    #[test]
    fn scrolls_the_history_and_goes_back_to_live() {
        let mut app = App::new(SIZE);

        assert_eq!(
            app.on_input(Routed::Command(Command::History)),
            vec![Effect::Send(ClientMsg::History(Some(0)))]
        );
        assert_eq!(
            app.on_input(Routed::View(vec![Key::Char('k'), Key::Char('k')])),
            vec![
                Effect::Send(ClientMsg::History(Some(1))),
                Effect::Send(ClientMsg::History(Some(2)))
            ]
        );
        app.on_input(Routed::View(vec![Key::Char('g')]));
        let mut top = frame_with("1", Modes::default());
        top.history = Some(120);
        app.on_server(ServerMsg::Frame(top));
        assert_eq!(
            app.view,
            View::History(120),
            "the server says where the history ends"
        );
        assert_eq!(
            app.on_input(Routed::View(vec![Key::Ctrl('d')])),
            vec![Effect::Send(ClientMsg::History(Some(118)))]
        );

        let terminal = render(&app);
        assert!(
            row(&terminal, 5).contains("↑ 118 lines back   k j scroll  g G ends  q live"),
            "{}",
            row(&terminal, 5)
        );
        assert_eq!(
            app.on_input(Routed::View(vec![Key::Char('q')])),
            vec![Effect::Send(ClientMsg::History(None))]
        );
        assert!(!app.is_modal());
    }

    #[test]
    fn keeps_or_ends_unfinished_flows_when_flow_stops() {
        let mut app = board_app();
        app.on_input(Routed::Command(Command::StopServer));
        let terminal = render_at(&app, FLEET);
        let dialog: String = (0..42).map(|y| row(&terminal, y) + "\n").collect();
        assert!(dialog.contains("3 flows are not finished:"), "{dialog}");
        assert!(
            dialog.contains("● shop-221    migration needs a default value"),
            "{dialog}"
        );
        assert!(
            dialog.contains("e end them; branches and worktrees stay until flow clear"),
            "{dialog}"
        );

        assert_eq!(
            app.on_input(Routed::View(vec![Key::Char('y')])),
            vec![],
            "y is not an answer here"
        );
        let kept = app.on_input(Routed::View(vec![Key::Char('k')]));
        assert_eq!(kept[0], Effect::Send(ClientMsg::StopServer));
        assert_eq!(
            kept[1],
            Effect::Quit(
                "stopped flow; 3 unfinished flows resume the next time you run `flow ui`".into()
            )
        );

        let mut app = board_app();
        app.on_input(Routed::Command(Command::StopServer));
        let ended = app.on_input(Routed::View(vec![Key::Char('e')]));
        assert_eq!(ended[0], Effect::Send(ClientMsg::EndFlows));
        assert_eq!(ended[1], Effect::Send(ClientMsg::StopServer));
        let Effect::Quit(message) = &ended[2] else {
            panic!("expected to quit: {ended:?}");
        };
        assert!(
            message.starts_with(
                "stopped flow and ended 3 flows; their code stays until `flow clear`:\n"
            ),
            "{message}"
        );
        assert!(
            message.contains("\n  shop-221  flow/shop-221  /shop/.flow/worktrees/shop-221"),
            "{message}"
        );

        let mut app = board_app();
        app.on_input(Routed::Command(Command::StopServer));
        assert_eq!(app.on_input(Routed::View(vec![Key::Esc])), vec![]);
        assert!(!app.is_modal());
    }

    #[test]
    fn lists_every_key_until_any_key() {
        let mut app = with_fleet();
        app.on_input(Routed::Command(Command::Help));

        let terminal = render_at(&app, FLEET);
        let help: String = (0..42).map(|y| row(&terminal, y) + "\n").collect();
        assert!(help.contains("C-space then"), "{help}");
        assert!(
            help.contains("  b        fold the sidebar to a rail and back"),
            "{help}"
        );
        assert_eq!(side(&terminal, 41).trim_end(), " C-space ? keys");

        app.on_input(Routed::View(vec![Key::Char('x')]));
        assert!(!app.is_modal());
    }

    #[test]
    fn the_new_flow_dialog_keeps_its_title_beside_a_long_place() {
        let mut app = with_fleet();
        app.on_server(ServerMsg::Master {
            cwd: "/private/tmp/claude-501/-Users-jan-projects-flow/9e99090c-c805-4120-a341-a400210fb25e/scratchpad/ui-look/repo".into(),
            branch: None,
        });
        app.on_input(Routed::Command(Command::NewFlow));

        let terminal = render_at(&app, FLEET);
        let header = (5..20)
            .map(|y| row(&terminal, y))
            .find(|line| line.contains(" new flow"))
            .expect("the dialog names itself");

        assert!(header.contains("in /private/…/"), "{header}");
        assert!(header.contains("/ui-look/repo │"), "{header}");
    }

    #[test]
    fn keeps_what_a_new_flow_creates_inside_the_dialog() {
        let mut app = board_app();
        app.on_input(Routed::Command(Command::NewFlow));
        app.on_input(Routed::View(
            "List the files here with ls, then say done"
                .chars()
                .map(Key::Char)
                .collect(),
        ));

        let terminal = render_at(&app, FLEET);
        let creates = (10..24)
            .map(|y| row(&terminal, y))
            .find(|line| line.contains("creates"))
            .unwrap();

        assert!(
            creates.contains("│ creates flow/list-the-files-here  ·  slot 2"),
            "{creates}"
        );
        assert!(creates.contains("  │"), "{creates}");
    }

    /// The whole screen as text, compared with src/ui/golden/<name>.txt. FLOW_UPDATE_GOLDEN=1
    /// writes the file instead, for a change to the look that is meant; a missing file is written
    /// and fails, so a new view is looked at once.
    fn golden(name: &str, terminal: &Terminal<TestBackend>) {
        let buffer = terminal.backend().buffer();
        let text: String = (0..buffer.area.height)
            .map(|y| {
                let line: String = (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol().to_string())
                    .collect();
                line.trim_end().to_string() + "\n"
            })
            .collect();
        let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src/ui/golden")
            .join(format!("{name}.txt"));
        let update = std::env::var_os("FLOW_UPDATE_GOLDEN").is_some();
        match std::fs::read_to_string(&file) {
            Ok(expected) if !update => assert!(
                expected == text,
                "{name} looks different; if that is meant, run FLOW_UPDATE_GOLDEN=1 cargo test\n{text}"
            ),
            found => {
                std::fs::create_dir_all(file.parent().unwrap()).unwrap();
                std::fs::write(&file, &text).unwrap();
                assert!(
                    update || found.is_ok(),
                    "wrote {}; look at it, then run the tests again",
                    file.display()
                );
            }
        }
    }

    #[test]
    fn every_view_looks_as_it_did() {
        let mut solo = App::new(FLEET).with_home(Some("/Users/jan".into()));
        solo.on_server(ServerMsg::Master {
            cwd: "/Users/jan/projects/shop".into(),
            branch: Some("main".into()),
        });
        golden("solo", &render_at(&solo, FLEET));

        let fleet = board_app();
        golden("fleet", &render_at(&fleet, FLEET));

        let mut worker = board_app();
        worker.on_input(Routed::Command(Command::Focus(2)));
        golden("worker", &render_at(&worker, FLEET));

        let mut peek = board_app();
        peek.on_input(Routed::Command(Command::Cursor(Step::Next)));
        golden("peek", &render_at(&peek, FLEET));

        let mut rail = board_app();
        rail.on_input(Routed::Command(Command::Rail));
        golden("rail", &render_at(&rail, FLEET));

        let mut board = board_app();
        board.on_input(Routed::Command(Command::Board));
        golden("board", &render_at(&board, FLEET));

        let mut new_flow = board_app();
        new_flow.on_input(Routed::Command(Command::NewFlow));
        golden("new-flow", &render_at(&new_flow, FLEET));

        let mut help = board_app();
        help.on_input(Routed::Command(Command::Help));
        golden("help", &render_at(&help, FLEET));

        let mut stop = board_app();
        stop.on_input(Routed::Command(Command::StopServer));
        golden("stop", &render_at(&stop, FLEET));
    }
}
