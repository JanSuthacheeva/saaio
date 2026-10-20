use std::time::{Duration, Instant};

use super::Server;
use crate::adapter::Adapter;
use crate::event::{Event as LogEvent, EventKind};
use crate::fold;
use crate::proto::{PaneKey, PaneStatus};
use crate::server_log;
use crate::view::{FlowView, State};

/// The question a stalled start waits on.
pub(super) const STALLED_START: &str = "no turn began after launch: answer the dialog in its pane";

impl Server {
    /// Blocks a flow whose worker process ended while the flow still says working.
    pub(super) fn notice_exited_worker(&mut self, key: &PaneKey, code: Option<u32>) {
        let PaneKey::Flow(id) = key else {
            return;
        };
        let events = match self.store.events(id) {
            Ok(events) => events,
            Err(error) => {
                server_log::write(
                    (self.clock)(),
                    &format!("could not check exited {key}: {error:#}"),
                );
                return;
            }
        };
        let now = (self.clock)();
        let Some(flow) = fold::fold(id, &events, None, now) else {
            return;
        };
        if !exited_while_working(&flow, self.host.status(key)) {
            return;
        }
        let status = code.map_or_else(
            || "exit status unavailable".to_string(),
            |code| format!("code {code}"),
        );
        let mut reason = format!("agent exited ({status})");
        if let Some(tail) = self.host.output_tail(key, 4, 400)
            && !tail.is_empty()
        {
            reason.push('\n');
            reason.push_str(&tail);
        }
        if let Err(error) = self
            .store
            .append(id, &LogEvent::new(now, EventKind::Blocked { reason }))
        {
            server_log::write((self.clock)(), &format!("{error:#}"));
        }
    }

    /// Appends Waiting { question: STALLED_START } to each flow whose start stalled.
    pub(super) fn notice_stalled_starts(&mut self) {
        let now = Instant::now();
        let ids: Vec<String> = self
            .board
            .flows
            .iter()
            .filter(|flow| {
                let key = PaneKey::Flow(flow.id.clone());
                stalled(
                    flow,
                    self.adapters.get(&key).copied(),
                    self.host.status(&key),
                    self.opened_at.get(&key).copied(),
                    now,
                    self.timing.first_turn_grace,
                )
            })
            .map(|flow| flow.id.clone())
            .collect();
        for id in ids {
            let event = LogEvent::new(
                (self.clock)(),
                EventKind::Waiting {
                    question: STALLED_START.to_string(),
                    questions: Vec::new(),
                },
            );
            if let Err(error) = self.store.append(&id, &event) {
                server_log::write((self.clock)(), &format!("{error:#}"));
            }
        }
    }
}

/// A real worker pane ended while the latest flow state still says working.
fn exited_while_working(flow: &FlowView, status: Option<&PaneStatus>) -> bool {
    !flow.is_plain()
        && flow.state == State::Working
        && matches!(status, Some(PaneStatus::Exited { .. }))
}

/// True when a worker opened at `opened_at` began no turn within `grace` by `now`.
pub(super) fn stalled(
    flow: &FlowView,
    adapter: Option<Adapter>,
    status: Option<&PaneStatus>,
    opened_at: Option<Instant>,
    now: Instant,
    grace: Duration,
) -> bool {
    adapter == Some(Adapter::Codex)
        && status == Some(&PaneStatus::Running)
        && !flow.is_plain()
        && flow.state == State::Working
        && flow.session.is_none()
        && opened_at.is_some_and(|at| now.saturating_duration_since(at) >= grace)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{Event as LogEvent, EventKind, Mode};
    use crate::fold;
    use crate::server::Timing;
    use crate::view::Session;

    fn flow() -> FlowView {
        fold::fold(
            "7f3c",
            &[
                LogEvent::new(
                    0,
                    EventKind::Started {
                        name: "worker".into(),
                        request: "x".into(),
                        mode: Mode::Auto,
                        phases: None,
                    },
                ),
                LogEvent::new(
                    1,
                    EventKind::Workspace {
                        path: "/worktree".into(),
                        branch: None,
                        slot: None,
                        window: Some("saaio-ui".into()),
                        agent: "codex".into(),
                    },
                ),
            ],
            None,
            2,
        )
        .unwrap()
    }

    #[test]
    fn stalls_only_a_running_codex_worker_that_never_began_a_turn() {
        let grace = Timing::default().first_turn_grace;
        let opened = Instant::now();
        let now = opened + grace;
        assert!(stalled(
            &flow(),
            Some(Adapter::Codex),
            Some(&PaneStatus::Running),
            Some(opened),
            now,
            grace
        ));
        assert!(!stalled(
            &flow(),
            Some(Adapter::ClaudeCode),
            Some(&PaneStatus::Running),
            Some(opened),
            now,
            grace
        ));
        assert!(!stalled(
            &flow(),
            Some(Adapter::Codex),
            Some(&PaneStatus::Exited { code: Some(0) }),
            Some(opened),
            now,
            grace
        ));
        let mut plain = flow();
        plain.mode = Mode::Plain;
        assert!(!stalled(
            &plain,
            Some(Adapter::Codex),
            Some(&PaneStatus::Running),
            Some(opened),
            now,
            grace
        ));
        let mut waiting = flow();
        waiting.state = State::Waiting;
        assert!(!stalled(
            &waiting,
            Some(Adapter::Codex),
            Some(&PaneStatus::Running),
            Some(opened),
            now,
            grace
        ));
        let mut started = flow();
        started.session = Some(Session {
            id: "01a1".into(),
            transcript: None,
        });
        assert!(!stalled(
            &started,
            Some(Adapter::Codex),
            Some(&PaneStatus::Running),
            Some(opened),
            now,
            grace
        ));
        assert!(!stalled(
            &flow(),
            Some(Adapter::Codex),
            Some(&PaneStatus::Running),
            None,
            now,
            grace
        ));
        assert!(!stalled(
            &flow(),
            Some(Adapter::Codex),
            Some(&PaneStatus::Running),
            Some(now),
            now,
            grace
        ));
    }

    #[test]
    fn only_an_exited_working_flow_needs_an_exit_blocker() {
        let exited = PaneStatus::Exited { code: Some(0) };
        assert!(exited_while_working(&flow(), Some(&exited)));
        assert!(!exited_while_working(&flow(), Some(&PaneStatus::Running)));
        assert!(!exited_while_working(&flow(), None));
        let mut plain = flow();
        plain.mode = Mode::Plain;
        assert!(!exited_while_working(&plain, Some(&exited)));
        for state in [
            State::Idle,
            State::Waiting,
            State::Blocked,
            State::Done,
            State::Ended,
        ] {
            let mut stopped = flow();
            stopped.state = state;
            assert!(!exited_while_working(&stopped, Some(&exited)), "{state:?}");
        }
    }
}
