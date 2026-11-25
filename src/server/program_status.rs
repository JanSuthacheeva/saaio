//! A program's status reports (OSC 7501): the last one kept, and a turn an Esc ended recorded

use super::Server;
use super::panes::waiter_of;
use crate::adapter::Adapter;
use crate::event::{Event, EventKind};
use crate::fold;
use crate::program_status::{Report, Status};
use crate::proto::PaneKey;
use crate::server_log;
use crate::store::Busy;
use crate::view::{MasterTurn, State};

/// The note an interrupted turn's Idle carries, as Codex's Interrupt hook writes it.
const INTERRUPTED: &str = "interrupted";

impl Server {
    /// Keeps the pane's last report and acts on the change from the one before.
    pub(super) fn program_status(&mut self, key: &PaneKey, status: Option<Report>) {
        let before = std::mem::replace(&mut self.pane_mut(key).status, status);
        let after = self.pane(key).status.as_ref();
        let adapter = self.pane(key).adapter;
        if after.is_none_or(|report| report.status != Status::Idle) {
            return;
        }
        let busy = self.store.busy(&waiter_of(key));
        if interrupted(before.as_ref(), after, adapter, busy) {
            self.interrupt(key);
        }
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
