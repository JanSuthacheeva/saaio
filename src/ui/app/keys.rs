use std::fmt::Write as _;
use std::mem;

use ratatui::layout::Position;

use super::{App, Effect, Job, Removal, View};
use crate::proto::{ClientMsg, PaneKey};
use crate::text::plural;
use crate::ui::board::BoardState;
use crate::ui::input::{Command, Key, Routed, Step};
use crate::ui::newflow::{FormAction, NewFlowForm};
use crate::ui::rail::Rail;
use crate::ui::screen::Screen;
use crate::ui::theme::Theme;

impl App {
    pub fn on_input(&mut self, routed: Routed) -> Vec<Effect> {
        if !matches!(routed, Routed::Pane(_)) {
            self.notice = None;
        }
        match routed {
            Routed::Pane(bytes) if self.view == View::Panes => match self.ended_worker() {
                Some(name) => self.on_ended_key(name, &bytes),
                None => vec![Effect::Send(ClientMsg::Input(bytes))],
            },
            Routed::Click { col, row } if self.view == View::Panes => self.on_click(col, row),
            Routed::Pane(_) | Routed::Click { .. } => vec![],
            Routed::View(keys) => self.on_keys(keys),
            Routed::Command(command) => self.on_command(command),
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
            Command::NewFlow => self.open_new_flow(),
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
        match mem::replace(&mut self.view, View::Panes) {
            View::Board(state) => {
                self.view = View::Board(state);
                self.on_board_key(state, key)
            }
            View::History(back) => {
                self.view = View::History(back);
                self.on_history_key(back, key)
            }
            View::NewFlow(mut form) => match form.on_key(key) {
                FormAction::Edit => {
                    self.view = View::NewFlow(form);
                    vec![]
                }
                FormAction::Cancel => vec![],
                FormAction::Submit => {
                    self.notice = Some("starting the flow…".to_string());
                    vec![Effect::Run(Job::Start(form))]
                }
            },
            View::Stop(unfinished) => self.on_stop_key(unfinished, key),
            View::Confirm { then, .. } if key == Key::Char('y') => self.remove(then),
            View::Confirm { .. } | View::Help | View::Panes => vec![],
        }
    }

    /// With every worker finished: y stops, any other key keeps flow running. With some unfinished:
    /// k keeps them for the next flow ui, e ends them, esc goes back.
    fn on_stop_key(&mut self, unfinished: Vec<String>, key: Key) -> Vec<Effect> {
        let stop =
            |message: String| vec![Effect::Send(ClientMsg::StopServer), Effect::Quit(message)];
        if unfinished.is_empty() {
            if key == Key::Char('y') {
                return stop("stopped flow and every session in it".to_string());
            }
            return vec![];
        }
        match key {
            Key::Char('k') => stop(format!(
                "stopped flow; {} unfinished flow{} resume the next time you run `flow ui`",
                unfinished.len(),
                plural(unfinished.len())
            )),
            Key::Char('e') => {
                let mut message = format!(
                    "stopped flow and ended {} flow{}; their code stays until `flow clear`:",
                    unfinished.len(),
                    plural(unfinished.len())
                );
                for flow in self
                    .workers()
                    .into_iter()
                    .filter(|flow| unfinished.contains(&flow.id))
                {
                    if let Some(space) = &flow.workspace {
                        let _ = write!(message, "\n  {}  ", flow.name);
                        if let Some(branch) = &space.branch {
                            let _ = write!(message, "{branch}  ");
                        }
                        message.push_str(&self.tilde(&space.path));
                    }
                }
                let mut effects = vec![Effect::Send(ClientMsg::EndFlows)];
                effects.extend(stop(message));
                effects
            }
            Key::Esc => vec![],
            _ => {
                self.view = View::Stop(unfinished);
                vec![]
            }
        }
    }

    /// k j a line, C-u C-d half a page, C-b C-f a page, g G the ends, q or esc back to live.
    fn on_history_key(&mut self, back: u32, key: Key) -> Vec<Effect> {
        let page = u32::from(self.areas().pane.height.max(2));
        let next = match key {
            Key::Char('k') | Key::Up => back.saturating_add(1),
            Key::Char('j') | Key::Down => back.saturating_sub(1),
            Key::Ctrl('u') => back.saturating_add(page / 2),
            Key::Ctrl('d') => back.saturating_sub(page / 2),
            Key::Ctrl('b') => back.saturating_add(page),
            Key::Ctrl('f') => back.saturating_sub(page),
            Key::Char('g') => u32::MAX,
            Key::Char('G') => 0,
            Key::Char('q') | Key::Esc => {
                self.view = View::Panes;
                return vec![Effect::Send(ClientMsg::History(None))];
            }
            _ => return vec![],
        };
        self.view = View::History(next);
        vec![Effect::Send(ClientMsg::History(Some(next)))]
    }

    /// Over a worker whose session ended: x cleans the flow up, r resumes it, esc goes to the master.
    fn on_ended_key(&mut self, name: String, bytes: &[u8]) -> Vec<Effect> {
        match bytes {
            b"x" | b"X" => self.remove(Removal::of(name)),
            b"r" | b"R" => vec![Effect::Send(ClientMsg::Reopen(self.focus.clone()))],
            [0x1b] => self.focus(PaneKey::Master),
            _ => vec![],
        }
    }

    fn on_click(&mut self, col: u16, row: u16) -> Vec<Effect> {
        let Some(side) = self.areas().side else {
            return vec![];
        };
        if !side.contains(Position::new(col, row)) {
            return vec![];
        }
        let theme = Theme::default();
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

    pub(super) fn focus(&mut self, key: PaneKey) -> Vec<Effect> {
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

    /// Moves the peek cursor over the workers not in view; past either end it goes away.
    fn move_cursor(&mut self, step: Step) {
        let ids: Vec<String> = self
            .workers()
            .into_iter()
            .filter(|flow| !self.in_view(&flow.id))
            .map(|flow| flow.id.clone())
            .collect();
        let at = self
            .cursor
            .as_ref()
            .and_then(|id| ids.iter().position(|other| other == id));
        let next = match (at, step) {
            (None, Step::Next) => Some(0),
            (None, Step::Prev) => ids.len().checked_sub(1),
            (Some(at), Step::Next) => Some(at + 1),
            (Some(at), Step::Prev) => at.checked_sub(1),
        };
        self.cursor = next.and_then(|index| ids.get(index)).cloned();
    }

    /// The new-flow dialog for the repo in view, asking for its branch convention.
    pub(super) fn open_new_flow(&mut self) -> Vec<Effect> {
        let repo = self.focused_repo();
        let place = self.tilde(&repo.display().to_string());
        self.view = View::NewFlow(NewFlowForm::new(repo.clone(), place));
        vec![Effect::LoadBranching(repo)]
    }

    /// Runs `flow remove` in the background; what it refuses comes back as a question (on_ran).
    pub(super) fn remove(&mut self, removal: Removal) -> Vec<Effect> {
        self.notice = Some(format!("cleaning up {}…", removal.name));
        vec![Effect::Run(Job::Remove(removal))]
    }
}
