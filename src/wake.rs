//! What is news for a session (pure), and how it reads: `for_worker`, `for_master`

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::event::{Event, EventKind, Mode};
use crate::id::FlowId;
use crate::text::{LIST_TEXT_LIMIT, truncated};
use crate::view::{FlowView, State};

/// What the master was last woken for, by flow id.
pub type Seen = BTreeMap<FlowId, Mark>;

/// A flow as the master last saw it: its state and when that changed, and when the failed setup
/// it was told about failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mark {
    pub state: State,
    pub changed_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub setup_failed: Option<u64>,
}

/// What a worker hears: the master's message, saaio's word that its base merged, or that its
/// setup failed for `reason`, its whole output in `log`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Note {
    Master(String),
    BaseMerged { base: String, onto: String },
    SetupFailed { reason: String, log: String },
}

/// The notes in a flow's log after the first `delivered` of them (Message, BaseMerged and failed
/// Setup events); a flow that merges nothing has nothing to rebase, so its base moves without a
/// note.
pub fn undelivered(events: &[Event], delivered: usize) -> Vec<Note> {
    let merges = events
        .iter()
        .all(|event| !matches!(event.kind, EventKind::Started { merges: false, .. }));
    events
        .iter()
        .filter_map(|event| match &event.kind {
            EventKind::Message { text } => Some(Note::Master(text.clone())),
            EventKind::BaseMerged { base, onto } if merges => Some(Note::BaseMerged {
                base: base.clone(),
                onto: onto.clone(),
            }),
            EventKind::Setup {
                failed: Some(reason),
                log,
            } => Some(Note::SetupFailed {
                reason: reason.clone(),
                log: log.clone(),
            }),
            _ => None,
        })
        .skip(delivered)
        .collect()
}

/// The lines queued in a flow's log after the first `typed` of them.
pub fn untyped(events: &[Event], typed: usize) -> Vec<&str> {
    events
        .iter()
        .filter_map(|event| match &event.kind {
            EventKind::Typed { text } => Some(text.as_str()),
            _ => None,
        })
        .skip(typed)
        .collect()
}

/// True when the master sent a message after `flow` opened the question dialog it still waits on.
pub fn dialog_answered(flow: &FlowView, events: &[Event]) -> bool {
    if flow.questions.is_empty() {
        return false;
    }
    events
        .iter()
        .rev()
        .take_while(|event| !matches!(event.kind, EventKind::Waiting { .. }))
        .any(|event| matches!(event.kind, EventKind::Message { .. }))
}

/// Flows whose state needs the master and that changed since it last saw them, or whose setup
/// failed since, whatever their state; oldest first; never a plain session, which is the user's
/// own.
pub fn changed_flows<'a>(flows: &'a [FlowView], seen: &Seen) -> Vec<&'a FlowView> {
    let mut changed: Vec<&FlowView> = flows
        .iter()
        .filter(|flow| !flow.is_plain())
        .filter(|flow| {
            let last = seen.get(&flow.id);
            let moved = last.map(|mark| (mark.state, mark.changed_at))
                != Some((flow.state, flow.changed_at));
            let failed = setup_failed(flow);
            (needs_master(flow.state) && moved)
                || (failed.is_some() && last.and_then(|mark| mark.setup_failed) != failed)
        })
        .collect();
    changed.sort_by_key(|flow| flow.changed_at);
    changed
}

/// Seen after the master was told about `flows`.
pub fn mark_seen(seen: &mut Seen, flows: &[&FlowView]) {
    for flow in flows {
        seen.insert(flow.id, mark(flow));
    }
}

/// The Seen to start from when none is stored: every finished flow, so only unfinished ones are news.
pub fn first_seen(flows: &[FlowView]) -> Seen {
    flows
        .iter()
        .filter(|flow| flow.is_finished())
        .map(|flow| (flow.id, mark(flow)))
        .collect()
}

/// A worker stopped and the master may have something to do: answer, relay, report.
fn needs_master(state: State) -> bool {
    matches!(
        state,
        State::Waiting | State::Blocked | State::Idle | State::Done | State::NotResponding
    )
}

fn mark(flow: &FlowView) -> Mark {
    Mark {
        state: flow.state,
        changed_at: flow.changed_at,
        setup_failed: setup_failed(flow),
    }
}

/// When the flow's last setup run failed; None while it passed or never ran.
fn setup_failed(flow: &FlowView) -> Option<u64> {
    let run = flow.setup.as_ref()?;
    run.failed.as_ref().map(|_| run.at)
}

/// What a worker reads when saaio wakes it: each of the master's messages, indented, and what to
/// do once its base merged.
pub fn for_worker(notes: &[Note]) -> String {
    notes
        .iter()
        .flat_map(|note| match note {
            Note::Master(message) => std::iter::once("message from the saaio master:".to_string())
                .chain(message.lines().map(|line| format!("  {line}")))
                .collect::<Vec<_>>(),
            Note::BaseMerged { base, onto } => vec![format!(
                "news from saaio: {base}, the branch this flow stands on, was merged into {onto}. \
Rebase your own commits onto it (`git fetch`, then `git rebase --onto origin/{onto} {base}`, or \
`origin/{base}` once that branch is gone), point your merge request at {onto} and push with \
`--force-with-lease`."
            )],
            Note::SetupFailed { reason, log } => vec![format!(
                "news from saaio: .saaio/setup failed for this worktree: {reason}. Its whole output \
is in {log}. Read it. When the cause is in this repository, fix it and run `saaio setup`; when it \
needs something on this machine, such as a missing PHP extension, end your turn saying what is \
missing."
            )],
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// What the master reads when saaio wakes it: changed[n]{name,state,mode,says}, who answers by the
/// flows' modes, and where a question dialog's options are.
pub fn for_master(flows: &[&FlowView]) -> String {
    #[derive(Serialize)]
    struct Changed {
        name: String,
        state: &'static str,
        mode: &'static str,
        says: String,
    }
    #[derive(Serialize)]
    struct News {
        changed: Vec<Changed>,
        help: Vec<&'static str>,
    }
    let has = |mode: Mode| flows.iter().any(|flow| flow.mode == mode);
    let mut help = Vec::new();
    if has(Mode::Auto) {
        help.push("auto: answer what the code, the plan or the user's decisions settle with `saaio send <name> \"<answer>\"`, then tell the user what you answered; else ask the user");
    }
    if has(Mode::Guided) {
        help.push(
            "guided: tell the user what it says or asks; send their answer with `saaio send`",
        );
    }
    if flows.iter().any(|flow| !flow.questions.is_empty()) {
        help.push("`saaio status <name>` lists a question dialog's options");
    }
    help.push("`saaio status <name> --full` shows its whole last answer");
    let says = |flow: &FlowView| match (&flow.blocker, flow.setup_line(false)) {
        (Some(blocker), _) if flow.setup_problem().as_ref() != Some(blocker) => {
            truncated(blocker, LIST_TEXT_LIMIT, false)
        }
        (_, Some(setup)) => setup,
        (blocker, None) => {
            let last = flow.timeline.last().map_or("", |entry| entry.text.as_str());
            let text = blocker
                .as_deref()
                .unwrap_or_else(|| last.strip_prefix("idle: ").unwrap_or(last));
            truncated(text, LIST_TEXT_LIMIT, false)
        }
    };
    serde_toon::to_string(&News {
        changed: flows
            .iter()
            .map(|flow| Changed {
                name: flow.name.clone(),
                state: flow.state.name(),
                mode: flow.mode.name(),
                says: says(flow),
            })
            .collect(),
        help,
    })
    .expect("news always encodes")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::Question;
    use crate::fixtures;
    use crate::fold;

    fn flow(id: &str, kinds: Vec<(u64, EventKind)>) -> FlowView {
        let events = kinds
            .into_iter()
            .map(|(at, kind)| Event::new(at, kind))
            .collect();
        fixtures::flow(id, events, 100)
    }

    fn message(text: &str) -> EventKind {
        EventKind::Message { text: text.into() }
    }

    fn dialog() -> EventKind {
        EventKind::Waiting {
            question: "Which color?".into(),
            questions: vec![Question::of("Which color?", &["Red", "Blue"], false)],
            permission: false,
        }
    }

    /// Whether the master answered the dialog of a flow whose log holds `kinds`.
    fn answered(kinds: Vec<EventKind>) -> bool {
        let mut events = vec![fixtures::started("ask", "ask")];
        events.extend(kinds.into_iter().map(|kind| Event::new(1, kind)));
        let flow = fold::fold(fixtures::flow_id("ask"), &events, None, 2).unwrap();
        dialog_answered(&flow, &events)
    }

    #[test]
    fn a_message_after_an_open_dialog_answers_it() {
        assert!(answered(vec![dialog(), message("Red")]));
        assert!(!answered(vec![dialog()]), "no message yet");
        assert!(
            !answered(vec![message("Red"), dialog()]),
            "sent before the dialog opened"
        );
        assert!(
            !answered(vec![dialog(), message("Red"), EventKind::Working]),
            "the dialog closed"
        );
        assert!(
            !answered(vec![EventKind::waiting("Ship it?"), message("Yes")]),
            "a question asked in words"
        );
        assert!(
            answered(vec![message("old"), dialog(), message("Red")]),
            "the message after it counts"
        );
    }

    #[test]
    fn hands_over_only_the_messages_not_yet_delivered() {
        let events: Vec<Event> = [message("one"), EventKind::Working, message("two")]
            .into_iter()
            .map(|kind| Event::new(1, kind))
            .collect();

        assert_eq!(undelivered(&events, 0), [master("one"), master("two")]);
        assert_eq!(undelivered(&events, 1), [master("two")]);
        assert_eq!(undelivered(&events, 2), []);
        assert_eq!(undelivered(&events, 5), []);
    }

    fn master(text: &str) -> Note {
        Note::Master(text.into())
    }

    #[test]
    fn hands_over_a_merged_base_in_order_with_the_messages() {
        let merged = EventKind::BaseMerged {
            base: "flow/shop-1".into(),
            onto: "main".into(),
        };
        let events: Vec<Event> = [message("one"), merged, message("two")]
            .into_iter()
            .map(|kind| Event::new(1, kind))
            .collect();
        let note = Note::BaseMerged {
            base: "flow/shop-1".into(),
            onto: "main".into(),
        };

        assert_eq!(
            undelivered(&events, 0),
            [master("one"), note.clone(), master("two")]
        );
        assert_eq!(undelivered(&events, 2), [master("two")], "both kinds count");
        assert_eq!(
            for_worker(&[note]),
            "news from saaio: flow/shop-1, the branch this flow stands on, was merged into main. \
Rebase your own commits onto it (`git fetch`, then `git rebase --onto origin/main flow/shop-1`, \
or `origin/flow/shop-1` once that branch is gone), point your merge request at main and push \
with `--force-with-lease`."
        );
    }

    #[test]
    fn a_flow_that_merges_nothing_hears_no_merged_base() {
        let research = EventKind::Started {
            name: "why".into(),
            request: "why".into(),
            mode: crate::event::Mode::Guided,
            phases: None,
            kind: Some("research".into()),
            workflow: None,
            prompt: None,
            merges: false,
        };
        let merged = EventKind::BaseMerged {
            base: "flow/shop-1".into(),
            onto: "main".into(),
        };
        let events: Vec<Event> = [research, message("one"), merged, message("two")]
            .into_iter()
            .map(|kind| Event::new(1, kind))
            .collect();

        assert_eq!(undelivered(&events, 0), [master("one"), master("two")]);
        assert_eq!(undelivered(&events, 1), [master("two")]);
    }

    #[test]
    fn queues_only_the_lines_not_yet_typed_and_no_messages() {
        let line = |text: &str| EventKind::Typed { text: text.into() };
        let events: Vec<Event> = [line("/compact"), message("hi"), line("continue shipping")]
            .into_iter()
            .map(|kind| Event::new(1, kind))
            .collect();

        assert_eq!(untyped(&events, 0), ["/compact", "continue shipping"]);
        assert_eq!(untyped(&events, 1), ["continue shipping"]);
        assert_eq!(untyped(&events, 2), [] as [&str; 0]);
        assert_eq!(undelivered(&events, 0), [master("hi")]);
    }

    #[test]
    fn wakes_the_master_for_flows_that_stopped_but_not_for_running_ones() {
        let flows = vec![
            flow("asks", vec![(5, EventKind::waiting("Which?"))]),
            flow("works", vec![(3, EventKind::Working)]),
            flow(
                "rests",
                vec![(
                    2,
                    EventKind::Idle {
                        note: "Ready.".into(),
                    },
                )],
            ),
            flow(
                "stuck",
                vec![(
                    4,
                    EventKind::Blocked {
                        reason: "tests".into(),
                    },
                )],
            ),
            flow("gone", vec![(1, EventKind::Ended)]),
        ];

        let names: Vec<&str> = changed_flows(&flows, &Seen::new())
            .iter()
            .map(|flow| flow.name.as_str())
            .collect();

        assert_eq!(names, ["rests", "stuck", "asks"]);
    }

    #[test]
    fn a_flow_the_master_saw_is_news_again_only_once_it_changes() {
        let asked = vec![flow("asks", vec![(5, EventKind::waiting("Which?"))])];
        let seen = Seen::new();
        let news = changed_flows(&asked, &seen);
        let mut marked = seen.clone();
        mark_seen(&mut marked, &news);
        let seen = marked;

        assert_eq!(changed_flows(&asked, &seen), [] as [&FlowView; 0]);

        let again = vec![flow(
            "asks",
            vec![
                (5, EventKind::waiting("Which?")),
                (9, EventKind::waiting("And the header?")),
            ],
        )];
        assert_eq!(changed_flows(&again, &seen).len(), 1);

        let mut silent = asked.clone();
        silent[0].state = State::NotResponding;
        assert_eq!(
            changed_flows(&silent, &seen).len(),
            1,
            "the same change, gone silent"
        );
    }

    #[test]
    fn never_wakes_the_master_for_a_plain_session() {
        let mut plain = flow("scratch", vec![(5, EventKind::waiting("Which?"))]);
        plain.mode = Mode::Plain;

        assert_eq!(changed_flows(&[plain], &Seen::new()), [] as [&FlowView; 0]);
    }

    #[test]
    fn starts_out_having_seen_every_finished_flow() {
        let flows = vec![
            flow("asks", vec![(5, EventKind::waiting("Which?"))]),
            flow(
                "shipped",
                vec![
                    (
                        2,
                        EventKind::MergeRequest {
                            url: "https://x/1".into(),
                        },
                    ),
                    (3, EventKind::Ended),
                ],
            ),
            flow("gone", vec![(1, EventKind::Ended)]),
        ];

        let seen = first_seen(&flows);

        assert_eq!(seen.len(), 2);
        let news = changed_flows(&flows, &seen);
        assert_eq!(news.len(), 1);
        assert_eq!(news[0].name, "asks");
    }

    #[test]
    fn a_question_after_pr_is_new_for_the_master() {
        let done = flow(
            "shipped",
            vec![(
                2,
                EventKind::MergeRequest {
                    url: "https://x/1".into(),
                },
            )],
        );
        let waiting = flow(
            "shipped",
            vec![
                (
                    2,
                    EventKind::MergeRequest {
                        url: "https://x/1".into(),
                    },
                ),
                (3, EventKind::waiting("What about the race?")),
            ],
        );
        let mut seen = Seen::new();
        mark_seen(&mut seen, &[&done]);
        let flows = [waiting];
        let changed = changed_flows(&flows, &seen);
        assert_eq!(changed.len(), 1);
        assert_eq!(changed[0].state, State::Waiting);
    }

    const LOG: &str = "/store/hooks/0b6e/setup.log";

    fn setup(failed: Option<&str>) -> EventKind {
        EventKind::Setup {
            failed: failed.map(str::to_string),
            log: LOG.into(),
        }
    }

    #[test]
    fn hands_a_worker_its_failed_setup_in_order_with_the_messages() {
        let events: Vec<Event> = [
            message("one"),
            setup(Some("Please run composer update")),
            setup(None),
            message("two"),
        ]
        .into_iter()
        .map(|kind| Event::new(1, kind))
        .collect();
        let note = Note::SetupFailed {
            reason: "Please run composer update".into(),
            log: LOG.into(),
        };

        assert_eq!(
            undelivered(&events, 0),
            [master("one"), note.clone(), master("two")]
        );
        assert_eq!(
            undelivered(&events, 2),
            [master("two")],
            "a pass is no note"
        );
        assert_eq!(
            for_worker(&[note]),
            format!(
                "news from saaio: .saaio/setup failed for this worktree: Please run composer \
update. Its whole output is in {LOG}. Read it. When the cause is in this repository, fix it and \
run `saaio setup`; when it needs something on this machine, such as a missing PHP extension, end \
your turn saying what is missing."
            )
        );
    }

    #[test]
    fn tells_the_master_once_per_failed_setup_whatever_the_flow_does() {
        let failed = flow(
            "shop-240",
            vec![(1, setup(Some("Please run composer update")))],
        );
        let news = changed_flows(std::slice::from_ref(&failed), &Seen::new());
        assert_eq!(news.len(), 1);
        assert_eq!(
            for_master(&news).lines().nth(1).unwrap().trim(),
            format!(
                "shop-240,blocked,auto,\"setup failed: Please run composer update (full output: {LOG})\""
            )
        );
        let mut seen = Seen::new();
        mark_seen(&mut seen, &news);

        let working = flow(
            "shop-240",
            vec![
                (1, setup(Some("Please run composer update"))),
                (2, EventKind::Working),
            ],
        );
        assert_eq!(
            changed_flows(std::slice::from_ref(&working), &seen).len(),
            0
        );

        let again = flow(
            "shop-240",
            vec![
                (1, setup(Some("Please run composer update"))),
                (2, EventKind::Working),
                (3, setup(Some("ext-zip is missing"))),
            ],
        );
        let news = changed_flows(std::slice::from_ref(&again), &seen);
        assert_eq!(news.len(), 1, "a second failure is news while it works");
        mark_seen(&mut seen, &news);
        assert_eq!(changed_flows(std::slice::from_ref(&again), &seen).len(), 0);

        let passed = flow(
            "shop-240",
            vec![
                (1, setup(Some("Please run composer update"))),
                (2, EventKind::Working),
                (4, setup(None)),
            ],
        );
        assert_eq!(changed_flows(std::slice::from_ref(&passed), &seen).len(), 0);
    }

    #[test]
    fn the_master_says_a_long_setup_reason_cut_but_the_log_whole() {
        let why = "z".repeat(LIST_TEXT_LIMIT + 5);
        let failed = flow("shop-240", vec![(1, setup(Some(&why)))]);

        let news = for_master(&[&failed]);

        assert!(news.contains(&format!("(full output: {LOG})")), "{news}");
        assert!(news.contains("(truncated, 85 chars"), "{news}");
    }

    #[test]
    fn reads_a_mark_written_before_setups_were_kept() {
        let mark: Mark = serde_json::from_str(r#"{"state":"blocked","changed_at":4}"#).unwrap();

        assert_eq!(mark.setup_failed, None);
        assert_eq!(
            serde_json::to_string(&mark).unwrap(),
            r#"{"state":"blocked","changed_at":4}"#
        );
    }

    #[test]
    fn hands_a_worker_each_message_from_the_master() {
        assert_eq!(
            for_worker(&[
                master("Use semicolons."),
                master("Keep the header row,\nas in the old export.")
            ]),
            [
                "message from the saaio master:",
                "  Use semicolons.",
                "message from the saaio master:",
                "  Keep the header row,",
                "  as in the old export.",
            ]
            .join("\n")
        );
    }

    #[test]
    fn points_the_master_to_a_dialogs_options() {
        let asked = flow("ask", vec![(1, dialog())]);

        let news = for_master(&[&asked]);

        assert!(news.contains("  ask,waiting,auto,Which color?\n"), "{news}");
        assert!(
            news.contains("`saaio status <name>` lists a question dialog's options"),
            "{news}"
        );
    }

    #[test]
    fn tells_the_master_what_changed_and_who_answers_by_mode() {
        let asked = flow(
            "shop-217",
            vec![(1, EventKind::waiting("Which delimiter?"))],
        );
        let mut ready = flow(
            "shop-220",
            vec![(
                1,
                EventKind::Idle {
                    note: "The design is ready for review.".into(),
                },
            )],
        );
        ready.mode = Mode::Guided;

        assert_eq!(
            for_master(&[&asked, &ready]),
            [
                "changed[2]{name,state,mode,says}:",
                "  shop-217,waiting,auto,Which delimiter?",
                "  shop-220,idle,guided,The design is ready for review.",
                "help[3]: \"auto: answer what the code, the plan or the user's decisions settle with `saaio send <name> \\\"<answer>\\\"`, then tell the user what you answered; else ask the user\",\"guided: tell the user what it says or asks; send their answer with `saaio send`\",`saaio status <name> --full` shows its whole last answer",
            ]
            .join("\n")
        );
    }
}
