mod board_keys;
mod chrome;
mod draw;
mod effect;
mod keys;
mod messages;

pub use effect::{Effect, Job, JobError, Removal, outcome};

use std::collections::HashMap;
use std::path::PathBuf;

use crate::launch::worktree::WORKTREES_DIR;
use crate::proto::{Modes, PaneInfo, PaneKey, PaneStatus, Size};
use crate::ui::board::BoardState;
use crate::ui::fit;
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
    /// The agents the new-flow dialog offers, the default first.
    agents: Vec<String>,
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
            agents: Vec::new(),
        }
    }

    /// Shows a one-line notice until the next key, such as why theme.toml could not be read.
    pub fn say(&mut self, notice: String) {
        self.notice = Some(notice);
    }

    /// Clears the notice when it starts with `prefix`, such as a theme error once the theme reads.
    pub fn take_back(&mut self, prefix: &str) {
        if self
            .notice
            .as_deref()
            .is_some_and(|notice| notice.starts_with(prefix))
        {
            self.notice = None;
        }
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

    /// The agents the new-flow dialog offers, the default first.
    pub fn with_agents(mut self, agents: Vec<String>) -> App {
        self.agents = agents;
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

    /// The repository of the pane in view: the master's directory, the repo a worker's worktree is
    /// in, or a plain session's folder.
    fn focused_repo(&self) -> PathBuf {
        let focused = match &self.focus {
            PaneKey::Master => None,
            PaneKey::Flow(id) => self.worker(id),
        };
        if let Some(flow) = focused.filter(|flow| flow.is_plain())
            && let Some(space) = &flow.workspace
        {
            return PathBuf::from(&space.path);
        }
        let worktree = focused
            .and_then(|flow| flow.workspace.as_ref())
            .map(|space| space.path.clone());
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
        fit::tilde(path, self.home.as_deref())
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
            home: self.home.as_deref(),
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
mod tests;
