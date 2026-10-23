//! Tests of `fold`: states, phases, steps, merge requests, services, the board, name lookups

use super::*;
use crate::event::{Question, ServiceStatus};
use crate::fixtures;
use crate::view::Services;

const MIN: u64 = 60_000;

#[test]
fn codex_permission_waits_after_a_grace_period_and_completion_cancels_it() {
    let pending = at(
        1_000,
        EventKind::PermissionPending {
            question: "permission: Bash(touch notes.txt)".into(),
        },
    );
    let started = started(0);
    let events = [started, pending];
    assert!(changes("x", &events[..1], &events[1].kind, 1_000));
    assert_eq!(view(&events, 3_999).state, State::Working);
    let waiting = view(&events, 4_000);
    assert_eq!(waiting.state, State::Waiting);
    assert_eq!(
        waiting.blocker.as_deref(),
        Some("permission: Bash(touch notes.txt)")
    );
    assert_eq!(waiting.changed_at, 4_000);
    assert!(waiting.permission);
    assert!(!view(&events, 3_999).permission, "inside its grace");
    assert!(changes("x", &events, &EventKind::Working, 2_000));
    let completed = [
        events[0].clone(),
        events[1].clone(),
        at(2_000, EventKind::Working),
    ];
    assert_eq!(view(&completed, 10_000).state, State::Working);
    assert_eq!(texts(&view(&completed, 10_000)), vec!["started"]);
}

#[test]
fn an_idle_worker_does_not_need_you_but_shows_its_last_line() {
    let idle = |note: &str| EventKind::Idle { note: note.into() };
    let events = vec![
        started(0),
        Event::new(1, idle("The review is still running.")),
    ];

    let flow = fold("x", &events, None, 2).unwrap();

    assert_eq!(flow.state, State::Idle);
    assert!(!flow.needs_you());
    assert_eq!(flow.blocker, None);
    assert_eq!(
        flow.timeline.last().unwrap().text,
        "idle: The review is still running."
    );
    assert!(!changes(
        "x",
        &events,
        &idle("The review is still running."),
        3
    ));
    assert!(changes("x", &events, &idle("Tests pass."), 3));
    assert!(changes("x", &events, &EventKind::Working, 3));
}

#[test]
fn a_message_is_in_the_timeline_and_leaves_the_state() {
    let events = vec![
        started(0),
        at(1, EventKind::waiting("Which delimiter?")),
        at(
            2,
            EventKind::Message {
                text: "Semicolons.".into(),
            },
        ),
    ];

    let flow = fold("x", &events, None, 3).unwrap();

    assert_eq!(flow.state, State::Waiting);
    assert_eq!(flow.changed_at, 1);
    assert_eq!(flow.timeline.last().unwrap().text, "master: Semicolons.");
    assert!(changes(
        "x",
        &events,
        &EventKind::Message {
            text: "Semicolons.".into()
        },
        3
    ));
}

#[test]
fn a_typed_line_is_in_the_timeline_and_leaves_the_state() {
    let typed = EventKind::Typed {
        text: "/compact".into(),
    };
    let events = vec![
        started(0),
        at(
            1,
            EventKind::Idle {
                note: "plan done".into(),
            },
        ),
        at(2, typed.clone()),
    ];

    let flow = fold("x", &events, None, 3).unwrap();

    assert_eq!(flow.state, State::Idle);
    assert_eq!(flow.changed_at, 1);
    assert_eq!(flow.timeline.last().unwrap().text, "master typed: /compact");
    assert!(changes("x", &events, &typed, 3));
}

#[test]
fn keeps_the_last_answer_whole() {
    let long = "word ".repeat(300);
    let answer = |text: &str| EventKind::Answer { text: text.into() };
    let events = vec![started(0), at(1, answer("first")), at(2, answer(&long))];

    let flow = fold("x", &events, None, 3).unwrap();

    assert_eq!(flow.answer.as_deref(), Some(long.as_str()));
    assert_eq!(flow.timeline.len(), 1, "an answer adds no entry");
    assert!(!changes("x", &events, &answer(&long), 3));
    assert!(changes("x", &events, &answer("other"), 3));
}

#[test]
fn keeps_the_harness_session_without_showing_it() {
    let session = |id: &str| EventKind::Session {
        id: id.into(),
        transcript: Some(format!("/home/dev/.codex/sessions/rollout-{id}.jsonl")),
    };
    let events = vec![started(0), at(1, session("01a1"))];

    let flow = fold("x", &events, None, 2).unwrap();

    assert_eq!(
        flow.session,
        Some(Session {
            id: "01a1".into(),
            transcript: Some("/home/dev/.codex/sessions/rollout-01a1.jsonl".into()),
        })
    );
    assert_eq!(flow.timeline.len(), 1, "a session adds no entry");
    assert_eq!(flow.state, fold("x", &events[..1], None, 2).unwrap().state);
    assert!(!changes("x", &events, &session("01a1"), 2), "a repeat");
    assert!(changes("x", &events, &session("02b2"), 2), "a new session");
    assert_eq!(fold("x", &events[..1], None, 2).unwrap().session, None);
}

#[test]
fn a_new_idle_note_counts_as_a_change() {
    let idle = |note: &str| EventKind::Idle { note: note.into() };
    let events = vec![
        started(0),
        at(1, idle("Design is ready.")),
        at(5, idle("Plan is ready.")),
    ];

    assert_eq!(fold("x", &events, None, 6).unwrap().changed_at, 5);
}

#[test]
fn a_phase_a_delegate_starts_keeps_the_flow_delegated() {
    let events = vec![
        started(0),
        Event::new(
            1,
            EventKind::Delegated {
                description: "reviewer".into(),
            },
        ),
        Event::new(
            2,
            EventKind::Phase {
                phase: Phase::of("review"),
            },
        ),
    ];

    let flow = fold("x", &events, None, 3).unwrap();

    assert_eq!(flow.state, State::Delegated);
    assert_eq!(flow.phase, Some(Phase::of("review")));
    assert_eq!(flow.delegate.as_deref(), Some("reviewer"));
}

#[test]
fn the_master_is_in_the_state_its_last_saying_kind_puts_it() {
    let kinds = [
        EventKind::Working,
        EventKind::Answer {
            text: "done".into(),
        },
        EventKind::Idle {
            note: "done".into(),
        },
        EventKind::Session {
            id: "s1".into(),
            transcript: None,
        },
    ];

    assert_eq!(master_state(&kinds), Some(State::Idle));
    assert_eq!(
        master_state(&[EventKind::Blocked {
            reason: "may I?".into()
        }]),
        Some(State::Blocked)
    );
}

#[test]
fn kinds_that_say_no_state_leave_the_master_as_it_was() {
    let kinds = [
        EventKind::Answer {
            text: "done".into(),
        },
        EventKind::Phase {
            phase: Phase::of("implement"),
        },
    ];

    assert_eq!(master_state(&kinds), None);
    assert_eq!(master_state(&[]), None);
}

fn started(at: u64) -> Event {
    Event {
        at,
        ..fixtures::started("shop-221", "SHOP-221 add CSV export")
    }
}

fn at(at: u64, kind: EventKind) -> Event {
    Event::new(at, kind)
}

fn phase(at_ms: u64, phase: Phase) -> Event {
    at(at_ms, EventKind::Phase { phase })
}

fn view(events: &[Event], now: u64) -> FlowView {
    fold("7f3c", events, None, now).expect("a started flow folds")
}

fn texts(view: &FlowView) -> Vec<&str> {
    view.timeline
        .iter()
        .map(|entry| entry.text.as_str())
        .collect()
}

fn steps(view: &FlowView) -> Vec<(StepStatus, Option<u64>)> {
    view.steps
        .iter()
        .map(|step| (step.status, step.duration_ms))
        .collect()
}

#[test]
fn says_whether_an_event_changes_what_the_flow_shows() {
    let started = Event::new(
        0,
        EventKind::Started {
            name: "csv".into(),
            request: "x".into(),
            mode: Mode::Auto,
            phases: None,
        },
    );
    let waiting = Event::new(1, EventKind::waiting("Which?"));
    let events = vec![started.clone(), waiting.clone()];

    assert!(
        !changes("id", &events, &waiting.kind, 2),
        "the same question again"
    );
    assert!(changes("id", &events, &EventKind::Working, 2));
    assert!(
        !changes("id", std::slice::from_ref(&started), &EventKind::Working, 2),
        "it already works"
    );
    assert!(changes(
        "id",
        &[started],
        &EventKind::Phase {
            phase: Phase::of("plan")
        },
        2
    ));
    assert!(
        changes("id", &[], &EventKind::Working, 2),
        "a log without a start"
    );
}

#[test]
fn a_plain_session_has_no_steps_but_keeps_its_phase() {
    let plain = at(
        1,
        EventKind::Started {
            name: "scratch".into(),
            request: String::new(),
            mode: Mode::Plain,
            phases: None,
        },
    );

    assert_eq!(
        view(std::slice::from_ref(&plain), 2).state,
        State::Idle,
        "it opens at an empty prompt"
    );
    let flow = view(&[plain, phase(2, Phase::of("plan"))], 3);

    assert!(flow.steps.is_empty());
    assert_eq!(flow.phase, Some(Phase::of("plan")));
}

#[test]
fn needs_a_started_event_to_name_the_flow() {
    assert_eq!(fold("7f3c", &[at(1, EventKind::Working)], None, 2), None);
    assert_eq!(fold("7f3c", &[], None, 2), None);
}

#[test]
fn a_started_flow_is_working_with_four_open_steps() {
    let flow = view(&[started(1_000)], 2_000);

    assert_eq!(flow.id, "7f3c");
    assert_eq!(flow.name, "shop-221");
    assert_eq!(flow.request, "SHOP-221 add CSV export");
    assert_eq!(flow.mode, Mode::Auto);
    assert_eq!(flow.state, State::Working);
    assert_eq!(flow.phase, None);
    assert_eq!(flow.started_at, 1_000);
    assert_eq!(flow.changed_at, 1_000);
    assert_eq!(
        flow.steps
            .iter()
            .map(|step| step.phase.name())
            .collect::<Vec<_>>(),
        ["plan", "implement", "review", "merge-request"]
    );
    assert_eq!(steps(&flow), vec![(StepStatus::Open, None); 4]);
    assert_eq!(texts(&flow), vec!["started"]);
    assert_eq!(flow.timeline[0].tone, Tone::Neutral);
}

#[test]
fn phases_close_the_previous_step_and_the_current_one_runs_until_now() {
    let flow = view(
        &[
            started(0),
            phase(0, Phase::of("plan")),
            phase(9 * MIN, Phase::of("implement")),
        ],
        40 * MIN,
    );

    assert_eq!(flow.phase, Some(Phase::of("implement")));
    assert_eq!(
        steps(&flow),
        vec![
            (StepStatus::Done, Some(9 * MIN)),
            (StepStatus::Current, Some(31 * MIN)),
            (StepStatus::Open, None),
            (StepStatus::Open, None),
        ]
    );
    assert_eq!(
        texts(&flow),
        vec!["started", "phase: plan", "phase: implement"]
    );
    assert_eq!(flow.timeline[1].tone, Tone::Progress);
    assert_eq!(flow.changed_at, 9 * MIN);
}

#[test]
fn going_back_a_phase_adds_to_its_time_and_keeps_later_steps_done() {
    let flow = view(
        &[
            started(0),
            phase(0, Phase::of("implement")),
            phase(10 * MIN, Phase::of("review")),
            phase(15 * MIN, Phase::of("implement")),
        ],
        20 * MIN,
    );

    assert_eq!(
        steps(&flow),
        vec![
            (StepStatus::Done, None),
            (StepStatus::Current, Some(15 * MIN)),
            (StepStatus::Done, Some(5 * MIN)),
            (StepStatus::Open, None),
        ]
    );
}

#[test]
fn the_same_phase_reported_twice_changes_nothing() {
    let once = view(&[started(0), phase(1, Phase::of("plan"))], 10);
    let twice = view(
        &[
            started(0),
            phase(1, Phase::of("plan")),
            phase(5, Phase::of("plan")),
        ],
        10,
    );

    assert_eq!(once, twice);
}

#[test]
fn a_new_phase_clears_a_blocker_and_a_delegate() {
    let flow = view(
        &[
            started(0),
            at(
                1,
                EventKind::Delegated {
                    description: "build".into(),
                },
            ),
            at(
                2,
                EventKind::Blocked {
                    reason: "no default".into(),
                },
            ),
            phase(3, Phase::of("review")),
        ],
        4,
    );

    assert_eq!(flow.state, State::Working);
    assert_eq!(flow.blocker, None);
    assert_eq!(flow.delegate, None);
}

#[test]
fn delegated_keeps_what_the_agent_does() {
    let flow = view(
        &[
            started(0),
            at(
                5,
                EventKind::Delegated {
                    description: "implement plan".into(),
                },
            ),
        ],
        6,
    );

    assert_eq!(flow.state, State::Delegated);
    assert_eq!(flow.delegate.as_deref(), Some("implement plan"));
    assert_eq!(flow.changed_at, 5);
    assert_eq!(texts(&flow), vec!["started"]);
}

#[test]
fn waiting_and_blocked_set_the_blocker_once_per_question() {
    let waiting = view(
        &[
            started(0),
            at(1, EventKind::waiting("Approve the plan?")),
            at(2, EventKind::waiting("Approve the plan?")),
        ],
        3,
    );
    assert_eq!(waiting.state, State::Waiting);
    assert_eq!(waiting.blocker.as_deref(), Some("Approve the plan?"));
    assert_eq!(
        texts(&waiting),
        vec!["started", "waiting: Approve the plan?"]
    );
    assert_eq!(waiting.timeline[1].tone, Tone::Attention);
    assert_eq!(waiting.changed_at, 1);

    let blocked = view(
        &[
            started(0),
            at(
                1,
                EventKind::Blocked {
                    reason: "no default".into(),
                },
            ),
            at(
                2,
                EventKind::Blocked {
                    reason: "no default".into(),
                },
            ),
            at(
                3,
                EventKind::Blocked {
                    reason: "tests red".into(),
                },
            ),
        ],
        4,
    );
    assert_eq!(blocked.state, State::Blocked);
    assert_eq!(blocked.blocker.as_deref(), Some("tests red"));
    assert_eq!(
        texts(&blocked),
        vec!["started", "blocked: no default", "blocked: tests red"]
    );
    assert_eq!(blocked.timeline[1].tone, Tone::Problem);
}

fn dialog(at_ms: u64, questions: Vec<Question>) -> Event {
    let question = questions[0].question.clone();
    at(
        at_ms,
        EventKind::Waiting {
            question,
            questions,
            permission: false,
        },
    )
}

#[test]
fn a_dialog_shows_its_questions_until_the_flow_leaves_waiting() {
    let asked = vec![
        Question::of("Which color?", &["Red", "Blue"], false),
        Question::of("Which toppings?", &["Cheese", "Ham"], true),
    ];
    let events = vec![started(0), dialog(1, asked.clone())];

    let flow = view(&events, 2);
    assert_eq!(flow.state, State::Waiting);
    assert_eq!(flow.blocker.as_deref(), Some("Which color?"));
    assert_eq!(flow.questions, asked);

    let worded = EventKind::waiting("Ship it?");
    for next in [
        EventKind::Working,
        EventKind::Idle {
            note: "declined".into(),
        },
        EventKind::Blocked {
            reason: "no answer".into(),
        },
        worded,
        EventKind::Ended,
    ] {
        let mut after = events.clone();
        after.push(at(3, next.clone()));
        assert_eq!(view(&after, 4).questions, Vec::new(), "{next:?}");
    }
    let mut messaged = events.clone();
    messaged.push(at(3, EventKind::Message { text: "Red".into() }));
    assert_eq!(
        view(&messaged, 4).questions,
        asked,
        "a message leaves it open"
    );
}

#[test]
fn a_permission_prompt_shows_until_the_flow_leaves_waiting() {
    let asked = EventKind::Waiting {
        question: "permission: Bash(curl -sI https://example.org)".into(),
        questions: Vec::new(),
        permission: true,
    };
    let events = vec![started(0), at(1, asked)];

    let flow = view(&events, 2);
    assert_eq!(flow.state, State::Waiting);
    assert!(flow.permission);
    assert!(!view(&[started(0), at(1, EventKind::waiting("Ship it?"))], 2).permission);

    for next in [
        EventKind::Working,
        EventKind::waiting("Ship it?"),
        EventKind::Ended,
    ] {
        let mut after = events.clone();
        after.push(at(3, next.clone()));
        assert!(!view(&after, 4).permission, "{next:?}");
    }
    let mut messaged = events.clone();
    messaged.push(at(3, EventKind::Message { text: "Hi".into() }));
    assert!(view(&messaged, 4).permission, "a message leaves it open");
}

#[test]
fn a_question_or_reason_does_not_outlive_waiting_or_blocked() {
    let asked = at(
        2 * MIN,
        EventKind::waiting("Should I also update the README?"),
    );
    let stuck = at(
        2 * MIN,
        EventKind::Blocked {
            reason: "exited with code 1".into(),
        },
    );
    let delegated = EventKind::Delegated {
        description: "review the branch".into(),
    };
    for stop in [asked, stuck] {
        for next in [EventKind::Ended, delegated.clone()] {
            let events = [
                started(0),
                opened(MIN, PR),
                stop.clone(),
                at(3 * MIN, next.clone()),
            ];
            let flow = view(&events, 4 * MIN);
            assert_eq!(flow.blocker, None, "{:?} then {next:?}", stop.kind);
        }
    }
}

#[test]
fn a_new_dialog_with_the_same_first_question_is_a_change() {
    let events = vec![
        started(0),
        dialog(
            1,
            vec![Question::of("Which color?", &["Red", "Blue"], false)],
        ),
    ];
    let same = events[1].kind.clone();
    let other = EventKind::Waiting {
        question: "Which color?".into(),
        questions: vec![Question::of("Which color?", &["Red", "Teal"], false)],
        permission: false,
    };

    assert!(!changes("x", &events, &same, 2));
    assert!(changes("x", &events, &other, 2));
}

#[test]
fn working_after_a_stop_is_an_answer_and_clears_the_blocker() {
    let flow = view(
        &[
            started(0),
            at(1, EventKind::Working),
            at(
                2,
                EventKind::Blocked {
                    reason: "no default".into(),
                },
            ),
            at(3, EventKind::Working),
            at(4, EventKind::Working),
        ],
        5,
    );

    assert_eq!(flow.state, State::Working);
    assert_eq!(flow.blocker, None);
    assert_eq!(
        texts(&flow),
        vec!["started", "blocked: no default", "answered"]
    );
    assert_eq!(flow.changed_at, 3);
}

#[test]
fn decisions_are_kept_once_and_do_not_change_the_state() {
    let flow = view(
        &[
            started(0),
            at(1, EventKind::waiting("Which approach?")),
            at(
                2,
                EventKind::Decision {
                    text: "approach B".into(),
                },
            ),
            at(
                3,
                EventKind::Decision {
                    text: "approach B".into(),
                },
            ),
        ],
        4,
    );

    assert_eq!(flow.decisions, vec!["approach B"]);
    assert_eq!(flow.state, State::Waiting);
    assert_eq!(
        texts(&flow),
        vec!["started", "waiting: Which approach?", "decided: approach B"]
    );
    assert_eq!(flow.changed_at, 1);
}

#[test]
fn tasks_and_review_counts_are_the_latest_report() {
    let flow = view(
        &[
            started(0),
            at(1, EventKind::Tasks { done: 3, total: 9 }),
            at(2, EventKind::Tasks { done: 4, total: 9 }),
            at(3, EventKind::Review { fixed: 3, open: 1 }),
        ],
        4,
    );

    assert_eq!(flow.tasks, Some(Tasks { done: 4, total: 9 }));
    assert_eq!(flow.review, Some(ReviewCount { fixed: 3, open: 1 }));
    assert_eq!(texts(&flow), vec!["started", "review: 3 fixed, 1 open"]);
    assert_eq!(flow.timeline[1].tone, Tone::Attention);
    assert_eq!(flow.changed_at, 0);

    let clean = view(
        &[started(0), at(1, EventKind::Review { fixed: 2, open: 0 })],
        2,
    );
    assert_eq!(clean.timeline[1].tone, Tone::Progress);
}

#[test]
fn a_merge_request_finishes_the_flow_and_every_step() {
    let flow = view(
        &[
            started(0),
            phase(0, Phase::of("review")),
            at(
                6 * MIN,
                EventKind::MergeRequest {
                    url: "https://x/88".into(),
                },
            ),
        ],
        60 * MIN,
    );

    assert_eq!(flow.state, State::Done);
    assert_eq!(flow.phase, Some(Phase::of("merge-request")));
    assert_eq!(flow.merge_request.as_deref(), Some("https://x/88"));
    assert_eq!(
        steps(&flow),
        vec![
            (StepStatus::Done, None),
            (StepStatus::Done, None),
            (StepStatus::Done, Some(6 * MIN)),
            (StepStatus::Done, Some(0)),
        ]
    );
    assert_eq!(flow.timeline.last().unwrap().text, "MR opened");
    assert_eq!(flow.timeline.last().unwrap().tone, Tone::Success);
}

const PR: &str = "https://github.com/a/b/pull/42";

#[test]
fn a_question_after_pr_is_waiting() {
    let events = [
        started(0),
        opened(MIN, PR),
        at(2 * MIN, EventKind::waiting("Ship with the race fix?")),
    ];
    let flow = view(&events, 3 * MIN);
    assert_eq!(flow.state, State::Waiting);
    assert_eq!(flow.blocker.as_deref(), Some("Ship with the race fix?"));
    assert_eq!(flow.merge_request.as_deref(), Some(PR));
    assert!(!flow.is_finished());
    assert!(
        flow.steps
            .iter()
            .all(|step| step.status == StepStatus::Done)
    );
}

#[test]
fn ended_after_pr_is_finished() {
    let flow = view(
        &[started(0), opened(MIN, PR), at(2 * MIN, EventKind::Ended)],
        3 * MIN,
    );
    assert_eq!(flow.state, State::Ended);
    assert!(flow.is_finished());
    assert_eq!(flow.merge_request.as_deref(), Some(PR));
}

#[test]
fn forge_update_does_not_revive_ended_worker() {
    let flow = view(
        &[
            started(0),
            opened(MIN, PR),
            at(2 * MIN, EventKind::Ended),
            merged(3 * MIN, PR),
        ],
        4 * MIN,
    );
    assert_eq!(flow.state, State::Ended);
    assert!(flow.merged);
    assert!(flow.is_finished());
}

fn opened(at_ms: u64, url: &str) -> Event {
    at(at_ms, EventKind::MergeRequest { url: url.into() })
}

fn merged(at_ms: u64, url: &str) -> Event {
    at(at_ms, EventKind::Merged { url: url.into() })
}

fn closed(at_ms: u64, url: &str) -> Event {
    at(at_ms, EventKind::MergeRequestClosed { url: url.into() })
}

#[test]
fn a_merge_seen_at_review_finishes_the_flow_merged() {
    let events = [
        started(0),
        phase(0, Phase::of("review")),
        merged(6 * MIN, PR),
    ];
    let flow = view(&events, 60 * MIN);

    assert_eq!(flow.state, State::Done);
    assert!(flow.merged);
    assert_eq!(flow.phase, Some(Phase::of("merge-request")));
    assert_eq!(flow.merge_request.as_deref(), Some(PR));
    assert_eq!(flow.changed_at, 6 * MIN);
    assert!(
        steps(&flow)
            .iter()
            .all(|(status, _)| *status == StepStatus::Done)
    );
    assert_eq!(flow.timeline.last().unwrap().text, "PR #42 merged");
    assert_eq!(flow.timeline.last().unwrap().tone, Tone::Success);
}

#[test]
fn an_open_or_merged_request_keeps_the_final_phase_after_worker_events() {
    for mr in [opened(2 * MIN, PR), merged(2 * MIN, PR)] {
        let events = [
            started(0),
            phase(MIN, Phase::of("review")),
            mr,
            phase(3 * MIN, Phase::of("review")),
            at(4 * MIN, EventKind::Working),
            at(
                5 * MIN,
                EventKind::Idle {
                    note: "review finished".into(),
                },
            ),
        ];
        let flow = view(&events, 6 * MIN);
        assert_eq!(flow.state, State::Idle);
        assert_eq!(flow.phase, Some(Phase::of("merge-request")));
        assert_eq!(flow.merge_request.as_deref(), Some(PR));
        assert!(
            steps(&flow)
                .iter()
                .all(|(status, _)| *status == StepStatus::Done)
        );
    }
}

#[test]
fn a_custom_workflow_mr_keeps_its_last_phase_after_a_late_skill() {
    let events = [
        at(
            0,
            EventKind::Started {
                name: "csv".into(),
                request: "add csv".into(),
                mode: Mode::Auto,
                phases: Some(vec![Phase::of("build"), Phase::of("release")]),
            },
        ),
        phase(MIN, Phase::of("build")),
        opened(2 * MIN, PR),
        phase(3 * MIN, Phase::of("build")),
    ];
    let flow = view(&events, 4 * MIN);
    assert_eq!(flow.phase, Some(Phase::of("release")));
    assert_eq!(flow.state, State::Done);
}

#[test]
fn a_merge_of_an_open_one_keeps_the_flow_done_and_its_change_time() {
    let events = [started(0), opened(MIN, PR), merged(9 * MIN, PR)];
    let flow = view(&events, 60 * MIN);

    assert_eq!(flow.state, State::Done);
    assert!(flow.merged);
    assert_eq!(flow.changed_at, MIN, "the state did not change");
    assert_eq!(texts(&flow), vec!["started", "MR opened", "PR #42 merged"]);
    assert!(changes(
        "x",
        &events[..2],
        &EventKind::Merged { url: PR.into() },
        10 * MIN
    ));
    assert!(!changes(
        "x",
        &events,
        &EventKind::Merged { url: PR.into() },
        10 * MIN
    ));
}

#[test]
fn closing_the_open_one_takes_the_flow_back_to_idle_at_its_last_phase() {
    let events = [
        started(0),
        phase(0, Phase::of("review")),
        opened(5 * MIN, PR),
        closed(8 * MIN, PR),
    ];
    let flow = view(&events, 10 * MIN);

    assert_eq!(flow.state, State::Idle);
    assert!(!flow.is_finished());
    assert_eq!(flow.merge_request, None);
    assert_eq!(flow.phase, Some(Phase::of("merge-request")));
    assert_eq!(steps(&flow)[3], (StepStatus::Current, Some(5 * MIN)));
    assert_eq!(flow.changed_at, 8 * MIN);
    assert_eq!(flow.timeline.last().unwrap().text, "PR #42 closed");
    assert_eq!(flow.timeline.last().unwrap().tone, Tone::Attention);
}

#[test]
fn closing_another_or_a_merged_one_changes_nothing() {
    let other = view(
        &[
            started(0),
            opened(MIN, PR),
            closed(2 * MIN, "https://github.com/a/b/pull/41"),
        ],
        3 * MIN,
    );
    assert_eq!(other.state, State::Done);
    assert_eq!(other.merge_request.as_deref(), Some(PR));
    assert_eq!(texts(&other), vec!["started", "MR opened"]);

    let after_merge = view(&[started(0), merged(MIN, PR), closed(2 * MIN, PR)], 3 * MIN);
    assert_eq!(after_merge.state, State::Done);
    assert!(after_merge.merged);
}

#[test]
fn closing_a_pr_after_the_worker_ended_keeps_the_flow_ended() {
    let events = [
        started(0),
        phase(0, Phase::of("implement")),
        at(MIN, EventKind::Ended),
        opened(2 * MIN, PR),
        closed(3 * MIN, PR),
    ];
    let before_close = view(&events[..4], 10 * MIN);
    let flow = view(&events, 10 * MIN);
    assert_eq!(flow.state, State::Ended);
    assert!(flow.is_finished());
    assert_eq!(flow.merge_request, None);
    assert_eq!(flow.changed_at, 3 * MIN);
    assert_eq!(flow.timeline.last().unwrap().text, "PR #42 closed");
    assert_eq!(flow.steps[1].duration_ms, before_close.steps[1].duration_ms);
    assert_eq!(flow.steps[3].duration_ms, before_close.steps[3].duration_ms);
}

#[test]
fn closing_a_pr_opened_before_the_worker_ended_keeps_the_flow_ended() {
    let flow = view(
        &[
            started(0),
            opened(MIN, PR),
            at(2 * MIN, EventKind::Ended),
            closed(3 * MIN, PR),
        ],
        4 * MIN,
    );

    assert_eq!(flow.state, State::Ended);
    assert!(flow.is_finished());
    assert_eq!(flow.merge_request, None);
    assert_eq!(flow.timeline.last().unwrap().text, "PR #42 closed");
}

#[test]
fn ended_stops_the_clock_and_finishes_a_pr_open_flow() {
    let ended = view(
        &[
            started(0),
            phase(0, Phase::of("implement")),
            at(5 * MIN, EventKind::Ended),
        ],
        60 * MIN,
    );
    assert_eq!(ended.state, State::Ended);
    assert_eq!(steps(&ended)[1], (StepStatus::Current, Some(5 * MIN)));
    assert_eq!(texts(&ended), vec!["started", "phase: implement", "ended"]);

    let done = view(
        &[
            started(0),
            at(
                1,
                EventKind::MergeRequest {
                    url: "https://x/88".into(),
                },
            ),
            at(2, EventKind::Ended),
        ],
        3,
    );
    assert_eq!(done.state, State::Ended);
}

#[test]
fn a_silent_heartbeat_means_not_responding_unless_finished() {
    let running = [started(0), phase(0, Phase::of("implement"))];
    let beat = Some(10 * MIN);

    assert_eq!(
        fold("a", &running, beat, 10 * MIN + SILENT_AFTER_MS)
            .unwrap()
            .state,
        State::Working
    );
    assert_eq!(
        fold("a", &running, beat, 10 * MIN + SILENT_AFTER_MS + 1)
            .unwrap()
            .state,
        State::NotResponding
    );
    assert_eq!(
        fold("a", &running, None, 99 * MIN).unwrap().state,
        State::Working
    );

    let ended = [started(0), at(1, EventKind::Ended)];
    assert_eq!(
        fold("a", &ended, beat, 99 * MIN).unwrap().state,
        State::Ended
    );
}

fn flow_in(name: &str, state: State, changed_at: u64) -> FlowView {
    let mut flow = view(&[started(0)], 1);
    flow.id = format!("{name}-{changed_at}");
    flow.name = name.to_string();
    flow.state = state;
    flow.changed_at = changed_at;
    flow
}

#[test]
fn the_board_puts_flows_needing_you_first_then_ready_then_running_then_finished() {
    let board = board(
        vec![
            flow_in("idle", State::Idle, 1),
            flow_in("done-old", State::Done, 1),
            flow_in("working-old", State::Working, 2),
            flow_in("blocked", State::Blocked, 3),
            flow_in("done-new", State::Done, 4),
            flow_in("working-new", State::Delegated, 5),
            flow_in("silent", State::NotResponding, 6),
            flow_in("ended", State::Ended, 7),
            flow_in("waiting", State::Waiting, 0),
        ],
        100,
    );

    let names: Vec<&str> = board.flows.iter().map(|flow| flow.name.as_str()).collect();
    assert_eq!(
        names,
        vec![
            "silent",
            "blocked",
            "waiting",
            "idle",
            "working-new",
            "working-old",
            "ended",
            "done-new",
            "done-old"
        ]
    );
    assert_eq!(board.version, VIEW_VERSION);
    assert_eq!(board.now, 100);
    assert_eq!(board.count, 9);
    assert_eq!(board.needs_you, 3);
}

#[test]
fn resolves_an_id_then_the_unfinished_flow_then_the_newest_finished_one() {
    let flows = vec![
        flow_in("shop-221", State::Done, 1),
        flow_in("shop-221", State::Done, 5),
        flow_in("shop-230", State::Done, 2),
        flow_in("shop-230", State::Working, 1),
    ];

    assert_eq!(resolve(&flows, "shop-221-1").unwrap().changed_at, 1);
    assert_eq!(resolve(&flows, "shop-221").unwrap().changed_at, 5);
    assert_eq!(resolve(&flows, "shop-230").unwrap().state, State::Done);
    assert_eq!(resolve(&flows, "shop-999"), None);
}

fn workspace(slot: u8, window: Option<&str>) -> EventKind {
    EventKind::Workspace {
        path: "/repo/.saaio/worktrees/shop-221".into(),
        branch: Some("flow/shop-221".into()),
        slot: Some(slot),
        window: window.map(String::from),
        agent: "claude".into(),
    }
}

#[test]
fn keeps_the_last_workspace_without_changing_the_state() {
    let flow = view(
        &[
            started(0),
            at(1, workspace(1, None)),
            at(2, workspace(1, Some("@14"))),
        ],
        3,
    );

    assert_eq!(
        flow.workspace.as_ref().and_then(|ws| ws.window.as_deref()),
        Some("@14")
    );
    assert_eq!(flow.workspace.as_ref().and_then(|ws| ws.slot), Some(1));
    assert_eq!(flow.state, State::Working);
    assert_eq!(flow.changed_at, 0);
    assert_eq!(texts(&flow), vec!["started"]);
    assert_eq!(view(&[started(0)], 1).workspace, None);
}

#[test]
fn ignores_a_second_started_event() {
    let flow = view(
        &[
            started(0),
            Event::new(
                5,
                EventKind::Started {
                    name: "other".into(),
                    request: "other".into(),
                    mode: Mode::Guided,
                    phases: None,
                },
            ),
        ],
        6,
    );

    assert_eq!(flow.name, "shop-221");
    assert_eq!(texts(&flow), vec!["started"]);
}

fn started_with(at: u64, phases: &[&str]) -> Event {
    Event::new(
        at,
        EventKind::Started {
            name: "shop-221".into(),
            request: "SHOP-221 add CSV export".into(),
            mode: Mode::Auto,
            phases: Some(phases.iter().copied().map(Phase::of).collect()),
        },
    )
}

#[test]
fn a_flow_walks_the_phases_it_started_with() {
    let flow = view(
        &[
            started_with(0, &["spec", "build", "ship"]),
            phase(MIN, Phase::of("build")),
        ],
        3 * MIN,
    );

    assert_eq!(
        flow.steps
            .iter()
            .map(|step| step.phase.name())
            .collect::<Vec<_>>(),
        ["spec", "build", "ship"]
    );
    assert_eq!(flow.phase, Some(Phase::of("build")));
    assert_eq!(
        steps(&flow),
        vec![
            (StepStatus::Done, None),
            (StepStatus::Current, Some(2 * MIN)),
            (StepStatus::Open, None),
        ]
    );
}

#[test]
fn a_log_without_its_phases_walks_the_builtin_four() {
    let flow = view(
        &[
            started(0),
            phase(MIN, Phase::of("design")),
            phase(2 * MIN, Phase::of("implement")),
        ],
        3 * MIN,
    );

    assert_eq!(flow.phase, Some(Phase::of("implement")));
    assert_eq!(texts(&flow), ["started", "phase: implement"]);
    assert_eq!(
        flow.steps
            .iter()
            .map(|step| step.phase.name())
            .collect::<Vec<_>>(),
        ["plan", "implement", "review", "merge-request"]
    );
}

#[test]
fn a_phase_the_flow_does_not_have_changes_nothing() {
    let events = [
        started_with(0, &["spec", "build"]),
        phase(MIN, Phase::of("spec")),
    ];

    let flow = view(
        &[
            events[0].clone(),
            events[1].clone(),
            phase(2 * MIN, Phase::of("review")),
        ],
        3 * MIN,
    );

    assert_eq!(flow.phase, Some(Phase::of("spec")));
    assert_eq!(texts(&flow), ["started", "phase: spec"]);
    assert!(!changes(
        "x",
        &events,
        &EventKind::Phase {
            phase: Phase::of("review")
        },
        3 * MIN
    ));
}

#[test]
fn a_merge_request_enters_the_last_phase_whatever_its_name() {
    let flow = view(
        &[
            started_with(0, &["spec", "build", "ship"]),
            phase(0, Phase::of("build")),
            at(
                MIN,
                EventKind::MergeRequest {
                    url: "https://x/88".into(),
                },
            ),
        ],
        2 * MIN,
    );

    assert_eq!(flow.phase, Some(Phase::of("ship")));
    assert!(
        flow.steps
            .iter()
            .all(|step| step.status == StepStatus::Done)
    );
}

#[test]
fn a_log_with_an_empty_phase_list_folds_without_steps() {
    let flow = view(
        &[
            started_with(0, &[]),
            phase(0, Phase::of("build")),
            at(
                MIN,
                EventKind::MergeRequest {
                    url: "https://x/88".into(),
                },
            ),
        ],
        2 * MIN,
    );

    assert_eq!(flow.state, State::Done);
    assert_eq!(flow.phase, None);
    assert!(flow.steps.is_empty());
}

fn services(at_ms: u64, status: ServiceStatus, reason: Option<&str>) -> Event {
    at(
        at_ms,
        EventKind::Services {
            status,
            reason: reason.map(str::to_string),
        },
    )
}

#[test]
fn services_show_where_they_stand_without_moving_the_flow() {
    let idle = at(
        MIN,
        EventKind::Idle {
            note: "Plan ready.".into(),
        },
    );
    let events = [
        started(0),
        idle,
        services(6 * MIN, ServiceStatus::Pausing, None),
        services(6 * MIN + 1, ServiceStatus::Paused, None),
    ];

    let flow = view(&events, 7 * MIN);

    assert_eq!(flow.state, State::Idle);
    assert_eq!(flow.changed_at, MIN, "a pause is no change of the flow");
    assert_eq!(
        flow.services,
        Some(Services {
            status: ServiceStatus::Paused,
            since: 6 * MIN + 1,
            reason: None,
        })
    );
    assert_eq!(texts(&flow), vec!["started", "idle: Plan ready."]);
    assert_eq!(view(&events[..2], 7 * MIN).services, None);
}

#[test]
fn a_failed_pause_or_resume_is_a_problem_in_the_timeline() {
    let events = [
        started(0),
        services(
            MIN,
            ServiceStatus::PauseFailed,
            Some("docker is not running"),
        ),
        services(
            2 * MIN,
            ServiceStatus::ResumeFailed,
            Some("port 8003 is taken"),
        ),
    ];

    let flow = view(&events, 3 * MIN);

    let failures: Vec<_> = flow.timeline[1..]
        .iter()
        .map(|entry| (entry.text.as_str(), entry.tone))
        .collect();
    assert_eq!(
        failures,
        vec![
            ("pause failed: docker is not running", Tone::Problem),
            ("resume failed: port 8003 is taken", Tone::Problem),
        ]
    );
    let services = flow.services.unwrap();
    assert_eq!(services.status, ServiceStatus::ResumeFailed);
    assert_eq!(services.reason.as_deref(), Some("port 8003 is taken"));
    assert_eq!(flow.changed_at, 0);
}

#[test]
fn services_still_fold_after_the_worker_ended() {
    let events = [
        started(0),
        at(MIN, EventKind::Ended),
        services(7 * MIN, ServiceStatus::Paused, None),
    ];

    let flow = view(&events, 8 * MIN);

    assert_eq!(flow.state, State::Ended);
    assert_eq!(flow.changed_at, MIN);
    assert_eq!(flow.services.unwrap().status, ServiceStatus::Paused);
}
