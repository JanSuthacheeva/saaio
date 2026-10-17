use std::fs;
use std::sync::mpsc::{self, Sender};
use std::time::Instant;

use super::host::HostEvent;
use super::master::{Instructions, Start};
use super::reconcile::{self, Wanted};
use super::{ConnId, Event, Server, spawn_named};
use crate::event::{Event as LogEvent, EventKind};
use crate::fold;
use crate::launch::pane::{self, HOSTED, PaneSpec};
use crate::proto::{PaneKey, PaneStatus, ServerMsg};
use crate::server_log;
use crate::store::{Busy, Waiter};
use crate::view::{FlowView, Workspace};

impl Server {
    /// Re-folds when the store changed, reconciles, sends Board and Panes to the attached UI.
    pub(super) fn refresh(&mut self) {
        self.refreshed_at = Instant::now();
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
        );
        let panes_change = wanted != Wanted::default();
        self.apply(wanted);
        self.send_board();
        if panes_change {
            self.send_panes();
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
            self.config.master.wake,
            self.store.master_session().as_deref(),
        ) {
            Some(spec) => match self.open_pane(spec) {
                Ok(()) if start == Start::Resume => {
                    self.resumed.insert(PaneKey::Master, Instant::now());
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
    /// have left: one killed mid-turn never reached its Stop, and a new process runs no turn yet.
    fn open_pane(&mut self, spec: PaneSpec) -> anyhow::Result<()> {
        let waiter = match &spec.key {
            PaneKey::Master => Waiter::Master,
            PaneKey::Flow(id) => Waiter::Flow(id.clone()),
        };
        if let Err(error) = self.store.set_busy(&waiter, Busy::Idle) {
            server_log::write((self.clock)(), &format!("{error:#}"));
        }
        if spec.key == PaneKey::Master
            && let Err(error) = self.store.forget_master_turn()
        {
            server_log::write((self.clock)(), &format!("{error:#}"));
        }
        self.host.open(spec)
    }

    fn open_worker(&mut self, spec: PaneSpec) {
        let key = spec.key.clone();
        let PaneKey::Flow(id) = &key else {
            return;
        };
        self.known.insert(id.clone());
        let Some(space) = self.flow(id).and_then(|flow| flow.workspace.clone()) else {
            return;
        };
        let now = (self.clock)();
        let event = match self.open_pane(spec) {
            Ok(()) => {
                if space.window.is_some() {
                    self.resumed.insert(key.clone(), Instant::now());
                }
                if space.window.as_deref() == Some(HOSTED) {
                    return;
                }
                EventKind::from(Workspace {
                    window: Some(HOSTED.to_string()),
                    ..space
                })
            }
            Err(error) => EventKind::Blocked {
                reason: format!("could not start the agent: {error:#}"),
            },
        };
        if let Err(error) = self.store.append(id, &LogEvent::new(now, event)) {
            server_log::write((self.clock)(), &format!("{error:#}"));
        }
    }

    /// Resumes a worker whose pane exited or never opened, also one whose flow ended.
    pub(super) fn reopen_worker(&mut self, id: &str) {
        let key = PaneKey::Flow(id.to_string());
        if self.host.status(&key) == Some(&PaneStatus::Running) {
            return;
        }
        let Some(flow) = self.flow(id).cloned() else {
            return;
        };
        let Some(space) = &flow.workspace else {
            return;
        };
        let Some((name, agent)) = self.config.agent(Some(&space.agent)) else {
            self.host
                .fail(key, format!("no agent named {}", space.agent));
            return;
        };
        match pane::worker_spec(&flow, agent, &self.env) {
            Some(spec) => self.open_worker(spec),
            None => self.host.fail(key, reconcile::unopenable(name)),
        }
    }

    /// Opens a pane once more with the agent's first command: its resume failed before the agent
    /// ever saved the session.
    pub(super) fn open_fresh(&mut self, key: &PaneKey) {
        server_log::write(
            (self.clock)(),
            &format!("resuming {key} failed, starting it fresh"),
        );
        let PaneKey::Flow(id) = key else {
            self.open_master(Start::Fresh);
            return;
        };
        let Some(mut flow) = self.flow(id).cloned() else {
            return;
        };
        let Some(space) = flow.workspace.as_mut() else {
            return;
        };
        space.window = None;
        let spec = self
            .config
            .agent(Some(&space.agent))
            .and_then(|(_, agent)| pane::worker_spec(&flow, agent, &self.env));
        if let Some(spec) = spec
            && let Err(error) = self.open_pane(spec)
        {
            server_log::write((self.clock)(), &format!("{error:#}"));
        }
    }

    /// Hangs up a flow's pane for `flow remove` or `flow clear`; the connection hears Closed once
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
        self.closing.entry(key.clone()).or_default().push(conn);
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
        let Some(waiting) = self.closing.remove(key) else {
            return;
        };
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
}

pub(super) fn forward_host_events(events: Sender<Event>) -> Sender<HostEvent> {
    let (host_events, inbox) = mpsc::channel();
    spawn_named("host events".to_string(), move || {
        for event in inbox {
            if events.send(Event::Host(event)).is_err() {
                break;
            }
        }
    });
    host_events
}
