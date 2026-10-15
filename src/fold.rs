use crate::event::{Event, EventKind, Mode, Phase};
use crate::store::FlowLog;
use crate::view::{
    Board, FlowView, ReviewCount, State, Step, StepStatus, Tasks, TimelineEntry, Tone,
    VIEW_VERSION, Workspace,
};

/// A flow with a heartbeat older than this, not done or ended, is not responding.
pub const SILENT_AFTER_MS: u64 = 90_000;

/// One flow's view at `now`; None when the log has no Started event to name it.
pub fn fold(id: &str, events: &[Event], last_beat: Option<u64>, now: u64) -> Option<FlowView> {
    let start = events
        .iter()
        .position(|event| matches!(event.kind, EventKind::Started { .. }))?;
    let EventKind::Started {
        name,
        request,
        mode,
    } = &events[start].kind
    else {
        unreachable!("position found a Started event");
    };
    let mut folding = Folding::new(id, name, request, *mode, events[start].at);
    for event in &events[start + 1..] {
        folding.apply(event);
    }
    let mut view = folding.finish(now);
    let is_silent = last_beat.is_some_and(|beat| now.saturating_sub(beat) > SILENT_AFTER_MS);
    if is_silent && !view.is_finished() {
        view.state = State::NotResponding;
    }
    Some(view)
}

/// True when appending `kind` changes what the flow shows: its state, phase, blocker, delegate,
/// tasks, review, merge request, decisions or answer; every message does. Adapters skip the rest, so a log does not grow by a
/// line for every tool call.
pub fn changes(id: &str, events: &[Event], kind: &EventKind, now: u64) -> bool {
    if matches!(kind, EventKind::Message { .. }) {
        return true;
    }
    let Some(before) = fold(id, events, None, now) else {
        return true;
    };
    let mut after_events = events.to_vec();
    after_events.push(Event::new(now, kind.clone()));
    let Some(after) = fold(id, &after_events, None, now) else {
        return true;
    };
    let shown = |view: &FlowView| {
        (
            view.state,
            view.phase,
            view.blocker.clone(),
            view.delegate.clone(),
            view.tasks,
            view.review,
            view.merge_request.clone(),
            view.decisions.clone(),
            view.timeline.last().map(|entry| entry.text.clone()),
            view.answer.clone(),
        )
    };
    shown(&before) != shown(&after)
}

/// Every log that names its flow, folded at `now`.
pub fn views(logs: Vec<FlowLog>, now: u64) -> Vec<FlowView> {
    logs.into_iter()
        .filter_map(|log| fold(&log.id, &log.events, log.beat, now))
        .collect()
}

/// Sorted: needs you, running, finished; newest change first within each group.
pub fn board(mut flows: Vec<FlowView>, now: u64) -> Board {
    flows.sort_by_key(|flow| (group(flow), std::cmp::Reverse(flow.changed_at)));
    Board {
        version: VIEW_VERSION,
        now,
        count: flows.len(),
        needs_you: flows.iter().filter(|flow| flow.needs_you()).count(),
        flows,
    }
}

/// The flow a name or id means: an exact id, else the unfinished flow of that name,
/// else the most recent finished one of that name.
pub fn resolve<'a>(flows: &'a [FlowView], name_or_id: &str) -> Option<&'a FlowView> {
    let named = || flows.iter().filter(|flow| flow.name == name_or_id);
    flows
        .iter()
        .find(|flow| flow.id == name_or_id)
        .or_else(|| {
            named()
                .filter(|flow| !flow.is_finished())
                .max_by_key(|flow| flow.changed_at)
        })
        .or_else(|| named().max_by_key(|flow| flow.changed_at))
}

fn group(flow: &FlowView) -> u8 {
    if flow.needs_you() {
        0
    } else if flow.is_finished() {
        2
    } else {
        1
    }
}

struct Folding {
    view: FlowView,
    durations: [Option<u64>; 4],
    entered: [bool; 4],
    phase_since: u64,
    stopped_at: Option<u64>,
}

impl Folding {
    fn new(id: &str, name: &str, request: &str, mode: Mode, at: u64) -> Folding {
        let view = FlowView {
            id: id.to_string(),
            name: name.to_string(),
            request: request.to_string(),
            mode,
            phase: None,
            state: State::Working,
            blocker: None,
            delegate: None,
            tasks: None,
            review: None,
            merge_request: None,
            decisions: Vec::new(),
            steps: Vec::new(),
            timeline: vec![entry(at, "started".to_string(), Tone::Neutral)],
            started_at: at,
            changed_at: at,
            workspace: None,
            answer: None,
        };
        Folding {
            view,
            durations: [None; 4],
            entered: [false; 4],
            phase_since: at,
            stopped_at: None,
        }
    }

    fn apply(&mut self, event: &Event) {
        let before = (self.view.state, self.view.phase, self.view.blocker.clone());
        let at = event.at;
        match &event.kind {
            EventKind::Started { .. } => {}
            EventKind::Phase { phase } => {
                if self.view.phase != Some(*phase) {
                    self.enter(*phase, at);
                    // A delegate can start the phase itself, as a review in a fresh agent does.
                    if self.view.state != State::Delegated {
                        self.view.state = State::Working;
                        self.view.delegate = None;
                    }
                    self.view.blocker = None;
                    self.note(at, format!("phase: {}", phase.name()), Tone::Progress);
                }
            }
            EventKind::Working => {
                if matches!(self.view.state, State::Waiting | State::Blocked) {
                    self.note(at, "answered".to_string(), Tone::Progress);
                }
                self.view.state = State::Working;
                self.view.blocker = None;
                self.view.delegate = None;
            }
            EventKind::Delegated { description } => {
                self.view.state = State::Delegated;
                self.view.delegate = Some(description.clone());
            }
            EventKind::Waiting { question } => {
                self.stop_for(State::Waiting, question, at, "waiting", Tone::Attention);
            }
            EventKind::Idle { note } => {
                let text = format!("idle: {note}");
                let same = self.view.state == State::Idle
                    && self.view.timeline.last().map(|entry| &entry.text) == Some(&text);
                if !same {
                    self.view.changed_at = at;
                    self.view.state = State::Idle;
                    self.view.blocker = None;
                    self.view.delegate = None;
                    self.note(at, text, Tone::Neutral);
                }
            }
            EventKind::Blocked { reason } => {
                self.stop_for(State::Blocked, reason, at, "blocked", Tone::Problem);
            }
            EventKind::Decision { text } => {
                if !self.view.decisions.contains(text) {
                    self.view.decisions.push(text.clone());
                    self.note(at, format!("decided: {text}"), Tone::Neutral);
                }
            }
            EventKind::Tasks { done, total } => {
                self.view.tasks = Some(Tasks {
                    done: *done,
                    total: *total,
                });
            }
            EventKind::Review { fixed, open } => {
                self.view.review = Some(ReviewCount {
                    fixed: *fixed,
                    open: *open,
                });
                let tone = if *open > 0 {
                    Tone::Attention
                } else {
                    Tone::Progress
                };
                self.note(at, format!("review: {fixed} fixed, {open} open"), tone);
            }
            EventKind::MergeRequest { url } => {
                if self.view.phase != Some(Phase::MergeRequest) {
                    self.enter(Phase::MergeRequest, at);
                }
                self.view.state = State::Done;
                self.view.blocker = None;
                self.view.delegate = None;
                self.view.merge_request = Some(url.clone());
                self.stopped_at.get_or_insert(at);
                self.note(at, "MR opened".to_string(), Tone::Success);
            }
            EventKind::Workspace {
                path,
                branch,
                slot,
                window,
                agent,
            } => {
                self.view.workspace = Some(Workspace {
                    path: path.clone(),
                    branch: branch.clone(),
                    slot: *slot,
                    window: window.clone(),
                    agent: agent.clone(),
                });
            }
            EventKind::Message { text } => {
                self.note(at, format!("master: {text}"), Tone::Neutral);
            }
            EventKind::Answer { text } => {
                self.view.answer = Some(text.clone());
            }
            EventKind::Ended => {
                if self.view.state != State::Done {
                    self.view.state = State::Ended;
                }
                self.view.delegate = None;
                self.stopped_at.get_or_insert(at);
                self.note(at, "ended".to_string(), Tone::Neutral);
            }
        }
        if before != (self.view.state, self.view.phase, self.view.blocker.clone()) {
            self.view.changed_at = at;
        }
    }

    fn enter(&mut self, phase: Phase, at: u64) {
        self.close_phase(at);
        self.view.phase = Some(phase);
        self.entered[index(phase)] = true;
        self.phase_since = at;
    }

    fn close_phase(&mut self, until: u64) {
        if let Some(current) = self.view.phase {
            let spent = until.saturating_sub(self.phase_since);
            let total = &mut self.durations[index(current)];
            *total = Some(total.unwrap_or(0) + spent);
        }
    }

    fn stop_for(&mut self, state: State, text: &str, at: u64, label: &str, tone: Tone) {
        if self.view.state == state && self.view.blocker.as_deref() == Some(text) {
            return;
        }
        self.view.state = state;
        self.view.blocker = Some(text.to_string());
        self.note(at, format!("{label}: {text}"), tone);
    }

    fn note(&mut self, at: u64, text: String, tone: Tone) {
        self.view.timeline.push(entry(at, text, tone));
    }

    fn finish(mut self, now: u64) -> FlowView {
        self.close_phase(self.stopped_at.unwrap_or(now));
        let current = self.view.phase.map(index);
        let is_done = self.view.state == State::Done;
        self.view.steps = Phase::ALL
            .iter()
            .enumerate()
            .map(|(position, phase)| {
                let status = if is_done {
                    StepStatus::Done
                } else if current == Some(position) {
                    StepStatus::Current
                } else if self.entered[position]
                    || current.is_some_and(|current| position < current)
                {
                    StepStatus::Done
                } else {
                    StepStatus::Open
                };
                Step {
                    phase: *phase,
                    status,
                    duration_ms: self.durations[position],
                }
            })
            .collect();
        self.view
    }
}

fn index(phase: Phase) -> usize {
    Phase::ALL
        .iter()
        .position(|candidate| *candidate == phase)
        .expect("every phase is in ALL")
}

fn entry(at: u64, text: String, tone: Tone) -> TimelineEntry {
    TimelineEntry { at, text, tone }
}

#[cfg(test)]
mod tests {
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
                    phase: Phase::Review,
                },
            ),
        ];

        let flow = fold("x", &events, None, 3).unwrap();

        assert_eq!(flow.state, State::Delegated);
        assert_eq!(flow.phase, Some(Phase::Review));
        assert_eq!(flow.delegate.as_deref(), Some("reviewer"));
    }

    fn started(at: u64) -> Event {
        Event::new(
            at,
            EventKind::Started {
                name: "shop-221".into(),
                request: "SHOP-221 add CSV export".into(),
                mode: Mode::Auto,
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
                phase: Phase::Design
            },
            2
        ));
        assert!(
            changes("id", &[], &EventKind::Working, 2),
            "a log without a start"
        );
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
            flow.steps.iter().map(|step| step.phase).collect::<Vec<_>>(),
            Phase::ALL
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
                phase(0, Phase::Design),
                phase(9 * MIN, Phase::Implement),
            ],
            40 * MIN,
        );

        assert_eq!(flow.phase, Some(Phase::Implement));
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
            vec!["started", "phase: design", "phase: implement"]
        );
        assert_eq!(flow.timeline[1].tone, Tone::Progress);
        assert_eq!(flow.changed_at, 9 * MIN);
    }

    #[test]
    fn going_back_a_phase_adds_to_its_time_and_keeps_later_steps_done() {
        let flow = view(
            &[
                started(0),
                phase(0, Phase::Implement),
                phase(10 * MIN, Phase::Review),
                phase(15 * MIN, Phase::Implement),
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
        let once = view(&[started(0), phase(1, Phase::Design)], 10);
        let twice = view(
            &[started(0), phase(1, Phase::Design), phase(5, Phase::Design)],
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
                phase(3, Phase::Review),
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
                phase(0, Phase::Review),
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
        assert_eq!(flow.phase, Some(Phase::MergeRequest));
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
                phase(0, Phase::Implement),
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
        let running = [started(0), phase(0, Phase::Implement)];
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
    fn the_board_puts_flows_needing_you_first_then_running_then_finished() {
        let board = board(
            vec![
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
                "working-new",
                "working-old",
                "ended",
                "done-new",
                "done-old"
            ]
        );
        assert_eq!(board.version, VIEW_VERSION);
        assert_eq!(board.now, 100);
        assert_eq!(board.count, 8);
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
            branch: "flow/shop-221".into(),
            slot,
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
        assert_eq!(flow.workspace.as_ref().map(|ws| ws.slot), Some(1));
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
                    },
                ),
            ],
            6,
        );

        assert_eq!(flow.name, "shop-221");
        assert_eq!(texts(&flow), vec!["started"]);
    }
}
