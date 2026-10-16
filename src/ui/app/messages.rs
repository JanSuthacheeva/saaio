use std::path::Path;

use super::{
    App, CHANGE_SHOWN_MS, Effect, Job, JobError, OVERVIEW_EVERY_MS, Removal, TICKS_PER_AGE, View,
};
use crate::branching::Branching;
use crate::cli::output::remove::Refusal;
use crate::proto::{ClientMsg, PaneKey, ServerMsg, Size};
use crate::ui::board::BoardState;
use crate::view::{Projects, State};

impl App {
    pub fn on_server(&mut self, message: ServerMsg) -> Vec<Effect> {
        match message {
            ServerMsg::Welcome { .. } | ServerMsg::Closed(_) | ServerMsg::NotOpen(_) => vec![],
            ServerMsg::Master { cwd, branch, .. } => {
                let place = self.tilde(&cwd);
                self.master_cwd = Some(cwd);
                self.master = Some(match branch {
                    Some(branch) => format!("{place} · {branch}"),
                    None => place,
                });
                vec![]
            }
            ServerMsg::Board(board) => {
                self.now = self.now.max(board.now);
                if let Some(old) = &self.board {
                    for flow in &board.flows {
                        let was = old.flows.iter().find(|other| other.id == flow.id);
                        if was.is_some_and(|was| was.state != flow.state) {
                            self.changed.insert(flow.id.clone(), self.now);
                        }
                    }
                }
                self.board = Some(board);
                let mut effects = Vec::new();
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
            ServerMsg::Detached { by } => vec![Effect::Quit(format!(
                "another flow ui took over: pid {} in {} (flow {})",
                by.pid,
                self.tilde(&by.folder()),
                by.build
            ))],
        }
    }

    /// What a background flow command printed: the flow it started, or why it failed.
    pub fn on_ran(&mut self, job: Job, result: Result<String, JobError>) {
        if job != Job::Overview {
            self.notice = None;
        }
        match (job, result) {
            (Job::Overview, result) => {
                self.overview_running = false;
                match result.map_err(|error| error.message).and_then(|json| {
                    serde_json::from_str::<Projects>(&json).map_err(|error| error.to_string())
                }) {
                    Ok(projects) => self.projects = Some(self.here_first(projects)),
                    Err(error) => self.notice = Some(format!("cannot read every project: {error}")),
                }
            }
            (Job::Start(_) | Job::Remove(_), Ok(_)) => {}
            (Job::Start(mut form), Err(error)) => {
                form.error = Some(error.message);
                self.view = View::NewFlow(form);
            }
            (Job::Remove(removal), Err(error)) if error.refusal == Some(Refusal::Uncommitted) => {
                self.view = View::Confirm {
                    question: format!("{} has uncommitted changes. Discard them?", removal.name),
                    then: Removal {
                        force: true,
                        ..removal
                    },
                };
            }
            (Job::Remove(removal), Err(error)) if error.refusal == Some(Refusal::Unmerged) => {
                self.view = View::Confirm {
                    question: format!("{}. Delete it anyway?", error.message),
                    then: Removal {
                        delete_unmerged: true,
                        ..removal
                    },
                };
            }
            (Job::Remove(_), Err(error)) => self.notice = Some(error.message),
        }
    }

    /// The repo's branch convention for the open dialog; a broken file shows as its error.
    pub fn on_branching(&mut self, repo: &Path, branching: Result<Option<Branching>, String>) {
        let View::NewFlow(form) = &mut self.view else {
            return;
        };
        if form.repo != repo {
            return;
        }
        match branching {
            Ok(branching) => form.set_branching(branching),
            Err(error) => form.error = Some(error),
        }
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
        let shown = self.changed.len();
        self.changed
            .retain(|_, at| now.saturating_sub(*at) < CHANGE_SHOWN_MS);
        let fading = !self.changed.is_empty() || self.changed.len() != shown;
        spinning || fading || self.ticks.is_multiple_of(TICKS_PER_AGE)
    }

    /// Job::Overview when every project is in view and the last answer is older than 2 s.
    pub fn overview_due(&mut self) -> Vec<Effect> {
        let all = matches!(self.view, View::Board(BoardState { all: true, .. }));
        if !all || self.overview_running || self.now < self.overview_at + OVERVIEW_EVERY_MS {
            return vec![];
        }
        self.ask_overview()
    }

    pub fn on_resize(&mut self, size: Size) -> Vec<Effect> {
        self.size = size;
        vec![Effect::Send(ClientMsg::Resize(self.areas().pane_size()))]
    }

    pub(super) fn ask_overview(&mut self) -> Vec<Effect> {
        self.overview_at = self.now;
        self.overview_running = true;
        vec![Effect::Run(Job::Overview)]
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
}
