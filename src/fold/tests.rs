use super::*;

const MIN: u64 = 60_000;

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
        at(
            1,
            EventKind::Waiting {
                question: "Which delimiter?".into(),
            },
        ),
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
    Event::new(
        at,
        EventKind::Started {
            name: "shop-221".into(),
            request: "SHOP-221 add CSV export".into(),
            mode: Mode::Auto,
            phases: None,
        },
    )
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
    let waiting = Event::new(
        1,
        EventKind::Waiting {
            question: "Which?".into(),
        },
    );
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
            at(
                1,
                EventKind::Waiting {
                    question: "Approve the plan?".into(),
                },
            ),
            at(
                2,
                EventKind::Waiting {
                    question: "Approve the plan?".into(),
                },
            ),
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
            at(
                1,
                EventKind::Waiting {
                    question: "Which approach?".into(),
                },
            ),
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

#[test]
fn ended_stops_the_clock_but_keeps_a_done_flow_done() {
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
    assert_eq!(done.state, State::Done);
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
    assert_eq!(resolve(&flows, "shop-230").unwrap().state, State::Working);
    assert_eq!(resolve(&flows, "shop-999"), None);
}

fn workspace(slot: u8, window: Option<&str>) -> EventKind {
    EventKind::Workspace {
        path: "/repo/.flow/worktrees/shop-221".into(),
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
