//! What is news for a session (pure), and how it reads: `for_worker`, `for_master`

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::event::{Event, EventKind, Mode};
use crate::text::{LIST_TEXT_LIMIT, truncated};
use crate::view::{FlowView, State};

/// What the master was last woken for, by flow id.
pub type Seen = BTreeMap<String, Mark>;

/// A flow as the master last saw it: its state and when that changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mark {
    pub state: State,
    pub changed_at: u64,
}

/// The master's messages in a flow's log after the first `delivered` of them.
pub fn undelivered(events: &[Event], delivered: usize) -> Vec<&str> {
    events
        .iter()
        .filter_map(|event| match &event.kind {
            EventKind::Message { text } => Some(text.as_str()),
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

/// Flows whose state needs the master and that changed since it last saw them, oldest first; never
/// a plain session, which is the user's own.
pub fn changed_flows<'a>(flows: &'a [FlowView], seen: &Seen) -> Vec<&'a FlowView> {
    let mut changed: Vec<&FlowView> = flows
        .iter()
        .filter(|flow| needs_master(flow.state) && !flow.is_plain())
        .filter(|flow| seen.get(&flow.id) != Some(&mark(flow)))
        .collect();
    changed.sort_by_key(|flow| flow.changed_at);
    changed
}

/// Seen after the master was told about `flows`.
pub fn mark_seen(seen: &mut Seen, flows: &[&FlowView]) {
    for flow in flows {
        seen.insert(flow.id.clone(), mark(flow));
    }
}

/// The Seen to start from when none is stored: every finished flow, so only unfinished ones are news.
pub fn first_seen(flows: &[FlowView]) -> Seen {
    flows
        .iter()
        .filter(|flow| flow.is_finished())
        .map(|flow| (flow.id.clone(), mark(flow)))
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
    }
}

/// What a worker reads when saaio wakes it: each of the master's messages, indented.
pub fn for_worker(messages: &[&str]) -> String {
    messages
        .iter()
        .flat_map(|message| {
            std::iter::once("message from the saaio master:".to_string())
                .chain(message.lines().map(|line| format!("  {line}")))
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
    let says = |flow: &FlowView| {
        let text = flow.blocker.as_deref().unwrap_or_else(|| {
            let last = flow.timeline.last().map_or("", |entry| entry.text.as_str());
            last.strip_prefix("idle: ").unwrap_or(last)
        });
        truncated(text, LIST_TEXT_LIMIT, false)
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

    fn waiting(question: &str) -> EventKind {
        EventKind::Waiting {
            question: question.into(),
            questions: Vec::new(),
        }
    }

    fn message(text: &str) -> EventKind {
        EventKind::Message { text: text.into() }
    }

    fn dialog() -> EventKind {
        EventKind::Waiting {
            question: "Which color?".into(),
            questions: vec![Question::of("Which color?", &["Red", "Blue"], false)],
        }
    }

    /// Whether the master answered the dialog of a flow whose log holds `kinds`.
    fn answered(kinds: Vec<EventKind>) -> bool {
        let mut events = vec![fixtures::started("ask", "ask")];
        events.extend(kinds.into_iter().map(|kind| Event::new(1, kind)));
        let flow = fold::fold("ask", &events, None, 2).unwrap();
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
            !answered(vec![waiting("Ship it?"), message("Yes")]),
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

        assert_eq!(undelivered(&events, 0), ["one", "two"]);
        assert_eq!(undelivered(&events, 1), ["two"]);
        assert!(undelivered(&events, 2).is_empty());
        assert!(undelivered(&events, 5).is_empty());
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
        assert!(untyped(&events, 2).is_empty());
        assert_eq!(undelivered(&events, 0), ["hi"]);
    }

    #[test]
    fn wakes_the_master_for_flows_that_stopped_but_not_for_running_ones() {
        let flows = vec![
            flow("asks", vec![(5, waiting("Which?"))]),
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
        let asked = vec![flow("asks", vec![(5, waiting("Which?"))])];
        let seen = Seen::new();
        let news = changed_flows(&asked, &seen);
        let mut marked = seen.clone();
        mark_seen(&mut marked, &news);
        let seen = marked;

        assert!(changed_flows(&asked, &seen).is_empty());

        let again = vec![flow(
            "asks",
            vec![(5, waiting("Which?")), (9, waiting("And the header?"))],
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
        let mut plain = flow("scratch", vec![(5, waiting("Which?"))]);
        plain.mode = Mode::Plain;

        assert!(changed_flows(&[plain], &Seen::new()).is_empty());
    }

    #[test]
    fn starts_out_having_seen_every_finished_flow() {
        let flows = vec![
            flow("asks", vec![(5, waiting("Which?"))]),
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
                (3, waiting("What about the race?")),
            ],
        );
        let mut seen = Seen::new();
        mark_seen(&mut seen, &[&done]);
        let flows = [waiting];
        let changed = changed_flows(&flows, &seen);
        assert_eq!(changed.len(), 1);
        assert_eq!(changed[0].state, State::Waiting);
    }

    #[test]
    fn hands_a_worker_each_message_from_the_master() {
        assert_eq!(
            for_worker(&[
                "Use semicolons.",
                "Keep the header row,\nas in the old export."
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
        let asked = flow("shop-217", vec![(1, waiting("Which delimiter?"))]);
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
