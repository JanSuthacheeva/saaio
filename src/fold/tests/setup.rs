//! A setup run's outcome: kept until the next run, whatever the worker does meanwhile

use super::*;
use crate::view::SetupRun;

const LOG: &str = "/store/hooks/7f3c/setup.log";

fn setup(at_ms: u64, failed: Option<&str>) -> Event {
    at(
        at_ms,
        EventKind::Setup {
            failed: failed.map(str::to_string),
            log: LOG.into(),
        },
    )
}

#[test]
fn a_failed_setup_blocks_the_flow_and_outlives_working() {
    let failed = [started(0), setup(1, Some("Please run composer update"))];
    let problem = format!("setup failed: Please run composer update (full output: {LOG})");

    let blocked = view(&failed, 2);
    assert_eq!(blocked.state, State::Blocked);
    assert_eq!(blocked.blocker.as_deref(), Some(problem.as_str()));
    assert_eq!(blocked.setup_problem().as_deref(), Some(problem.as_str()));
    assert_eq!(
        texts(&blocked),
        vec!["started".to_string(), format!("blocked: {problem}")]
    );

    let working = view(&[&failed[..], &[at(3, EventKind::Working)]].concat(), 4);
    assert_eq!(working.state, State::Working);
    assert_eq!(working.blocker, None);
    assert_eq!(
        working.setup,
        Some(SetupRun {
            failed: Some("Please run composer update".into()),
            log: LOG.into(),
            at: 1,
        })
    );
    assert_eq!(working.setup_problem().as_deref(), Some(problem.as_str()));
}

#[test]
fn a_pass_after_a_failure_says_so_and_lets_go_of_the_flow() {
    let flow = view(
        &[
            started(0),
            setup(1, Some("Please run composer update")),
            setup(2, None),
        ],
        3,
    );

    assert_eq!(flow.state, State::Idle);
    assert_eq!(flow.blocker, None);
    assert_eq!(flow.setup_problem(), None);
    assert_eq!(texts(&flow).last(), Some(&"setup passed"));
    assert_eq!(flow.changed_at, 2);
}

#[test]
fn a_pass_leaves_a_flow_that_moved_on_where_it_is() {
    let flow = view(
        &[
            started(0),
            setup(1, Some("Please run composer update")),
            at(2, EventKind::Working),
            setup(3, None),
        ],
        4,
    );

    assert_eq!(flow.state, State::Working);
    assert_eq!(texts(&flow).last(), Some(&"setup passed"));
}

#[test]
fn a_pass_alone_adds_no_note() {
    let flow = view(&[started(0), at(1, EventKind::Working), setup(2, None)], 3);

    assert_eq!(flow.state, State::Working);
    assert_eq!(texts(&flow), vec!["started"]);
    assert_eq!(flow.setup.map(|run| run.at), Some(2));
}

#[test]
fn a_setup_after_the_end_is_ignored() {
    let flow = view(
        &[
            started(0),
            at(1, EventKind::Ended),
            setup(2, Some("Please run composer update")),
        ],
        3,
    );

    assert_eq!(flow.state, State::Ended);
    assert_eq!(flow.setup, None);
}

#[test]
fn a_failure_while_the_worker_works_leaves_it_working() {
    let flow = view(
        &[
            started(0),
            at(1, EventKind::Working),
            setup(2, Some("Please run composer update")),
        ],
        3,
    );
    let problem = format!("setup failed: Please run composer update (full output: {LOG})");

    assert_eq!(flow.state, State::Working);
    assert_eq!(flow.blocker, None);
    assert_eq!(flow.setup_problem().as_deref(), Some(problem.as_str()));
    assert_eq!(texts(&flow).last(), Some(&problem.as_str()));
}

#[test]
fn a_failure_after_the_turn_ended_blocks_the_flow() {
    let flow = view(
        &[
            started(0),
            at(1, EventKind::Working),
            at(
                2,
                EventKind::Idle {
                    note: "done".into(),
                },
            ),
            setup(3, Some("Please run composer update")),
        ],
        4,
    );

    assert_eq!(flow.state, State::Blocked);
}
