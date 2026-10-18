use std::path::Path;

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::style::Modifier;

use super::*;
use crate::branching::Branching;
use crate::cli::output::Failure;
use crate::cli::output::remove::{self, Refusal};
use crate::event::{Event, EventKind};
use crate::fold;
use crate::proto::{Cell, ClientMsg, Color, Flags, Frame, Mouse, ServerMsg};
use crate::ui::input::{Command, Key, Routed, Step};
use crate::ui::theme::PhaseColors;
use crate::view::{MasterTurn, State, VIEW_VERSION};

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

/// Row `y` of what saaio draws, inside the margin.
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
                phases: None,
            },
        ),
        Event::new(
            at,
            EventKind::Workspace {
                path: format!("/shop/.saaio/worktrees/{id}"),
                branch: Some(format!("flow/{id}")),
                slot: Some(1),
                window: Some("saaio-ui".into()),
                agent: "claude".into(),
            },
        ),
    ];
    log.extend(events.into_iter().map(|kind| Event::new(at, kind)));
    fold::fold(id, &log, None, NOW).unwrap()
}

/// A plain session in `folder`, started `age` minutes ago, with the events after its Workspace.
fn plain_session(id: &str, folder: &str, age: u64, events: Vec<EventKind>) -> FlowView {
    let at = NOW - age * MINUTE;
    let mut log = vec![
        Event::new(
            at,
            EventKind::Started {
                name: id.into(),
                request: String::new(),
                mode: crate::event::Mode::Plain,
                phases: None,
            },
        ),
        Event::new(
            at,
            EventKind::Workspace {
                path: folder.into(),
                branch: None,
                slot: None,
                window: Some("saaio-ui".into()),
                agent: "claude".into(),
            },
        ),
    ];
    log.extend(events.into_iter().map(|kind| Event::new(at, kind)));
    fold::fold(id, &log, None, NOW).unwrap()
}

fn fleet() -> Board {
    use crate::event::Mode::{Auto, Guided};
    use crate::event::Phase;
    let phase = |name| EventKind::Phase {
        phase: Phase::of(name),
    };
    let tasks = |done, total| EventKind::Tasks { done, total };
    let flows = vec![
        worker(
            "shop-221",
            Auto,
            29,
            vec![
                phase("plan"),
                phase("implement"),
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
                phase("plan"),
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
                phase("plan"),
                phase("implement"),
                phase("review"),
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
            vec![
                EventKind::MergeRequest {
                    url: "https://gitlab.com/shop/-/merge_requests/418".into(),
                },
                EventKind::Ended,
            ],
        ),
    ];
    fold::board(flows, NOW)
}

/// Where the master's turn stands, as the server says it.
fn master_at(state: State, at: u64) -> ServerMsg {
    ServerMsg::Master {
        cwd: "/Users/jan/projects/shop".into(),
        branch: Some("main".into()),
        turn: Some(MasterTurn { state, at }),
    }
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
    render_with(app, size, &Theme::default())
}

fn render_with(app: &App, size: Size, theme: &Theme) -> Terminal<TestBackend> {
    let mut terminal = Terminal::new(TestBackend::new(size.cols, size.rows)).unwrap();
    terminal.draw(|frame| app.draw(frame, theme, &[])).unwrap();
    terminal
}

/// The default theme at `level`.
fn theme_at(level: PhaseColors) -> Theme {
    Theme {
        phase_colors: level,
        ..Theme::default()
    }
}

/// Row `y` of the sidebar.
fn side(terminal: &Terminal<TestBackend>, y: u16) -> String {
    let side = layout::areas(FLEET, layout::Chrome::Sidebar).side;
    let screen = layout::screen(FLEET);
    let from = (side.x - screen.x) as usize;
    row(terminal, y).chars().skip(from).collect::<String>()
}

fn render_peek(app: &App, peek: &[PeekLine]) -> Terminal<TestBackend> {
    let mut terminal = Terminal::new(TestBackend::new(FLEET.cols, FLEET.rows)).unwrap();
    terminal
        .draw(|frame| app.draw(frame, &Theme::default(), peek))
        .unwrap();
    terminal
}

fn board_app() -> App {
    let mut app = with_fleet().with_home(Some("/Users/jan".into()));
    app.on_server(ServerMsg::Master {
        cwd: "/Users/jan/projects/shop".into(),
        branch: Some("main".into()),
        turn: None,
    });
    app
}

fn every_project() -> Projects {
    let api = worker("api-7", crate::event::Mode::Auto, 5, vec![]);
    Projects {
        version: VIEW_VERSION,
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

mod board;
mod dialogs;
mod golden;
mod panes;
mod prefix;
mod sidebar;
