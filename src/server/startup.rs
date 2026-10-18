use std::time::{Duration, Instant};

use crate::adapter::Adapter;
use crate::proto::PaneStatus;
use crate::view::{FlowView, State};

/// How long a Codex worker may run with no first turn before its flow shows waiting.
#[cfg(not(test))]
pub(super) const FIRST_TURN_GRACE: Duration = Duration::from_secs(30);
#[cfg(test)]
pub(super) const FIRST_TURN_GRACE: Duration = Duration::from_millis(300);

/// The question a stalled start waits on.
pub(super) const STALLED_START: &str = "no turn began after launch: answer the dialog in its pane";

/// True when a worker opened at `opened_at` began no turn by `now`.
pub(super) fn stalled(
    flow: &FlowView,
    adapter: Option<Adapter>,
    status: Option<&PaneStatus>,
    opened_at: Option<Instant>,
    now: Instant,
) -> bool {
    adapter == Some(Adapter::Codex)
        && status == Some(&PaneStatus::Running)
        && !flow.is_plain()
        && flow.state == State::Working
        && flow.session.is_none()
        && opened_at.is_some_and(|at| now.saturating_duration_since(at) >= FIRST_TURN_GRACE)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{Event as LogEvent, EventKind, Mode};
    use crate::fold;
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
        let now = Instant::now();
        let opened = now - FIRST_TURN_GRACE;
        assert!(stalled(
            &flow(),
            Some(Adapter::Codex),
            Some(&PaneStatus::Running),
            Some(opened),
            now
        ));
        assert!(!stalled(
            &flow(),
            Some(Adapter::ClaudeCode),
            Some(&PaneStatus::Running),
            Some(opened),
            now
        ));
        assert!(!stalled(
            &flow(),
            Some(Adapter::Codex),
            Some(&PaneStatus::Exited { code: Some(0) }),
            Some(opened),
            now
        ));
        let mut plain = flow();
        plain.mode = Mode::Plain;
        assert!(!stalled(
            &plain,
            Some(Adapter::Codex),
            Some(&PaneStatus::Running),
            Some(opened),
            now
        ));
        let mut waiting = flow();
        waiting.state = State::Waiting;
        assert!(!stalled(
            &waiting,
            Some(Adapter::Codex),
            Some(&PaneStatus::Running),
            Some(opened),
            now
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
            now
        ));
        assert!(!stalled(
            &flow(),
            Some(Adapter::Codex),
            Some(&PaneStatus::Running),
            None,
            now
        ));
        assert!(!stalled(
            &flow(),
            Some(Adapter::Codex),
            Some(&PaneStatus::Running),
            Some(now),
            now
        ));
    }
}
