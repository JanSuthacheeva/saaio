use std::time::Duration;

use crate::event::ServiceStatus;
use crate::view::{FlowView, State};

/// What PauseWatch runs for a flow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Job {
    Pause,
    Resume,
}

/// The job a flow needs at `now`; None for a plain session, a flow without a worktree, or with
/// `after` None ([services] off). A failed job is tried again only once the flow's state changed
/// after it; a pausing or resuming no job finishes (the server restarted) runs again.
pub fn next(flow: &FlowView, now: u64, after: Option<Duration>) -> Option<Job> {
    let after = after?;
    if flow.is_plain() || flow.workspace.is_none() || flow.state == State::NotResponding {
        return None;
    }
    let works = matches!(flow.state, State::Working | State::Delegated);
    let rested = rests(flow.state)
        && now.saturating_sub(flow.changed_at)
            >= u64::try_from(after.as_millis()).unwrap_or(u64::MAX);
    let Some(services) = &flow.services else {
        return rested.then_some(Job::Pause);
    };
    let changed_since = flow.changed_at > services.since;
    match services.status {
        ServiceStatus::Running => rested.then_some(Job::Pause),
        ServiceStatus::PauseFailed => (rested && changed_since).then_some(Job::Pause),
        ServiceStatus::Pausing => Some(Job::Pause),
        ServiceStatus::Paused => works.then_some(Job::Resume),
        ServiceStatus::Resuming => Some(Job::Resume),
        ServiceStatus::ResumeFailed if changed_since && works => Some(Job::Resume),
        ServiceStatus::ResumeFailed => (changed_since && rested).then_some(Job::Pause),
    }
}

/// Idle, waiting, blocked, done and ended: a worker that runs nothing.
fn rests(state: State) -> bool {
    matches!(
        state,
        State::Idle | State::Waiting | State::Blocked | State::Done | State::Ended
    )
}

/// Pausing, paused or resuming: a tool call waits.
pub fn holds_tools(flow: &FlowView) -> bool {
    flow.services.as_ref().is_some_and(|services| {
        matches!(
            services.status,
            ServiceStatus::Pausing | ServiceStatus::Paused | ServiceStatus::Resuming
        )
    })
}

/// A tool call of the flow is work it has not reported yet: its services are held and it rests,
/// but not on a question (the tool call may be the one that asks it).
pub fn wakes_at_tool_call(flow: &FlowView) -> bool {
    holds_tools(flow) && rests(flow.state) && flow.state != State::Waiting
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{Event, EventKind, Mode};
    use crate::fold::fold;
    use crate::view::{Services, Workspace};

    const MIN: u64 = 60_000;
    const AFTER: Option<Duration> = Some(Duration::from_mins(5));

    const STATES: [State; 8] = [
        State::Working,
        State::Delegated,
        State::Waiting,
        State::Idle,
        State::Blocked,
        State::NotResponding,
        State::Done,
        State::Ended,
    ];

    /// A started flow with a worktree, in `state` since minute 10, its services at `services`
    /// since `since`.
    fn flow(state: State, services: Option<(ServiceStatus, u64)>) -> FlowView {
        let started = Event::new(
            0,
            EventKind::Started {
                name: "shop-217".into(),
                request: "x".into(),
                mode: Mode::Auto,
                phases: None,
            },
        );
        let mut flow = fold("7f3c", &[started], None, 0).unwrap();
        flow.workspace = Some(Workspace {
            path: "/repo/.saaio/worktrees/shop-217".into(),
            branch: Some("flow/shop-217".into()),
            slot: Some(1),
            window: None,
            agent: "claude".into(),
        });
        flow.state = state;
        flow.changed_at = 10 * MIN;
        flow.services = services.map(|(status, since)| Services {
            status,
            since,
            reason: None,
        });
        flow
    }

    fn jobs(services: Option<(ServiceStatus, u64)>, now: u64) -> Vec<(State, Option<Job>)> {
        STATES
            .iter()
            .map(|&state| (state, next(&flow(state, services), now, AFTER)))
            .collect()
    }

    /// The states of `jobs` that got `job`.
    fn with(jobs: &[(State, Option<Job>)], job: Option<Job>) -> Vec<State> {
        jobs.iter()
            .filter(|(_, got)| *got == job)
            .map(|(state, _)| *state)
            .collect()
    }

    const RESTING: [State; 5] = [
        State::Waiting,
        State::Idle,
        State::Blocked,
        State::Done,
        State::Ended,
    ];

    #[test]
    fn pauses_a_resting_flow_once_it_rested_the_delay() {
        for services in [None, Some((ServiceStatus::Running, MIN))] {
            assert_eq!(with(&jobs(services, 15 * MIN), Some(Job::Pause)), RESTING);
            assert_eq!(
                with(&jobs(services, 15 * MIN - 1), Some(Job::Pause)),
                vec![]
            );
        }
    }

    #[test]
    fn resumes_paused_services_when_the_worker_works() {
        let paused = jobs(Some((ServiceStatus::Paused, 5 * MIN)), 60 * MIN);

        assert_eq!(
            with(&paused, Some(Job::Resume)),
            vec![State::Working, State::Delegated]
        );
        assert_eq!(with(&paused, Some(Job::Pause)), vec![]);
    }

    #[test]
    fn runs_a_pause_or_resume_again_that_never_finished() {
        let every_but_silent: Vec<State> = STATES
            .into_iter()
            .filter(|state| *state != State::NotResponding)
            .collect();

        let pausing = jobs(Some((ServiceStatus::Pausing, 9 * MIN)), 10 * MIN);
        assert_eq!(with(&pausing, Some(Job::Pause)), every_but_silent);
        let resuming = jobs(Some((ServiceStatus::Resuming, 9 * MIN)), 10 * MIN);
        assert_eq!(with(&resuming, Some(Job::Resume)), every_but_silent);
    }

    #[test]
    fn tries_a_failed_pause_again_only_after_the_flow_changed() {
        let before = jobs(Some((ServiceStatus::PauseFailed, 11 * MIN)), 60 * MIN);
        assert_eq!(with(&before, None), STATES);

        let after = jobs(Some((ServiceStatus::PauseFailed, 9 * MIN)), 60 * MIN);
        assert_eq!(with(&after, Some(Job::Pause)), RESTING);
        let early = jobs(Some((ServiceStatus::PauseFailed, 9 * MIN)), 14 * MIN);
        assert_eq!(with(&early, None), STATES);
    }

    #[test]
    fn tries_a_failed_resume_again_at_the_next_turn_or_pauses_after_it() {
        let before = jobs(Some((ServiceStatus::ResumeFailed, 11 * MIN)), 60 * MIN);
        assert_eq!(with(&before, None), STATES);

        let after = jobs(Some((ServiceStatus::ResumeFailed, 9 * MIN)), 60 * MIN);
        assert_eq!(
            with(&after, Some(Job::Resume)),
            vec![State::Working, State::Delegated]
        );
        assert_eq!(with(&after, Some(Job::Pause)), RESTING);
    }

    #[test]
    fn never_pauses_a_plain_session_a_flow_without_a_worktree_or_with_pausing_off() {
        let mut plain = flow(State::Idle, None);
        plain.mode = Mode::Plain;
        let mut bare = flow(State::Idle, None);
        bare.workspace = None;

        assert_eq!(next(&plain, 60 * MIN, AFTER), None);
        assert_eq!(next(&bare, 60 * MIN, AFTER), None);
        assert_eq!(next(&flow(State::Idle, None), 60 * MIN, None), None);
        assert_eq!(
            next(&flow(State::Idle, None), 60 * MIN, AFTER),
            Some(Job::Pause)
        );
    }

    #[test]
    fn holds_tools_while_pausing_paused_or_resuming() {
        let holding: Vec<Option<ServiceStatus>> = [
            None,
            Some(ServiceStatus::Pausing),
            Some(ServiceStatus::Paused),
            Some(ServiceStatus::Resuming),
            Some(ServiceStatus::Running),
            Some(ServiceStatus::PauseFailed),
            Some(ServiceStatus::ResumeFailed),
        ]
        .into_iter()
        .filter(|status| holds_tools(&flow(State::Working, status.map(|status| (status, 0)))))
        .collect();

        assert_eq!(
            holding,
            vec![
                Some(ServiceStatus::Pausing),
                Some(ServiceStatus::Paused),
                Some(ServiceStatus::Resuming)
            ]
        );
    }

    #[test]
    fn a_tool_call_wakes_a_resting_flow_whose_services_are_held() {
        let waking: Vec<State> = STATES
            .into_iter()
            .filter(|&state| wakes_at_tool_call(&flow(state, Some((ServiceStatus::Paused, 0)))))
            .collect();

        assert_eq!(
            waking,
            vec![State::Idle, State::Blocked, State::Done, State::Ended]
        );
        assert!(wakes_at_tool_call(&flow(
            State::Idle,
            Some((ServiceStatus::Pausing, 0))
        )));
        assert!(!wakes_at_tool_call(&flow(
            State::Idle,
            Some((ServiceStatus::Running, 0))
        )));
        assert!(!wakes_at_tool_call(&flow(State::Idle, None)));
    }
}
