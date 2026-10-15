use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::event::{Event, EventKind};
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

/// Flows whose state needs the master and that changed since it last saw them, oldest first.
pub fn changed_flows<'a>(flows: &'a [FlowView], seen: &Seen) -> Vec<&'a FlowView> {
    let mut changed: Vec<&FlowView> = flows
        .iter()
        .filter(|flow| needs_master(flow.state))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::Mode;
    use crate::fold;

    fn flow(id: &str, kinds: Vec<(u64, EventKind)>) -> FlowView {
        let mut events = vec![Event::new(
            0,
            EventKind::Started {
                name: id.into(),
                request: id.into(),
                mode: Mode::Auto,
            },
        )];
        events.extend(kinds.into_iter().map(|(at, kind)| Event::new(at, kind)));
        fold::fold(id, &events, None, 100).unwrap()
    }

    fn waiting(question: &str) -> EventKind {
        EventKind::Waiting {
            question: question.into(),
        }
    }

    fn message(text: &str) -> EventKind {
        EventKind::Message { text: text.into() }
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
    fn starts_out_having_seen_every_finished_flow() {
        let flows = vec![
            flow("asks", vec![(5, waiting("Which?"))]),
            flow(
                "shipped",
                vec![(
                    2,
                    EventKind::MergeRequest {
                        url: "https://x/1".into(),
                    },
                )],
            ),
            flow("gone", vec![(1, EventKind::Ended)]),
        ];

        let seen = first_seen(&flows);

        assert_eq!(seen.len(), 2);
        let news = changed_flows(&flows, &seen);
        assert_eq!(news.len(), 1);
        assert_eq!(news[0].name, "asks");
    }
}
