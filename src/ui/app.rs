use std::path::{Path, PathBuf};

use ratatui::Frame as TuiFrame;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, Padding, Paragraph};

use crate::event::Mode;
use crate::launch::WORKTREES_DIR;
use crate::proto::{ClientMsg, Modes, PaneInfo, PaneKey, PaneStatus, ServerMsg, Size};
use crate::ui::board::{self, BoardState, BoardView};
use crate::ui::input::{Command, Key, Routed};
use crate::ui::layout::{self, Areas, Chrome};
use crate::ui::newflow::{FormAction, NewFlowForm, NewFlowView};
use crate::ui::peek::PeekLine;
use crate::ui::rail::Rail;
use crate::ui::screen::{Screen, ScreenView};
use crate::ui::sidebar::{self, Sidebar};
use crate::ui::theme::Theme;
use crate::view::{Board, FlowView, Projects, State};

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
}

/// A flow command flow ui runs in the background.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Job {
    Start(NewFlowForm),
    Remove(String),
    /// `flow status --all --json`: every project's board.
    Overview,
}

impl Job {
    /// The arguments after the flow binary.
    pub fn args(&self) -> Vec<String> {
        match self {
            Job::Start(form) => vec![
                "start".to_string(),
                form.request.trim().to_string(),
                "--mode".to_string(),
                match form.mode {
                    Mode::Auto => "auto".to_string(),
                    Mode::Guided => "guided".to_string(),
                },
            ],
            Job::Remove(name) => vec!["remove".to_string(), name.clone()],
            Job::Overview => vec![
                "status".to_string(),
                "--all".to_string(),
                "--json".to_string(),
            ],
        }
    }

    /// Where it runs: the repo a flow starts in.
    pub fn dir(&self) -> Option<&Path> {
        match self {
            Job::Start(form) => Some(&form.repo),
            Job::Remove(_) | Job::Overview => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    Send(ClientMsg),
    Mirror(Modes),
    Run(Job),
    Quit(String),
}

/// h j k l and the arrows on a board.
fn board_step(key: &Key) -> Option<isize> {
    match key {
        Key::Char(c) => BoardState::step(*c),
        Key::Left => Some(-1),
        Key::Right => Some(1),
        Key::Down => BoardState::step('j'),
        Key::Up => BoardState::step('k'),
        _ => None,
    }
}

/// What a flow command printed: its first line when it worked, the error without "error: " when not.
pub fn outcome(success: bool, stdout: &str) -> Result<String, String> {
    let first = stdout.lines().next().unwrap_or("").trim();
    if success {
        Ok(first.to_string())
    } else {
        Err(first.strip_prefix("error: ").unwrap_or(first).to_string())
    }
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
            panes: Vec::new(),
            focus: PaneKey::Master,
            cursor: None,
            view: View::Panes,
            mirrored: Modes::default(),
            now: 0,
            spin: 0,
            ticks: 0,
            home: None,
            offset_secs: board::local_offset_secs(),
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

    /// Job::Overview when every project is in view and the last answer is older than 2 s.
    pub fn overview_due(&mut self) -> Vec<Effect> {
        let all = matches!(self.view, View::Board(BoardState { all: true, .. }));
        if !all || self.overview_running || self.now < self.overview_at + OVERVIEW_EVERY_MS {
            return vec![];
        }
        self.ask_overview()
    }

    fn ask_overview(&mut self) -> Vec<Effect> {
        self.overview_at = self.now;
        self.overview_running = true;
        vec![Effect::Run(Job::Overview)]
    }

    /// True while a dialog takes the keys instead of the pane.
    pub fn is_modal(&self) -> bool {
        self.view != View::Panes
    }

    pub fn on_server(&mut self, message: ServerMsg) -> Vec<Effect> {
        match message {
            ServerMsg::Welcome { .. } | ServerMsg::Closed(_) | ServerMsg::NotOpen(_) => vec![],
            ServerMsg::Master { cwd, branch } => {
                let place = self.tilde(&cwd);
                self.master_cwd = Some(cwd);
                self.master = Some(match branch {
                    Some(branch) => format!("{place} · {branch}"),
                    None => place,
                });
                vec![]
            }
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
                if let (View::History(_), Some(back)) = (&self.view, frame.history) {
                    self.view = View::History(back);
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
        if !matches!(routed, Routed::Pane(_)) {
            self.notice = None;
        }
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
        frame.render_widget(Block::default().style(theme.ui.background), frame.area());
        if let View::Board(state) = self.view {
            let back = match &self.focus {
                PaneKey::Master => "master".to_string(),
                PaneKey::Flow(id) => self
                    .workers()
                    .into_iter()
                    .find(|flow| &flow.id == id)
                    .map_or(id.clone(), |flow| flow.name.clone()),
            };
            frame.render_widget(
                BoardView {
                    sidebar: self.sidebar(theme, &[]),
                    state,
                    back: &back,
                    offset_secs: self.offset_secs,
                    notice: self.notice.as_deref(),
                    projects: self.projects.as_ref(),
                    here: self.here.as_deref(),
                },
                frame.area(),
            );
            return;
        }
        let areas = self.areas();
        frame.render_widget(self.border(theme), areas.frame);
        frame.render_widget(
            ScreenView {
                screen: &self.screen,
                text: theme.palette.text,
                background: theme.palette.background,
            },
            areas.pane,
        );
        if let Some(bar) = areas.bar {
            self.draw_bar(frame, bar, theme);
        }
        if let Some(side) = areas.side {
            if self.rail {
                frame.render_widget(
                    Rail {
                        sidebar: self.sidebar(theme, &[]),
                    },
                    side,
                );
            } else {
                frame.render_widget(self.sidebar(theme, peek), side);
            }
        }
        match self.status(&self.focus) {
            Some(PaneStatus::Running) => {}
            Some(status) => draw_not_running(frame, areas.pane, status, theme),
            None if self.focus != PaneKey::Master => draw_no_session(frame, areas.pane, theme),
            None => {}
        }
        match self.view {
            View::Stop(ref unfinished) => self.draw_stop(frame, areas.pane, theme, unfinished),
            View::Help => draw_help(frame, areas.pane, theme),
            View::Board(_) => {}
            View::History(back) => {
                let hint = Line::from(vec![
                    Span::styled(format!(" ↑ {back} lines back   "), theme.state.waiting),
                    Span::styled("k j", theme.ui.key),
                    Span::styled(" scroll  ", theme.ui.faint),
                    Span::styled("g G", theme.ui.key),
                    Span::styled(" ends  ", theme.ui.faint),
                    Span::styled("q", theme.ui.key),
                    Span::styled(" live ", theme.ui.faint),
                ]);
                let edge = Rect::new(
                    areas.frame.x,
                    areas.frame.y + areas.frame.height.saturating_sub(1),
                    areas.frame.width,
                    1,
                );
                frame.render_widget(
                    Paragraph::new(hint).alignment(Alignment::Right),
                    Rect {
                        width: edge.width.saturating_sub(2),
                        ..edge
                    },
                );
            }
            View::NewFlow(ref form) => {
                let flows: Vec<FlowView> = self.workers().into_iter().cloned().collect();
                let repo = form.repo.display().to_string();
                let screen = frame.area();
                frame.buffer_mut().set_style(screen, Modifier::DIM);
                frame.render_widget(
                    NewFlowView {
                        form,
                        flows: &flows,
                        place: &self.tilde(&repo),
                        theme,
                    },
                    areas.frame,
                );
                if let Some(cursor) = form.cursor(areas.frame) {
                    frame.set_cursor_position(cursor);
                }
            }
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
                let unfinished = self
                    .workers()
                    .into_iter()
                    .filter(|flow| !flow.is_finished())
                    .map(|flow| flow.id.clone())
                    .collect();
                self.view = View::Stop(unfinished);
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
            Command::Board => {
                let selected = match &self.focus {
                    PaneKey::Master => 0,
                    PaneKey::Flow(id) => self
                        .workers()
                        .iter()
                        .position(|flow| &flow.id == id)
                        .map_or(0, |at| at + 1),
                };
                self.view = View::Board(BoardState {
                    selected,
                    all: false,
                });
                vec![]
            }
            Command::Rail => {
                let before = self.areas().pane;
                self.rail = !self.rail;
                self.cursor = None;
                let after = self.areas();
                if after.pane == before {
                    return vec![];
                }
                vec![Effect::Send(ClientMsg::Resize(after.pane_size()))]
            }
            Command::History => {
                self.view = View::History(0);
                vec![Effect::Send(ClientMsg::History(Some(0)))]
            }
            Command::Help => {
                self.view = View::Help;
                vec![]
            }
            Command::NewFlow => {
                self.view = View::NewFlow(NewFlowForm::new(self.focused_repo()));
                vec![]
            }
            Command::Focus(0) => self.focus(PaneKey::Master),
            Command::Focus(digit) => {
                let Some(flow) = self.workers().get(digit as usize - 1).copied() else {
                    return vec![];
                };
                self.focus(PaneKey::Flow(flow.id.clone()))
            }
        }
    }

    fn on_keys(&mut self, keys: Vec<Key>) -> Vec<Effect> {
        let mut effects = Vec::new();
        for key in keys {
            if !self.is_modal() {
                break;
            }
            effects.extend(self.on_key(key));
        }
        effects
    }

    fn on_key(&mut self, key: Key) -> Vec<Effect> {
        if let View::Board(state) = self.view {
            return self.on_board_key(state, key);
        }
        if let View::History(back) = self.view {
            return self.on_history_key(back, key);
        }
        if let View::NewFlow(form) = &mut self.view {
            return match form.on_key(key) {
                FormAction::Edit => vec![],
                FormAction::Cancel => {
                    self.view = View::Panes;
                    vec![]
                }
                FormAction::Submit => {
                    let job = Job::Start(form.clone());
                    self.view = View::Panes;
                    self.notice = Some("starting the flow…".to_string());
                    vec![Effect::Run(job)]
                }
            };
        }
        if let View::Stop(unfinished) = &self.view {
            let unfinished = unfinished.clone();
            return self.on_stop_key(&unfinished, key);
        }
        self.view = View::Panes;
        vec![]
    }

    /// With every worker finished: y stops, any other key keeps flow running. With some unfinished:
    /// k keeps them for the next flow ui, e ends them, esc goes back.
    fn on_stop_key(&mut self, unfinished: &[String], key: Key) -> Vec<Effect> {
        let stop =
            |message: String| vec![Effect::Send(ClientMsg::StopServer), Effect::Quit(message)];
        if unfinished.is_empty() {
            if key == Key::Char('y') {
                return stop("stopped flow and every session in it".to_string());
            }
            self.view = View::Panes;
            return vec![];
        }
        match key {
            Key::Char('k') => stop(format!(
                "stopped flow; {} unfinished flow{} resume the next time you run `flow ui`",
                unfinished.len(),
                if unfinished.len() == 1 { "" } else { "s" }
            )),
            Key::Char('e') => {
                let mut message = format!(
                    "stopped flow and ended {} flow{}; their code stays until `flow clear`:",
                    unfinished.len(),
                    if unfinished.len() == 1 { "" } else { "s" }
                );
                for flow in self
                    .workers()
                    .into_iter()
                    .filter(|flow| unfinished.contains(&flow.id))
                {
                    if let Some(space) = &flow.workspace {
                        message.push_str(&format!(
                            "\n  {}  {}  {}",
                            flow.name,
                            space.branch,
                            self.tilde(&space.path)
                        ));
                    }
                }
                let mut effects = vec![Effect::Send(ClientMsg::EndFlows)];
                effects.extend(stop(message));
                effects
            }
            Key::Esc => {
                self.view = View::Panes;
                vec![]
            }
            _ => vec![],
        }
    }

    /// k j a line, C-u C-d half a page, C-b C-f a page, g G the ends, q or esc back to live.
    fn on_history_key(&mut self, back: u32, key: Key) -> Vec<Effect> {
        let page = self.areas().pane.height.max(2) as i64;
        let step: i64 = match key {
            Key::Char('k') | Key::Up => 1,
            Key::Char('j') | Key::Down => -1,
            Key::Ctrl('u') => page / 2,
            Key::Ctrl('d') => -(page / 2),
            Key::Ctrl('b') => page,
            Key::Ctrl('f') => -page,
            Key::Char('g') => i64::from(u32::MAX),
            Key::Char('G') => -i64::from(u32::MAX),
            Key::Char('q') | Key::Esc => {
                self.view = View::Panes;
                return vec![Effect::Send(ClientMsg::History(None))];
            }
            _ => return vec![],
        };
        let next = (i64::from(back) + step).clamp(0, i64::from(u32::MAX)) as u32;
        self.view = View::History(next);
        vec![Effect::Send(ClientMsg::History(Some(next)))]
    }

    fn on_board_key(&mut self, state: BoardState, key: Key) -> Vec<Effect> {
        if state.all {
            return self.on_all_key(state, key);
        }
        let cards = self.workers().len() + 1;
        if let Some(step) = board_step(&key) {
            self.view = View::Board(state.moved(step, cards));
            return vec![];
        }
        match key {
            Key::Char(digit @ '0'..='9') => {
                let selected = digit as usize - '0' as usize;
                if selected < cards {
                    self.view = View::Board(BoardState { selected, ..state });
                }
                vec![]
            }
            Key::Enter => {
                self.view = View::Panes;
                let key = match state.selected.checked_sub(1) {
                    None => PaneKey::Master,
                    Some(at) => match self.workers().get(at) {
                        Some(flow) => PaneKey::Flow(flow.id.clone()),
                        None => return vec![],
                    },
                };
                self.focus(key)
            }
            Key::Char('a') => {
                self.view = View::Board(BoardState {
                    selected: 0,
                    all: true,
                });
                if self.overview_running {
                    return vec![];
                }
                self.ask_overview()
            }
            Key::Char('x') => {
                let flow = state
                    .selected
                    .checked_sub(1)
                    .and_then(|at| self.workers().get(at).copied())
                    .cloned();
                self.clear(flow)
            }
            _ => self.on_any_board_key(key),
        }
    }

    /// Keys on every project's board: the cards of other projects are only shown.
    fn on_all_key(&mut self, state: BoardState, key: Key) -> Vec<Effect> {
        let flows: Vec<(String, String, FlowView)> = self
            .projects
            .as_ref()
            .map(|projects| {
                board::every_flow(projects)
                    .into_iter()
                    .map(|(project, flow)| {
                        (project.name.clone(), project.root.clone(), flow.clone())
                    })
                    .collect()
            })
            .unwrap_or_default();
        if let Some(step) = board_step(&key) {
            self.view = View::Board(state.moved(step, flows.len()));
            return vec![];
        }
        let enter = key == Key::Enter;
        match key {
            Key::Char('a') => {
                self.view = View::Board(BoardState::default());
                vec![]
            }
            Key::Enter | Key::Char('x') => {
                let Some((project, root, flow)) = flows.get(state.selected) else {
                    return vec![];
                };
                if self.here.as_ref() != Some(root) {
                    self.notice = Some(format!("{} runs in project {project}", flow.name));
                    return vec![];
                }
                if !enter {
                    return self.clear(Some(flow.clone()));
                }
                self.view = View::Panes;
                self.focus(PaneKey::Flow(flow.id.clone()))
            }
            other => self.on_any_board_key(other),
        }
    }

    /// Esc, q, o and n do the same on either board.
    fn on_any_board_key(&mut self, key: Key) -> Vec<Effect> {
        match key {
            Key::Esc | Key::Char('q') | Key::Char('o') => {
                self.view = View::Panes;
                vec![]
            }
            Key::Char('n') => {
                self.view = View::NewFlow(NewFlowForm::new(self.focused_repo()));
                vec![]
            }
            _ => vec![],
        }
    }

    /// Removes a finished flow of this project in the background; says so for any other.
    fn clear(&mut self, flow: Option<FlowView>) -> Vec<Effect> {
        match flow {
            Some(flow) if flow.is_finished() => {
                let job = Job::Remove(flow.name.clone());
                self.notice = Some(format!("clearing {}…", flow.name));
                if let View::Board(state) = &mut self.view {
                    state.selected = 0;
                }
                vec![Effect::Run(job)]
            }
            _ => {
                self.notice = Some("only a finished flow can be cleared".to_string());
                vec![]
            }
        }
    }

    fn on_click(&mut self, col: u16, row: u16) -> Vec<Effect> {
        let Some(side) = self.areas().side else {
            return vec![];
        };
        let inside = col >= side.x && col < side.x + side.width && row >= side.y;
        if !inside {
            return vec![];
        }
        let theme = Theme::builtin();
        let sidebar = self.sidebar(&theme, &[]);
        let key = if self.rail {
            Rail { sidebar }.key_at(row - side.y, side.height)
        } else {
            sidebar.key_at(row - side.y, side.height)
        };
        match key {
            Some(key) => self.focus(key),
            None => vec![],
        }
    }

    fn focus(&mut self, key: PaneKey) -> Vec<Effect> {
        if key == self.focus {
            return vec![];
        }
        self.focus = key.clone();
        self.cursor = None;
        if matches!(self.view, View::History(_)) {
            self.view = View::Panes;
        }
        self.screen = Screen::default();
        vec![Effect::Send(ClientMsg::Focus(key))]
    }

    /// What a background flow command printed: the flow it started, or why it failed.
    pub fn on_ran(&mut self, job: Job, result: Result<String, String>) {
        if job == Job::Overview {
            self.overview_running = false;
            match result.and_then(|json| {
                serde_json::from_str::<Projects>(&json).map_err(|error| error.to_string())
            }) {
                Ok(projects) => self.projects = Some(self.here_first(projects)),
                Err(error) => self.notice = Some(format!("cannot read every project: {error}")),
            }
            return;
        }
        self.notice = None;
        match (job, result) {
            (Job::Overview, _) => {}
            (Job::Start(_), Ok(_)) | (Job::Remove(_), Ok(_)) => {}
            (Job::Start(mut form), Err(error)) => {
                form.error = Some(error);
                self.view = View::NewFlow(form);
            }
            (Job::Remove(_), Err(error)) => self.notice = Some(error),
        }
    }

    fn here_first(&self, mut projects: Projects) -> Projects {
        if let Some(at) = projects
            .projects
            .iter()
            .position(|project| self.here.as_ref() == Some(&project.root))
        {
            let here = projects.projects.remove(at);
            projects.projects.insert(0, here);
        }
        projects
    }

    /// The repository of the pane in view: the master's directory, or the repo a worker's worktree is in.
    fn focused_repo(&self) -> PathBuf {
        let worktree = match &self.focus {
            PaneKey::Master => None,
            PaneKey::Flow(id) => self
                .workers()
                .into_iter()
                .find(|flow| &flow.id == id)
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
            master: self.master.as_deref(),
            now: self.now,
            spin: self.spin,
            theme,
        }
    }

    /// The rounded border around the focused pane: its name in the top edge, and for a worker
    /// its steps, branch and slot, and the way back to the master on the right.
    fn draw_stop(&self, frame: &mut TuiFrame, pane: Rect, theme: &Theme, unfinished: &[String]) {
        if unfinished.is_empty() {
            let text = vec![
                Line::styled("Stop flow? Every session in it ends.", theme.ui.text),
                Line::from(vec![
                    Span::styled("y", theme.ui.key),
                    Span::styled(" stops it, any other key keeps it", theme.ui.muted),
                ]),
            ];
            draw_dialog(frame, pane, theme, " stop ", theme.ui.rule, text);
            return;
        }
        let sidebar = self.sidebar(theme, &[]);
        let count = unfinished.len();
        let mut text = vec![Line::styled(
            format!(
                "{count} flow{} {} not finished:",
                if count == 1 { "" } else { "s" },
                if count == 1 { "is" } else { "are" }
            ),
            theme.ui.text,
        )];
        for flow in self
            .workers()
            .into_iter()
            .filter(|flow| unfinished.contains(&flow.id))
        {
            let doing = flow
                .blocker
                .clone()
                .or_else(|| flow.delegate.as_ref().map(|text| format!("agent: {text}")))
                .unwrap_or_else(|| sidebar.label(flow).content.to_string());
            text.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(sidebar.glyph(flow.state), theme.state(flow.state)),
                Span::styled(format!(" {}  ", flow.name), theme.ui.strong),
                Span::styled(crate::ui::sidebar::clip(&doing, 44), theme.ui.muted),
            ]));
        }
        let (them, they) = if count == 1 {
            ("it", "it resumes")
        } else {
            ("them", "they resume")
        };
        text.push(Line::raw(""));
        text.push(Line::from(vec![
            Span::styled("k", theme.ui.key),
            Span::styled(
                format!(" keep {them}; {they} the next time flow ui starts"),
                theme.ui.muted,
            ),
        ]));
        text.push(Line::from(vec![
            Span::styled("e", theme.ui.key),
            Span::styled(
                format!(" end {them}; branches and worktrees stay until flow clear"),
                theme.ui.muted,
            ),
        ]));
        text.push(Line::from(vec![
            Span::styled("esc", theme.ui.key),
            Span::styled(" keep flow running", theme.ui.muted),
        ]));
        draw_dialog(frame, pane, theme, " stop ", theme.state.waiting, text);
    }

    fn border(&self, theme: &Theme) -> Block<'static> {
        let mut block = Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(theme.ui.rule);
        if let Some(notice) = &self.notice {
            block = block.title_bottom(Line::styled(format!(" {notice} "), theme.state.waiting));
        }
        let PaneKey::Flow(id) = &self.focus else {
            let mut title = vec![
                Span::raw(" "),
                Span::styled(theme.glyphs.master, theme.ui.accent),
                Span::styled(" master ", theme.ui.strong),
            ];
            if let Some(place) = &self.master {
                title.push(Span::styled(format!(" {place} "), theme.ui.faint));
            }
            let block = block.title(Line::from(title));
            return match self.border_hint(theme) {
                Some(hint) => block.title(hint.right_aligned()),
                None => block,
            };
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
        let block = block.title(Line::from(title));
        match self.border_hint(theme) {
            Some(hint) => block.title(hint.right_aligned()),
            None => block,
        }
    }

    /// The right of the pane's top edge: with the rail, the next flow that needs you and the way back
    /// to the sidebar; with the sidebar, the way back to the master.
    fn border_hint(&self, theme: &Theme) -> Option<Line<'static>> {
        if !self.rail {
            return match self.focus {
                PaneKey::Master => None,
                PaneKey::Flow(_) => Some(Line::from(vec![
                    Span::styled(" C-space 0", theme.ui.key),
                    Span::styled(" back to master ", theme.ui.faint),
                ])),
            };
        }
        if self.workers().is_empty() {
            return None;
        }
        let sidebar = self.sidebar(theme, &[]);
        let mut spans = Vec::new();
        let next = self
            .workers()
            .into_iter()
            .enumerate()
            .find(|(_, flow)| flow.needs_you() && self.focus != PaneKey::Flow(flow.id.clone()));
        if let Some((index, flow)) = next {
            let what = match flow.state {
                State::Waiting => "is waiting",
                State::NotResponding => "is not responding",
                _ => "is blocked",
            };
            spans.push(Span::styled(
                format!(
                    " {} {} {} {what}",
                    sidebar.glyph(flow.state),
                    index + 1,
                    flow.name
                ),
                theme.state(flow.state),
            ));
            spans.push(Span::styled(
                format!("  {}  ", theme.glyphs.separator),
                theme.ui.rule,
            ));
        } else {
            spans.push(Span::raw(" "));
        }
        spans.push(Span::styled("C-space b", theme.ui.key));
        spans.push(Span::styled(" sidebar ", theme.ui.faint));
        Some(Line::from(spans))
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
        if let Some(place) = &self.master {
            left.push(Span::styled(format!("  {place}"), theme.ui.faint));
        }
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
        let keys = vec![
            Span::styled("C-space", theme.ui.key),
            Span::styled(" then  ", theme.ui.faint),
            Span::styled("n", theme.ui.key),
            Span::styled(" new flow   ", theme.ui.muted),
            Span::styled("?", theme.ui.key),
            Span::styled(" keys ", theme.ui.muted),
        ];
        let mut full = vec![
            Span::styled("no workers yet", theme.ui.faint),
            Span::styled(format!("   {}  ", glyphs.separator), theme.ui.rule),
        ];
        full.extend(keys.clone());
        let left = Line::from(left);
        let room = (area.width as usize).saturating_sub(left.width() + 1);
        let right = [Line::from(full), Line::from(keys)]
            .into_iter()
            .find(|line| line.width() <= room);
        frame.render_widget(Block::default().style(theme.ui.bar), area);
        frame.render_widget(Paragraph::new(left), area);
        if let Some(right) = right {
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
    draw_dialog(frame, pane, theme, title, style, text);
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
    draw_dialog(frame, pane, theme, " not open ", theme.ui.rule, text);
}

fn draw_dialog(
    frame: &mut TuiFrame,
    pane: Rect,
    theme: &Theme,
    title: &str,
    border: Style,
    text: Vec<Line>,
) {
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
                .style(theme.ui.background)
                .padding(Padding::horizontal(1))
                .title(title),
        ),
        area,
    );
}

/// Every key behind C-space, over the pane.
fn draw_help(frame: &mut TuiFrame, pane: Rect, theme: &Theme) {
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
        assert!(bar.contains("C-space then  n new flow   ? keys"), "{bar}");
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
    fn paints_every_cell_on_the_themes_ground() {
        let app = board_app();
        let theme = Theme::from_palette(crate::ui::theme::Palette::named("flow").unwrap());
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
            " C-space j k peek  0-9 jump  n new"
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
            " C-space j k peek  0-9 jump  n new"
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

        let bar = row(&render(&solo), 9);
        let sidebar = render_at(&fleet, FLEET);

        assert!(bar.contains("◎ master  ~/projects/shop · main"), "{bar}");
        let mut wide = App::new(FLEET);
        wide.on_server(ServerMsg::Master {
            cwd: "/shop".into(),
            branch: None,
        });
        let wide = row(&render_at(&wide, FLEET), 43);
        assert!(
            wide.ends_with("no workers yet   │  C-space then  n new flow   ? keys "),
            "{wide}"
        );
        assert!(row(&sidebar, 0).starts_with("╭ ◎ master  ~/projects/shop · main ─"));
        assert_eq!(side(&sidebar, 3).trim_end(), "     ~/projects/shop · main");
        assert_eq!(side(&sidebar, 5).trim_end(), " NEEDS YOU");
    }

    fn board_app() -> App {
        let mut app = with_fleet().with_home(Some("/Users/jan".into()));
        app.offset_secs = 0;
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
        assert!(row(&terminal, 0).starts_with(" ≋ flow board "));
        assert!(row(&terminal, 0).ends_with("5 sessions · 2 need you · 1 delegated · 1 done "));
        assert!(row(&terminal, 3).contains("│ 0 ◎ master                              you  │"));
        assert!(row(&terminal, 3).contains("│ 1 ✖ shop-221                        blocked  │"));
        assert!(row(&terminal, 3).contains("│ 2 ◆ shop-230                waiting for you  │"));
        assert!(row(&terminal, 4).contains("│   ~/projects/shop · main"));
        assert!(
            row(&terminal, 5).contains("│   ━━━━━━━━━━━━───────────────  4/9   29m ago │"),
            "{}",
            row(&terminal, 5)
        );
        assert!(row(&terminal, 10).contains("│ 3 ⠙ export-csv            reviewing · agent  │"));
        assert!(
            row(&terminal, 16)
                .starts_with(" 2 shop-230  shop-230 · guided · flow/shop-230 · slot 1"),
            "{}",
            row(&terminal, 16)
        );
        assert!(
            row(&terminal, 43)
                .starts_with(" 0-9 select   h j k l move   enter jump in   n new flow   x clear finished flow   a every project   esc back to shop-230")
        );
        let buffer = terminal.backend().buffer();
        let accent = Theme::builtin().ui.accent.fg.unwrap();
        assert_eq!(buffer[(101, 2)].fg, accent, "the selected card is outlined");
        assert_eq!(
            buffer[(52, 2)].fg,
            Theme::builtin().state.blocked.fg.unwrap()
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
                "auto"
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
        assert_eq!(effects, vec![Effect::Run(Job::Remove("shop-198".into()))]);
        app.on_ran(
            Job::Remove("shop-198".into()),
            Err("shop-198 has uncommitted work in /shop".into()),
        );
        assert_eq!(
            app.notice.as_deref(),
            Some("shop-198 has uncommitted work in /shop")
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
        let api = (0..44).find(|y| row(&terminal, *y).starts_with(" api  1 flow"));
        assert!(api.is_some(), "this project comes first, then api");
        assert!(
            row(&terminal, 43).starts_with(" h j k l move   enter jump in   n new flow   x clear finished flow   a this project"),
            "{}",
            row(&terminal, 43)
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
                rows: 42
            }))]
        );
        let terminal = render_at(&app, FLEET);
        let rail = |y| row(&terminal, y).chars().skip(144).collect::<String>();

        assert!(
            row(&terminal, 0).ends_with(" ◆ 2 shop-230 is waiting  │  C-space b sidebar ╮  ≋    ")
        );
        assert_eq!(rail(2), " 0 ◎  ");
        assert_eq!(rail(4), "▌1 ✖  ");
        assert_eq!(rail(5), " 2 ◆  ");
        assert_eq!(rail(7), " 3 ⠙  ");
        assert_eq!(rail(9), " 4 ✔  ");
        assert_eq!(rail(43), " C-␣  ");
        assert_eq!(
            app.on_input(Routed::Click { col: 146, row: 5 }),
            vec![Effect::Send(ClientMsg::Focus(PaneKey::Flow(
                "shop-230".into()
            )))]
        );
        assert_eq!(
            app.on_input(Routed::Command(Command::Rail)),
            vec![Effect::Send(ClientMsg::Resize(Size {
                cols: 107,
                rows: 42
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
            vec![Effect::Send(ClientMsg::History(Some(117)))]
        );

        let terminal = render(&app);
        assert!(
            row(&terminal, 7).contains("↑ 117 lines back   k j scroll  g G ends  q live"),
            "{}",
            row(&terminal, 7)
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
        let dialog: String = (0..44).map(|y| row(&terminal, y) + "\n").collect();
        assert!(dialog.contains("3 flows are not finished:"), "{dialog}");
        assert!(
            dialog.contains("✖ shop-221  migration needs a default value"),
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
        let help: String = (0..44).map(|y| row(&terminal, y) + "\n").collect();
        assert!(help.contains("C-space then"), "{help}");
        assert!(
            help.contains("  b        fold the sidebar to a rail and back"),
            "{help}"
        );
        assert_eq!(
            side(&terminal, 43).trim_end(),
            "         o board  b rail  ? all keys"
        );

        app.on_input(Routed::View(vec![Key::Char('x')]));
        assert!(!app.is_modal());
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
}
