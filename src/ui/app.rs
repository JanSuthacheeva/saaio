mod board_keys;
mod chrome;
mod draw;
mod effect;
mod keys;
mod messages;
mod pointer;

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
use crate::ui::prefix::Prefix;
use crate::ui::screen::Screen;
use crate::ui::selection::Selection;
use crate::ui::sidebar::{self, Sidebar};
use crate::ui::theme::Theme;
use crate::view::{Board, FlowView, MasterTurn, Projects, State};

/// How long a flow's row stands out after its state changed.
const CHANGE_SHOWN_MS: u64 = 3_000;
/// Ticks between redraws that only refresh how long ago things changed.
const TICKS_PER_AGE: u32 = 10;
/// How often the board asks `saaio status --all` again while every project is in view.
const OVERVIEW_EVERY_MS: u64 = 2_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum View {
    Panes,
    Board(BoardState),
    NewFlow(NewFlowForm),
    /// Scrolled back this many lines in the pane's history (the prefix and [).
    History(u32),
    /// The prefix and Q: the ids of the workers that are not finished, which the answer keeps or ends.
    Stop(Vec<String>),
    /// The prefix and ?: every key.
    Help,
    /// A question before a removal loses work: y runs `then`, any other key keeps the flow.
    Confirm {
        question: String,
        then: Removal,
    },
}

/// What saaio ui shows and what it does with input; no I/O of its own.
#[derive(Debug)]
pub struct App {
    size: Size,
    screen: Screen,
    board: Option<Board>,
    /// When each pane's state last changed while this UI watched: its row stands out a moment.
    changed: HashMap<PaneKey, u64>,
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
    master_turn: Option<MasterTurn>,
    /// A turn of the master ended, asked or wants permission while its pane was not in view.
    master_unseen: bool,
    /// A one-line message, such as why a flow command failed, until the next key.
    notice: Option<String>,
    /// The sidebar folded to a rail: Some once the prefix and b chose, None follows the width (RAIL_BELOW).
    rail: Option<bool>,
    /// This project's store, to tell its flows from other projects'.
    here: Option<String>,
    /// Every project's flows, this project first, from the last `saaio status --all`.
    projects: Option<Projects>,
    /// When the board last asked for them, and whether that answer is still out.
    overview_at: u64,
    overview_running: bool,
    /// The agents the new-flow dialog offers, the default first.
    agents: Vec<String>,
    /// The key before every command, for the hints that name it.
    prefix: Prefix,
    /// Text selected with the mouse, highlighted until the next input.
    selection: Option<Selection>,
    clicks: Option<pointer::Clicks>,
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
            master_turn: None,
            master_unseen: false,
            notice: None,
            rail: None,
            here: None,
            projects: None,
            overview_at: 0,
            overview_running: false,
            agents: Vec::new(),
            prefix: Prefix::default(),
            selection: None,
            clicks: None,
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

    /// The store this saaio ui belongs to, as `saaio status --all` names its root.
    pub fn with_store(mut self, root: String) -> App {
        self.here = Some(root);
        self
    }

    /// The agents the new-flow dialog offers, the default first; an open dialog keeps its list.
    pub fn set_agents(&mut self, agents: Vec<String>) {
        self.agents = agents;
    }

    /// The key before every command, for the hints that name it.
    pub fn set_prefix(&mut self, prefix: Prefix) {
        self.prefix = prefix;
    }

    /// Paths under `home` show as ~/...
    pub fn with_home(mut self, home: Option<String>) -> App {
        self.home = home;
        self
    }

    /// The sidebar or rail, from the start: the master has its row even before there are workers.
    pub fn areas(&self) -> Areas {
        self.areas_in(self.size)
    }

    /// The areas in a terminal of `size`. A frame can be smaller than the size the app knows,
    /// when the terminal shrank and its resize has not arrived yet; draw lays out for the frame.
    fn areas_in(&self, size: Size) -> Areas {
        let chrome = if self.folded() {
            Chrome::Rail
        } else {
            Chrome::Sidebar
        };
        layout::areas(size, chrome)
    }

    /// Whether the rail shows instead of the sidebar: the user's choice, else a narrow terminal.
    fn folded(&self) -> bool {
        self.rail.unwrap_or(self.size.cols < layout::RAIL_BELOW)
    }

    /// The focused pane's modes, for the router's Target.
    pub fn pane_modes(&self) -> Modes {
        self.screen.modes
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

    /// What row 0's glyph shows: Working or Delegated as they are; Waiting for an unseen Idle or
    /// Waiting, Blocked for an unseen Blocked; None once seen.
    fn master_state(&self) -> Option<State> {
        match self.master_turn?.state {
            state @ (State::Working | State::Delegated) => Some(state),
            State::Idle | State::Waiting if self.master_unseen => Some(State::Waiting),
            State::Blocked if self.master_unseen => Some(State::Blocked),
            _ => None,
        }
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
            master_state: self.master_state(),
            now: self.now,
            spin: self.spin,
            changed: &self.changed,
            home: self.home.as_deref(),
            prefix: self.prefix,
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
