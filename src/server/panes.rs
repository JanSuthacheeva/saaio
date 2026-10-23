//! Which panes are open: refresh, reconcile, open, resume and close

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};
use std::time::Instant;

use anyhow::{Context, Result};

use super::host::HostEvent;
use super::master::{Instructions, Start};
use super::reconcile::{self, Wanted};
use super::{ConnId, Event, Server, spawn_named};
use crate::event::{Event as LogEvent, EventKind};
use crate::fold;
use crate::identity;
use crate::launch::pane::{self, HOSTED, PROOF_ENV, PaneSpec};
use crate::launch::worktree;
use crate::proto::{PaneKey, PaneStatus, ServerMsg};
use crate::server_log;
use crate::store::{Busy, Waiter};
use crate::view::{FlowView, Workspace};
use crate::workspace::git;

impl Server {
    /// Whether the person trusts the repository of a flow's workspace, so Codex may trust it too.
    fn trusts(&self, space: &Workspace) -> bool {
        self.trust
            .as_ref()
            .is_some_and(|trust| trust.covers(&trust_root(space)))
    }

    /// Re-folds when the store changed, reconciles, sends Board and Panes to the attached UI.
    pub(super) fn refresh(&mut self) {
        self.refreshed_at = Instant::now();
        self.reload_config();
        self.send_master();
        let now = (self.clock)();
        match self.watch.changed(&self.store, now) {
            Ok(true) => {}
            Ok(false) => return,
            Err(error) => {
                server_log::write(
                    (self.clock)(),
                    &format!("could not look at the flows: {error:#}"),
                );
                return;
            }
        }
        let (logs, warnings) = self.store.logs();
        for warning in warnings {
            server_log::write(now, &warning);
        }
        self.board = fold::board(fold::views(&logs, now), now);
        let open: Vec<PaneKey> = self.host.keys().cloned().collect();
        let wanted = reconcile::reconcile(
            &self.board.flows,
            &open,
            &self.known,
            &self.config,
            &self.env,
            &|space| self.trusts(space),
        );
        let panes_change = wanted != Wanted::default();
        self.apply(wanted);
        self.send_board();
        if panes_change {
            self.send_panes();
        }
    }

    /// Takes config.toml again when it changed; a broken one keeps the last and is logged.
    fn reload_config(&mut self) {
        match self.configs.changed() {
            None => {}
            Some(Ok(config)) => {
                self.config = config;
                server_log::write((self.clock)(), "config.toml read again");
            }
            Some(Err(error)) => server_log::write((self.clock)(), &format!("config: {error:#}")),
        }
    }

    /// Opens and closes what reconcile asked for; marks new workers HOSTED in their log; Host::fail for failed.
    fn apply(&mut self, wanted: Wanted) {
        for key in &wanted.close {
            self.host.close(key);
        }
        for (key, reason) in wanted.failed {
            if let PaneKey::Flow(id) = &key {
                self.known.insert(id.clone());
            }
            self.host.fail(key, reason);
        }
        for spec in wanted.open {
            self.open_worker(spec);
        }
    }

    pub(super) fn open_master(&mut self, start: Start) {
        let Some((name, agent)) = self.config.master_agent() else {
            let missing = self
                .config
                .master
                .agent
                .as_ref()
                .unwrap_or(&self.config.worker.agent);
            let reason = format!("no agent named {missing}");
            self.host.fail(PaneKey::Master, reason);
            return;
        };
        if start == Start::Fresh
            && let Err(error) = self.store.forget_master_session()
        {
            server_log::write((self.clock)(), &format!("{error:#}"));
        }
        let text = fs::read_to_string(&self.instructions).unwrap_or_else(|error| {
            let file = self.instructions.display();
            server_log::write((self.clock)(), &format!("could not read {file}: {error}"));
            String::new()
        });
        let instructions = Instructions {
            path: self.instructions.clone(),
            text,
        };
        match self.record.spec(
            start,
            agent,
            &self.env,
            &instructions,
            self.store.master_session().as_deref(),
        ) {
            Some(spec) => match self.open_pane(spec) {
                Ok(()) if start == Start::Resume => {
                    self.pane_mut(&PaneKey::Master).resumed = Some(Instant::now());
                }
                Ok(()) => {}
                Err(error) => server_log::write(
                    (self.clock)(),
                    &format!("could not start the master: {error:#}"),
                ),
            },
            None => self.host.fail(
                PaneKey::Master,
                format!("agent {name} cannot resume a session"),
            ),
        }
    }

    /// Opens a pane, forgetting the busy mark (and the master's turn) an earlier process of it may
    /// have left: one killed mid-turn never reached its Stop, and a new process runs no turn yet;
    /// keeps when it opened.
    fn open_pane(&mut self, mut spec: PaneSpec) -> anyhow::Result<()> {
        let waiter = waiter_of(&spec.key);
        self.hand_key(&mut spec)?;
        if let Err(error) = self.store.set_busy(&waiter, Busy::Idle) {
            server_log::write((self.clock)(), &format!("{error:#}"));
        }
        if spec.key == PaneKey::Master
            && let Err(error) = self.store.forget_master_turn()
        {
            server_log::write((self.clock)(), &format!("{error:#}"));
        }
        let key = spec.key.clone();
        let adapter = spec.adapter;
        self.host.open(spec)?;
        let pane = self.pane_mut(&key);
        pane.adapter = adapter;
        pane.opened_at = Some(Instant::now());
        pane.line_at = None;
        Ok(())
    }

    /// A fresh key for the pane `spec` opens: its digest in the store, the key in spec.env;
    /// Err when the pane already runs (its key stays) or the digest cannot be written.
    fn hand_key(&mut self, spec: &mut PaneSpec) -> anyhow::Result<()> {
        if self.host.is_running(&spec.key) {
            anyhow::bail!("{} is already running", spec.key);
        }
        let key = identity::new_key().context("could not make a key for the pane")?;
        self.store
            .set_key(&waiter_of(&spec.key), &identity::digest(&key))?;
        spec.env.push((PROOF_ENV.to_string(), key));
        Ok(())
    }

    /// Marks the worker HOSTED in its log before its agent starts, so a server that dies right
    /// after the start leaves a worker the next one resumes, not one it starts again fresh; takes
    /// the mark back when the agent could not start, since there is no session to resume.
    fn open_worker(&mut self, spec: PaneSpec) {
        let key = spec.key.clone();
        let PaneKey::Flow(id) = &key else {
            return;
        };
        self.known.insert(id.clone());
        let Some(space) = self.flow(id).and_then(|flow| flow.workspace.clone()) else {
            return;
        };
        let resumes = space.window.is_some();
        let recorded = space.window.as_deref() != Some(HOSTED);
        if recorded {
            let hosted = EventKind::from(Workspace {
                window: Some(HOSTED.to_string()),
                ..space.clone()
            });
            if let Err(error) = self
                .store
                .append(id, &LogEvent::new((self.clock)(), hosted))
            {
                self.host.fail(
                    key.clone(),
                    format!("could not record its start: {error:#}"),
                );
                return;
            }
        }
        match self.open_pane(spec) {
            Ok(()) if resumes => {
                self.pane_mut(&key).resumed = Some(Instant::now());
            }
            Ok(()) => {}
            Err(error) => {
                let blocked = EventKind::Blocked {
                    reason: format!("could not start the agent: {error:#}"),
                };
                let unhosted = recorded.then(|| EventKind::from(space));
                for event in unhosted.into_iter().chain([blocked]) {
                    if let Err(error) = self.store.append(id, &LogEvent::new((self.clock)(), event))
                    {
                        server_log::write((self.clock)(), &format!("{error:#}"));
                    }
                }
            }
        }
    }

    /// Resumes an unfinished worker on the board whose pane exited or never opened.
    pub(super) fn reopen_worker(&mut self, id: &str) {
        let key = PaneKey::Flow(id.to_string());
        let on_board = self.board.flows.iter().any(|flow| flow.id == id);
        if !on_board || self.host.status(&key) == Some(&PaneStatus::Running) {
            return;
        }
        let flow = match self.current_flow(id) {
            Ok(flow) => flow,
            Err(error) => {
                self.host
                    .fail(key, format!("could not check flow: {error:#}"));
                return;
            }
        };
        if flow.is_finished() {
            self.host.fail(
                key,
                "finished flow cannot reopen; start a new flow".to_string(),
            );
            return;
        }
        let Some(space) = &flow.workspace else {
            return;
        };
        let Some((name, agent)) = self.config.agent(Some(&space.agent)) else {
            self.host
                .fail(key, format!("no agent named {}", space.agent));
            return;
        };
        match pane::worker_spec(&flow, agent, &self.env, self.trusts(space)) {
            Some(spec) => self.open_worker(spec),
            None => self.host.fail(key, reconcile::unopenable(name)),
        }
    }

    /// Opens a pane once more with the agent's first command: its resume failed before the agent
    /// ever saved the session.
    pub(super) fn open_fresh(&mut self, key: &PaneKey) {
        let PaneKey::Flow(id) = key else {
            server_log::write(
                (self.clock)(),
                &format!("resuming {key} failed, starting it fresh"),
            );
            self.open_master(Start::Fresh);
            return;
        };
        let mut flow = match self.current_flow(id) {
            Ok(flow) => flow,
            Err(error) => {
                self.host
                    .fail(key.clone(), format!("could not check flow: {error:#}"));
                return;
            }
        };
        if flow.is_finished() {
            return;
        }
        server_log::write(
            (self.clock)(),
            &format!("resuming {key} failed, starting it fresh"),
        );
        let Some(space) = flow.workspace.as_mut() else {
            return;
        };
        space.window = None;
        let trusted = self.trusts(space);
        let spec = self
            .config
            .agent(Some(&space.agent))
            .and_then(|(_, agent)| pane::worker_spec(&flow, agent, &self.env, trusted));
        if let Some(spec) = spec
            && let Err(error) = self.open_pane(spec)
        {
            server_log::write((self.clock)(), &format!("{error:#}"));
        }
    }

    /// Hangs up a flow's pane for `saaio remove` or `saaio clear`; the connection hears Closed once
    /// it exited, NotOpen when there is no pane.
    pub(super) fn close_flow(&mut self, conn: ConnId, id: String) {
        let key = PaneKey::Flow(id.clone());
        self.known.insert(id.clone());
        let Some(running) = self
            .host
            .status(&key)
            .map(|status| *status == PaneStatus::Running)
        else {
            self.send_to(conn, &ServerMsg::NotOpen(id));
            return;
        };
        self.pane_mut(&key).closing.push(conn);
        self.host.close(&key);
        if !running {
            self.tell_closed(&key);
        }
    }

    /// Tells everyone waiting for this pane that it is gone, once it is.
    pub(super) fn tell_closed(&mut self, key: &PaneKey) {
        if self.host.status(key).is_some() {
            return;
        }
        let waiting = self
            .panes
            .get_mut(key)
            .map(|pane| std::mem::take(&mut pane.closing))
            .unwrap_or_default();
        if waiting.is_empty() {
            return;
        }
        if let PaneKey::Flow(id) = key {
            for conn in waiting {
                self.send_to(conn, &ServerMsg::Closed(id.clone()));
            }
        }
        self.send_panes();
    }

    /// An Ended event for every unfinished flow with a worker, so the next start opens none of them.
    pub(super) fn end_flows(&mut self) {
        let now = (self.clock)();
        let unfinished: Vec<String> = self
            .board
            .flows
            .iter()
            .filter(|flow| flow.workspace.is_some() && !flow.is_finished())
            .map(|flow| flow.id.clone())
            .collect();
        for id in unfinished {
            if let Err(error) = self
                .store
                .append(&id, &LogEvent::new(now, EventKind::Ended))
            {
                server_log::write((self.clock)(), &format!("{error:#}"));
            }
        }
    }

    fn flow(&self, id: &str) -> Option<&FlowView> {
        self.board.flows.iter().find(|flow| flow.id == id)
    }

    /// Re-folds the current log before a launch, since the board refresh may not have run yet.
    fn current_flow(&self, id: &str) -> Result<FlowView> {
        let events = self.store.events(id)?;
        fold::fold(id, &events, None, (self.clock)()).context("flow has no start event")
    }
}

/// Whose wait a pane's session is.
fn waiter_of(key: &PaneKey) -> Waiter {
    match key {
        PaneKey::Master => Waiter::Master,
        PaneKey::Flow(id) => Waiter::Flow(id.clone()),
    }
}

pub(super) fn forward_host_events(events: Sender<Event>) -> Result<Sender<HostEvent>> {
    let (host_events, inbox) = mpsc::channel();
    spawn_named("host events".to_string(), move || {
        for event in inbox {
            if events.send(Event::Host(event)).is_err() {
                break;
            }
        }
    })
    .context("could not start the thread that forwards the panes' output")?;
    Ok(host_events)
}

/// The folder trust is kept for: a worktree's repository, else the repository a plain session's
/// folder is in, else that folder, as `saaio trust` records it.
fn trust_root(space: &Workspace) -> PathBuf {
    worktree::repo_of(space).unwrap_or_else(|| git::root_or_folder(Path::new(&space.path)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::launch::worktree::{folder_workspace, workspace_for};
    use crate::workspace::fixtures::repo;

    #[test]
    fn keeps_trust_for_the_repository_a_session_works_in() {
        let (_dir, repo) = repo();
        let sub = repo.join("docs");
        std::fs::create_dir(&sub).unwrap();
        let elsewhere = tempfile::TempDir::new().unwrap();
        let worktree = workspace_for(&repo, "shop-217", "flow/shop-217".into(), 1, "claude");

        assert_eq!(trust_root(&worktree), repo);
        assert_eq!(trust_root(&folder_workspace(&sub, "codex")), repo);
        let plain = folder_workspace(elsewhere.path(), "codex");
        assert_eq!(trust_root(&plain), elsewhere.path());
    }
}
