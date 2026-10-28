//! What `saaio new`, `saaio report` and `saaio send` print about one flow

use serde::Serialize;

use super::{Document, toon};
use crate::view::{FlowView, State};
use crate::workflow::Phase;

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
        help: flow
            .steps
            .first()
            .map(|step| {
                let phase = step.phase.name();
                format!(
                    "Run `saaio report --flow {} phase {phase}` when {phase} starts",
                    flow.name
                )
            })
            .into_iter()
            .chain([format!(
                "Or set SAAIO_FLOW_ID={} in the worker's environment and leave out --flow",
                flow.id
            )])
            .collect(),
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
        State::Blocked => "Run `saaio report working` once the block is resolved".to_string(),
        State::Waiting => "Run `saaio report working` once answered".to_string(),
        State::Done => "Run `saaio clear` after the worker session ends".to_string(),
        _ => format!(
            "Run `saaio status {}` for its phases and timeline",
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

/// sent, the flow's state, and who wakes whom now; for a flow on a question dialog, that saaio
/// closes it; for one on a permission prompt, what it asks and that the worker reads the message
/// once it is answered.
pub(crate) fn sent(flow: &FlowView) -> Document {
    #[derive(Serialize)]
    struct Sent<'a> {
        sent: &'a str,
        state: &'static str,
        #[serde(skip_serializing_if = "Option::is_none")]
        prompt: Option<&'a str>,
        help: [String; 2],
    }
    let prompt = flow.blocker.as_deref().filter(|_| flow.permission);
    let reads = if prompt.is_some() {
        format!(
            "{}'s permission prompt is open in its pane. Its worker reads this once the user answers the prompt there",
            flow.name
        )
    } else if flow.questions.is_empty() {
        "saaio wakes its worker with it now, or once its session runs again".to_string()
    } else {
        format!(
            "saaio closes {}'s question dialog and its worker reads this as the answer to every question in it",
            flow.name
        )
    };
    Document::Toon(toon(&Sent {
        sent: &flow.name,
        state: flow.state.name(),
        prompt,
        help: [
            reads,
            format!("saaio wakes you when {} changes again", flow.name),
        ],
    }))
}

/// queued, the line, how many lines wait before it, and when saaio types it.
pub(crate) fn queued(flow: &FlowView, line: &str, ahead: usize) -> Document {
    #[derive(Serialize)]
    struct Queued<'a> {
        queued: &'a str,
        line: &'a str,
        ahead: usize,
        help: [String; 2],
    }
    Document::Toon(toon(&Queued {
        queued: &flow.name,
        line,
        ahead,
        help: [
            format!(
                "saaio types it into {}'s pane once its session is idle",
                flow.name
            ),
            format!(
                "Queue the next line with another `saaio send --type {} \"<line>\"`. It waits for this one's turn to end",
                flow.name
            ),
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
        help: ["Set SAAIO_FLOW_ID or pass --flow <name>"],
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::output::fixtures::{MIN, flow};
    use crate::event::{EventKind, Question};
    use crate::workflow::Phase;

    #[test]
    fn created_names_the_flow_and_how_to_report_into_it() {
        let flow = flow("shop-221", vec![], MIN);

        assert_eq!(
            created(&flow, false).text(),
            [
                "flow: shop-221",
                "id: shop-221-id",
                "existing: false",
                "help[2]: Run `saaio report --flow shop-221 phase plan` when plan starts,Or set SAAIO_FLOW_ID=shop-221-id in the worker's environment and leave out --flow",
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
                "help[1]: Run `saaio report working` once the block is resolved",
            ]
            .join("\n")
        );

        let waiting = flow("shop-221", vec![EventKind::waiting("ok?")], MIN);
        assert!(
            reported(&waiting)
                .text()
                .ends_with("help[1]: Run `saaio report working` once answered")
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
                .ends_with("help[1]: Run `saaio clear` after the worker session ends")
        );

        let working = flow("shop-221", vec![], MIN);
        assert_eq!(
            reported(&working).text(),
            "flow: shop-221\nstate: working\nhelp[1]: Run `saaio status shop-221` for its phases and timeline"
        );
    }

    #[test]
    fn skipped_says_why_and_how_to_give_a_flow() {
        assert_eq!(
            skipped().text(),
            "skipped: no flow id\nhelp[1]: Set SAAIO_FLOW_ID or pass --flow <name>"
        );
    }

    #[test]
    fn says_a_message_was_sent_and_who_wakes_whom() {
        let asked = flow(
            "shop-217",
            vec![EventKind::waiting("Which delimiter?")],
            10 * MIN,
        );

        assert_eq!(
            sent(&asked).text(),
            [
                "sent: shop-217",
                "state: waiting",
                "help[2]: \"saaio wakes its worker with it now, or once its session runs again\",saaio wakes you when shop-217 changes again",
            ]
            .join("\n")
        );
    }

    #[test]
    fn says_saaio_closes_an_open_dialog_for_the_message() {
        let asked = flow(
            "ask",
            vec![EventKind::Waiting {
                question: "Which color?".into(),
                questions: vec![Question::of("Which color?", &["Red", "Blue"], false)],
                permission: false,
            }],
            10 * MIN,
        );

        assert_eq!(
            sent(&asked).text(),
            [
                "sent: ask",
                "state: waiting",
                "help[2]: saaio closes ask's question dialog and its worker reads this as the answer to every question in it,saaio wakes you when ask changes again",
            ]
            .join("\n")
        );
    }

    #[test]
    fn says_the_worker_reads_it_once_its_permission_prompt_is_answered() {
        let asked = flow(
            "ask",
            vec![EventKind::Waiting {
                question: "permission: Bash(curl -sI https://example.org)".into(),
                questions: Vec::new(),
                permission: true,
            }],
            10 * MIN,
        );

        assert_eq!(
            sent(&asked).text(),
            [
                "sent: ask",
                "state: waiting",
                "prompt: \"permission: Bash(curl -sI https://example.org)\"",
                "help[2]: ask's permission prompt is open in its pane. Its worker reads this once the user answers the prompt there,saaio wakes you when ask changes again",
            ]
            .join("\n")
        );
    }

    #[test]
    fn says_a_line_was_queued_and_how_many_wait_before_it() {
        let idle = flow(
            "shop-217",
            vec![EventKind::Idle {
                note: "plan done".into(),
            }],
            10 * MIN,
        );

        assert_eq!(
            queued(&idle, "/compact", 0).text(),
            [
                "queued: shop-217",
                "line: /compact",
                "ahead: 0",
                "help[2]: saaio types it into shop-217's pane once its session is idle,\"Queue the next line with another `saaio send --type shop-217 \\\"<line>\\\"`. It waits for this one's turn to end\"",
            ]
            .join("\n")
        );
        assert!(
            queued(&idle, "continue shipping", 1)
                .text()
                .contains("ahead: 1")
        );
    }
}
