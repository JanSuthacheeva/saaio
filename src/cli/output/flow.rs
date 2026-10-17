use serde::Serialize;

use super::{Document, toon};
use crate::event::Phase;
use crate::view::{FlowView, State};

/// flow, id, existing (true when `new` found the unfinished flow of that name), how to use it.
pub(crate) fn created(flow: &FlowView, existing: bool) -> Document {
    #[derive(Serialize)]
    struct Created<'a> {
        flow: &'a str,
        id: &'a str,
        existing: bool,
        help: Vec<String>,
    }
    Document::Toon(toon(&Created {
        flow: &flow.name,
        id: &flow.id,
        existing,
        help: vec![
            format!(
                "Run `flow report --flow {} phase design` when design starts",
                flow.name
            ),
            format!(
                "Or set FLOW_ID={} in the worker's environment and leave out --flow",
                flow.id
            ),
        ],
    }))
}

/// flow, state, phase, and the next step that fits the new state.
pub(crate) fn reported(flow: &FlowView) -> Document {
    #[derive(Serialize)]
    struct Reported<'a> {
        flow: &'a str,
        state: &'static str,
        #[serde(skip_serializing_if = "Option::is_none")]
        phase: Option<&'a str>,
        help: Vec<String>,
    }
    let next = match flow.state {
        State::Blocked => "Run `flow report working` once the block is resolved".to_string(),
        State::Waiting => "Run `flow report working` once answered".to_string(),
        State::Done => "Run `flow clear` after the MR is merged".to_string(),
        _ => format!(
            "Run `flow status {}` for its phases and timeline",
            flow.name
        ),
    };
    Document::Toon(toon(&Reported {
        flow: &flow.name,
        state: flow.state.name(),
        phase: flow.phase.as_ref().map(Phase::name),
        help: vec![next],
    }))
}

/// sent, the flow's state, and who wakes whom now.
pub(crate) fn sent(flow: &FlowView) -> Document {
    #[derive(Serialize)]
    struct Sent<'a> {
        sent: &'a str,
        state: &'static str,
        help: [String; 2],
    }
    Document::Toon(toon(&Sent {
        sent: &flow.name,
        state: flow.state.name(),
        help: [
            "flow wakes its worker with it now, or once its session runs again".to_string(),
            format!("flow wakes you when {} changes again", flow.name),
        ],
    }))
}

/// skipped: no flow id, and how to give one.
pub(crate) fn skipped() -> Document {
    #[derive(Serialize)]
    struct Skipped {
        skipped: &'static str,
        help: [&'static str; 1],
    }
    Document::Toon(toon(&Skipped {
        skipped: "no flow id",
        help: ["Set FLOW_ID or pass --flow <name>"],
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::output::fixtures::{MIN, flow};
    use crate::event::{EventKind, Phase};

    #[test]
    fn created_names_the_flow_and_how_to_report_into_it() {
        let flow = flow("shop-221", vec![], MIN);

        assert_eq!(
            created(&flow, false).text(),
            [
                "flow: shop-221",
                "id: shop-221-id",
                "existing: false",
                "help[2]: Run `flow report --flow shop-221 phase design` when design starts,Or set FLOW_ID=shop-221-id in the worker's environment and leave out --flow",
            ]
            .join("\n")
        );
        assert!(created(&flow, true).text().contains("existing: true"));
    }

    #[test]
    fn reported_answers_with_the_new_state_and_what_comes_next() {
        let blocked = flow(
            "shop-221",
            vec![
                EventKind::Phase {
                    phase: Phase::of("implement"),
                },
                EventKind::Blocked { reason: "x".into() },
            ],
            MIN,
        );
        assert_eq!(
            reported(&blocked).text(),
            [
                "flow: shop-221",
                "state: blocked",
                "phase: implement",
                "help[1]: Run `flow report working` once the block is resolved",
            ]
            .join("\n")
        );

        let waiting = flow(
            "shop-221",
            vec![EventKind::Waiting {
                question: "ok?".into(),
            }],
            MIN,
        );
        assert!(
            reported(&waiting)
                .text()
                .ends_with("help[1]: Run `flow report working` once answered")
        );

        let done = flow(
            "shop-221",
            vec![EventKind::MergeRequest {
                url: "https://x/1".into(),
            }],
            MIN,
        );
        assert!(
            reported(&done)
                .text()
                .ends_with("help[1]: Run `flow clear` after the MR is merged")
        );

        let working = flow("shop-221", vec![], MIN);
        assert_eq!(
            reported(&working).text(),
            "flow: shop-221\nstate: working\nhelp[1]: Run `flow status shop-221` for its phases and timeline"
        );
    }

    #[test]
    fn skipped_says_why_and_how_to_give_a_flow() {
        assert_eq!(
            skipped().text(),
            "skipped: no flow id\nhelp[1]: Set FLOW_ID or pass --flow <name>"
        );
    }

    #[test]
    fn says_a_message_was_sent_and_who_wakes_whom() {
        let asked = flow(
            "shop-217",
            vec![EventKind::Waiting {
                question: "Which delimiter?".into(),
            }],
            10 * MIN,
        );

        assert_eq!(
            sent(&asked).text(),
            [
                "sent: shop-217",
                "state: waiting",
                "help[2]: \"flow wakes its worker with it now, or once its session runs again\",flow wakes you when shop-217 changes again",
            ]
            .join("\n")
        );
    }
}
