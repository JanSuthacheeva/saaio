//! A program's status reports (OSC 7501): the last one kept, and the changes the hooks miss
//! recorded: a turn an Esc ended, a turn that failed, a dialog before a Codex worker's first turn

use std::path::Path;

use super::Server;
use super::panes::waiter_of;
use super::startup::dialog_question;
use crate::adapter::{Adapter, cut};
use crate::event::{Event, EventKind};
use crate::fold;
use crate::id::FlowId;
use crate::program_status::{Need, Report, Status};
use crate::proto::PaneKey;
use crate::server_log;
use crate::store::Busy;
use crate::tail;
use crate::view::{FlowView, MasterTurn, State};

/// The note an interrupted turn's Idle carries, as Codex's Interrupt hook writes it.
const INTERRUPTED: &str = "interrupted";
/// The reason of a failed turn whose transcript names none.
const FAILED: &str = "turn failed: see its pane";

impl Server {
    /// Keeps the pane's last report and acts on the change from the one before: an interrupt, a
    /// failed turn.
    pub(super) fn program_status(&mut self, key: &PaneKey, status: Option<Report>) {
        let before = std::mem::replace(&mut self.pane_mut(key).status, status);
        let after = self.pane(key).status.clone();
        let (before, after) = (before.as_ref(), after.as_ref());
        let adapter = self.pane(key).adapter;
        let idle = after.is_some_and(|report| report.status == Status::Idle);
        if idle && interrupted(before, after, adapter, self.store.busy(&waiter_of(key))) {
            self.interrupt(key);
        }
        if failed_turn(before, after, adapter) {
            self.fail_turn(key);
        }
        if let (PaneKey::Flow(id), Some(report)) = (key, after)
            && report.status == Status::Blocked
            && blocked_at_start(before, after, adapter, self.flow_now(key).as_ref())
        {
            self.wait_on_dialog(*id, report.need);
        }
    }

    /// Records the dialog a worker shows before its first turn: Waiting { dialog_question(need) }
    /// (when fold::changes says it shows).
    fn wait_on_dialog(&mut self, id: FlowId, need: Option<Need>) {
        let now = (self.clock)();
        let question = dialog_question(need);
        let kind = EventKind::waiting(question);
        let recorded = self.store.events(id).and_then(|events| {
            if fold::changes(id, &events, &kind, now) {
                self.store.append(id, &Event::new(now, kind))
            } else {
                Ok(())
            }
        });
        match recorded {
            Ok(()) => server_log::write(now, &format!("flow {id} waits: {question}")),
            Err(error) => server_log::write(
                now,
                &format!("could not record flow {id} as waiting: {error:#}"),
            ),
        }
    }

    /// Records the turn that failed: Blocked { "turn failed: …" } in a flow's log (when
    /// fold::changes says it shows), master.turn idle for the master; the session idle.
    fn fail_turn(&mut self, key: &PaneKey) {
        let now = (self.clock)();
        let transcript = self
            .flow_now(key)
            .and_then(|flow| flow.session)
            .and_then(|session| session.transcript);
        let message = match (self.pane(key).adapter, transcript) {
            (Some(adapter), Some(transcript)) => {
                adapter.turn_error(Path::new(&transcript), |file, matches| {
                    tail::last_line(file, matches, tail::CHUNK).unwrap_or_else(|error| {
                        let file = file.display();
                        server_log::write(now, &format!("could not read {file}: {error}"));
                        None
                    })
                })
            }
            _ => None,
        };
        let reason = message.map_or_else(
            || FAILED.to_string(),
            |message| cut(&format!("turn failed: {message}")),
        );
        let recorded = match key {
            PaneKey::Flow(id) => {
                let kind = EventKind::Blocked {
                    reason: reason.clone(),
                };
                self.store.events(*id).and_then(|events| {
                    if fold::changes(*id, &events, &kind, now) {
                        self.store.append(*id, &Event::new(now, kind))
                    } else {
                        Ok(())
                    }
                })
            }
            PaneKey::Master => self.store.set_master_turn(&MasterTurn {
                state: State::Idle,
                at: now,
            }),
        };
        if let Err(error) = recorded {
            server_log::write(now, &format!("could not record {key} as failed: {error:#}"));
        }
        if let Err(error) = self.store.set_busy(&waiter_of(key), Busy::Idle) {
            server_log::write(now, &format!("{error:#}"));
        }
        server_log::write(now, &format!("{key}'s turn failed: {reason}"));
        if *key == PaneKey::Master {
            self.send_master();
        }
    }

    /// The flow behind a pane, folded from its log now (the board may be a tick behind the
    /// hooks); None for the master and a flow whose log cannot be read.
    fn flow_now(&self, key: &PaneKey) -> Option<FlowView> {
        let PaneKey::Flow(id) = key else {
            return None;
        };
        let events = self.store.events(*id).ok()?;
        fold::fold(*id, &events, None, (self.clock)())
    }

    /// Records the turn an Esc ended: Idle { note: "interrupted" } in a flow's log (when
    /// fold::changes says it shows), master.turn idle for the master, the session idle.
    fn interrupt(&mut self, key: &PaneKey) {
        let now = (self.clock)();
        let recorded = match key {
            PaneKey::Flow(id) => {
                let kind = EventKind::Idle {
                    note: INTERRUPTED.to_string(),
                };
                self.store.events(*id).and_then(|events| {
                    if fold::changes(*id, &events, &kind, now) {
                        self.store.append(*id, &Event::new(now, kind))
                    } else {
                        Ok(())
                    }
                })
            }
            PaneKey::Master => self.store.set_master_turn(&MasterTurn {
                state: State::Idle,
                at: now,
            }),
        };
        if let Err(error) = recorded {
            server_log::write(
                now,
                &format!("could not record {key} as interrupted: {error:#}"),
            );
        }
        if let Err(error) = self.store.set_busy(&waiter_of(key), Busy::Idle) {
            server_log::write(now, &format!("{error:#}"));
        }
        server_log::write(now, &format!("{key} was interrupted: idle"));
        if *key == PaneKey::Master {
            self.send_master();
        }
    }
}

/// True when `after` ends a turn the hooks say still runs: idle right after working or
/// blocked, in a harness that runs no hook on an interrupt.
fn interrupted(
    before: Option<&Report>,
    after: Option<&Report>,
    adapter: Option<Adapter>,
    busy: Busy,
) -> bool {
    adapter.is_some_and(|adapter| !adapter.hooks_interrupt())
        && busy == Busy::Turn
        && before.is_some_and(|report| !report.at_rest())
        && after.is_some_and(|report| report.status == Status::Idle)
}

/// True when `after` ends a running turn on an error: error right after working or blocked, in
/// a session with an adapter (any harness; a plain session never).
fn failed_turn(before: Option<&Report>, after: Option<&Report>, adapter: Option<Adapter>) -> bool {
    adapter.is_some()
        && before.is_some_and(|report| !report.at_rest())
        && after.is_some_and(|report| report.status == Status::Error)
}

/// True when `after` is a dialog a Codex worker shows before its first turn: blocked, not
/// blocked before, its flow working with no session yet.
fn blocked_at_start(
    before: Option<&Report>,
    after: Option<&Report>,
    adapter: Option<Adapter>,
    flow: Option<&FlowView>,
) -> bool {
    adapter == Some(Adapter::Codex)
        && before.is_none_or(|report| report.status != Status::Blocked)
        && after.is_some_and(|report| report.status == Status::Blocked)
        && flow.is_some_and(|flow| {
            !flow.is_plain() && flow.state == State::Working && flow.session.is_none()
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{Event as LogEvent, Mode};
    use crate::fixtures;
    use crate::view::{Session, Workspace};

    fn report(status: Status) -> Report {
        Report {
            status,
            need: None,
            message: None,
        }
    }

    #[test]
    fn idle_right_after_working_or_blocked_ends_a_claude_code_turn() {
        for before in [Status::Working, Status::Blocked] {
            assert!(interrupted(
                Some(&report(before)),
                Some(&report(Status::Idle)),
                Some(Adapter::ClaudeCode),
                Busy::Turn,
            ));
        }
    }

    #[test]
    fn only_idle_ends_it_never_done_working_or_a_clear() {
        for after in [
            Some(report(Status::Done)),
            Some(report(Status::Error)),
            Some(report(Status::Working)),
            Some(report(Status::Blocked)),
            None,
        ] {
            assert!(
                !interrupted(
                    Some(&report(Status::Working)),
                    after.as_ref(),
                    Some(Adapter::ClaudeCode),
                    Busy::Turn,
                ),
                "{after:?}"
            );
        }
    }

    #[test]
    fn idle_after_rest_or_nothing_ends_nothing() {
        for before in [
            None,
            Some(report(Status::Idle)),
            Some(report(Status::Done)),
            Some(report(Status::Error)),
        ] {
            assert!(
                !interrupted(
                    before.as_ref(),
                    Some(&report(Status::Idle)),
                    Some(Adapter::ClaudeCode),
                    Busy::Turn,
                ),
                "{before:?}"
            );
        }
    }

    #[test]
    fn ends_only_a_turn_the_hooks_say_runs() {
        for busy in [Busy::Idle, Busy::Typed] {
            assert!(
                !interrupted(
                    Some(&report(Status::Working)),
                    Some(&report(Status::Idle)),
                    Some(Adapter::ClaudeCode),
                    busy,
                ),
                "{busy:?}"
            );
        }
    }

    #[test]
    fn leaves_harnesses_with_an_interrupt_hook_and_plain_sessions_alone() {
        for adapter in [Some(Adapter::Codex), None] {
            assert!(
                !interrupted(
                    Some(&report(Status::Working)),
                    Some(&report(Status::Idle)),
                    adapter,
                    Busy::Turn,
                ),
                "{adapter:?}"
            );
        }
    }

    #[test]
    fn error_right_after_working_or_blocked_fails_a_turn_in_any_harness() {
        for adapter in [Adapter::ClaudeCode, Adapter::Codex] {
            for before in [Status::Working, Status::Blocked] {
                assert!(
                    failed_turn(
                        Some(&report(before)),
                        Some(&report(Status::Error)),
                        Some(adapter),
                    ),
                    "{adapter:?} {before:?}"
                );
            }
        }
    }

    #[test]
    fn only_an_error_after_a_running_turn_fails_it() {
        for before in [
            None,
            Some(report(Status::Idle)),
            Some(report(Status::Done)),
            Some(report(Status::Error)),
        ] {
            assert!(
                !failed_turn(
                    before.as_ref(),
                    Some(&report(Status::Error)),
                    Some(Adapter::Codex),
                ),
                "{before:?}"
            );
        }
        for after in [Status::Idle, Status::Done, Status::Working, Status::Blocked] {
            assert!(
                !failed_turn(
                    Some(&report(Status::Working)),
                    Some(&report(after)),
                    Some(Adapter::Codex),
                ),
                "{after:?}"
            );
        }
        assert!(!failed_turn(
            Some(&report(Status::Working)),
            None,
            Some(Adapter::Codex)
        ));
    }

    #[test]
    fn a_plain_session_never_fails_a_turn() {
        assert!(!failed_turn(
            Some(&report(Status::Working)),
            Some(&report(Status::Error)),
            None,
        ));
    }

    #[test]
    fn the_recorded_done_and_interrupted_turns_never_fail() {
        for ending in [Status::Done, Status::Idle] {
            let turn = [Status::Idle, Status::Working, ending];
            for pair in turn.windows(2) {
                assert!(!failed_turn(
                    Some(&report(pair[0])),
                    Some(&report(pair[1])),
                    Some(Adapter::Codex),
                ));
            }
        }
    }

    fn starting() -> FlowView {
        let workspace = Workspace {
            window: Some("saaio-ui".into()),
            ..fixtures::workspace("worker", "codex")
        };
        fixtures::flow("worker", vec![LogEvent::new(1, workspace.into())], 2)
    }

    fn blocked() -> Report {
        Report {
            need: Some(Need::Auth),
            ..report(Status::Blocked)
        }
    }

    #[test]
    fn a_codex_worker_blocked_before_its_first_turn_waits_on_the_dialog() {
        for before in [
            None,
            Some(report(Status::Idle)),
            Some(report(Status::Working)),
        ] {
            assert!(
                blocked_at_start(
                    before.as_ref(),
                    Some(&blocked()),
                    Some(Adapter::Codex),
                    Some(&starting()),
                ),
                "{before:?}"
            );
        }
    }

    #[test]
    fn only_a_new_block_waits() {
        assert!(!blocked_at_start(
            Some(&report(Status::Blocked)),
            Some(&blocked()),
            Some(Adapter::Codex),
            Some(&starting()),
        ));
        for after in [Status::Idle, Status::Working, Status::Done, Status::Error] {
            assert!(
                !blocked_at_start(
                    None,
                    Some(&report(after)),
                    Some(Adapter::Codex),
                    Some(&starting()),
                ),
                "{after:?}"
            );
        }
        assert!(!blocked_at_start(
            None,
            None,
            Some(Adapter::Codex),
            Some(&starting())
        ));
    }

    #[test]
    fn a_started_waiting_or_plain_flow_and_other_harnesses_never_wait_on_it() {
        let mut started = starting();
        started.session = Some(Session {
            id: fixtures::session_id("01a1"),
            transcript: None,
        });
        let mut waiting = starting();
        waiting.state = State::Waiting;
        let mut plain = starting();
        plain.mode = Mode::Plain;
        for flow in [Some(&started), Some(&waiting), Some(&plain), None] {
            assert!(
                !blocked_at_start(None, Some(&blocked()), Some(Adapter::Codex), flow),
                "{flow:?}"
            );
        }
        for adapter in [Some(Adapter::ClaudeCode), None] {
            assert!(
                !blocked_at_start(None, Some(&blocked()), adapter, Some(&starting())),
                "{adapter:?}"
            );
        }
    }
}
