use serde::Serialize;

use super::{LIST_TEXT_LIMIT, toon, truncated};
use crate::event::Mode;
use crate::view::FlowView;

/// What a worker reads when saaio wakes it: each of the master's messages, indented.
pub(crate) fn for_worker(messages: &[&str]) -> String {
    messages
        .iter()
        .flat_map(|message| {
            std::iter::once("message from the saaio master:".to_string())
                .chain(message.lines().map(|line| format!("  {line}")))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// What the master reads when saaio wakes it: changed[n]{name,state,mode,says}, and who answers
/// by the flows' modes.
pub(crate) fn for_master(flows: &[&FlowView]) -> String {
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
    help.push("`saaio status <name> --full` shows its whole last answer");
    let says = |flow: &FlowView| {
        let text = flow.blocker.as_deref().unwrap_or_else(|| {
            let last = flow.timeline.last().map_or("", |entry| entry.text.as_str());
            last.strip_prefix("idle: ").unwrap_or(last)
        });
        truncated(text, LIST_TEXT_LIMIT, false)
    };
    toon(&News {
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::output::fixtures::{MIN, flow};
    use crate::event::{EventKind, Mode};

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
    fn tells_the_master_what_changed_and_who_answers_by_mode() {
        let now = 10 * MIN;
        let asked = flow(
            "shop-217",
            vec![EventKind::Waiting {
                question: "Which delimiter?".into(),
            }],
            now,
        );
        let mut ready = flow(
            "shop-220",
            vec![EventKind::Idle {
                note: "The design is ready for review.".into(),
            }],
            now,
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
