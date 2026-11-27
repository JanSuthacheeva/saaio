//! What `saaio start` prints: the started or plain flow, its setup and where it opens

use serde::Serialize;

use super::setup::Setup;
use super::{Document, toon};
use crate::id::FlowId;
use crate::view::{FlowView, Workspace};

/// Whether saaio ui opens a started worker now or once it runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Opening {
    Running,
    WhenUiRuns,
}

/// flow, id, kind, worktree, branch (none for a flow that merges nothing), base (for a stacked
/// flow), slot, setup (ran / none / failed: ...), agent, skill (only when the agent lacked the
/// kind's skill), pane, help.
pub(crate) fn started(
    flow: &FlowView,
    workspace: &Workspace,
    setup: &Setup,
    missing_skill: Option<&str>,
    opening: Opening,
) -> Document {
    #[derive(Serialize)]
    struct Started<'a> {
        flow: &'a str,
        id: FlowId,
        #[serde(skip_serializing_if = "Option::is_none")]
        kind: Option<&'a str>,
        worktree: &'a str,
        #[serde(skip_serializing_if = "Option::is_none")]
        branch: Option<&'a str>,
        #[serde(skip_serializing_if = "Option::is_none")]
        base: Option<&'a str>,
        slot: Option<u8>,
        setup: String,
        agent: &'a str,
        #[serde(skip_serializing_if = "Option::is_none")]
        skill: Option<String>,
        pane: &'a str,
        help: Vec<String>,
    }
    let (pane, watch) = pane(flow, opening, "worker");
    let mut help = vec![watch, "Run `saaio` for all flows".to_string()];
    if workspace.branch.is_none() {
        help.push(
            "It has no branch: it answers in its pane, and `saaio clear` forgets it once it is done"
                .to_string(),
        );
    }
    if let Some(skill) = missing_skill {
        help.push(format!(
            "Install the {skill} skill for Claude Code, or set [kinds.{}] prompt \
             (docs/configuration.md#without-matching-skills)",
            flow.kind.as_deref().unwrap_or("feature")
        ));
    }
    Document::Toon(toon(&Started {
        flow: &flow.name,
        id: flow.id,
        kind: flow.kind.as_deref(),
        worktree: &workspace.path,
        branch: workspace.branch.as_deref(),
        base: workspace.base.as_deref(),
        slot: workspace.slot,
        setup: setup.to_string(),
        agent: &workspace.agent,
        skill: missing_skill
            .map(|skill| format!("{skill} not found, the worker got saaio's built-in prompt")),
        pane,
        help,
    }))
}

/// flow, id, folder, agent, pane, help: a plain session.
pub(crate) fn opened(flow: &FlowView, workspace: &Workspace, opening: Opening) -> Document {
    #[derive(Serialize)]
    struct Opened<'a> {
        flow: &'a str,
        id: FlowId,
        folder: &'a str,
        agent: &'a str,
        pane: &'a str,
        help: Vec<String>,
    }
    let (pane, watch) = pane(flow, opening, "session");
    Document::Toon(toon(&Opened {
        flow: &flow.name,
        id: flow.id,
        folder: &workspace.path,
        agent: &workspace.agent,
        pane,
        help: vec![watch, "Run `saaio` for all flows".to_string()],
    }))
}

/// The pane's state and how to watch it open.
fn pane(flow: &FlowView, opening: Opening, what: &str) -> (&'static str, String) {
    match opening {
        Opening::Running => (
            "opening",
            format!("saaio opens {} in its sidebar", flow.name),
        ),
        Opening::WhenUiRuns => (
            "when saaio opens",
            format!("Run `saaio` in a terminal to open the {what}"),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::super::fixtures::flow;
    use super::*;
    use crate::fixtures::workspace;

    #[test]
    fn names_a_failed_setup_and_its_whole_output() {
        let flow = flow("shop-240", vec![], 0);
        let log = "/store/hooks/0b6e/setup.log".to_string();
        let text = |setup: &Setup| {
            started(
                &flow,
                &workspace("shop-240", "claude"),
                setup,
                None,
                Opening::Running,
            )
            .text()
            .to_string()
        };

        let failed = text(&Setup::Failed {
            reason: "Please run composer update".into(),
            log: log.clone(),
        });
        let ran = text(&Setup::Ran { log });

        assert!(
            failed.contains(
                "setup: \"failed: Please run composer update (full output: /store/hooks/0b6e/setup.log)\""
            ),
            "{failed}"
        );
        assert!(ran.contains("\nsetup: ran\n"), "{ran}");
        assert!(!ran.contains("skill:"), "{ran}");
    }

    #[test]
    fn says_when_the_worker_got_the_built_in_prompt_and_how_to_change_that() {
        let mut flow = flow("shop-240", vec![], 0);
        flow.kind = Some("fix".to_string());

        let text = started(
            &flow,
            &workspace("shop-240", "claude"),
            &Setup::None,
            Some("ship-fix"),
            Opening::Running,
        )
        .text()
        .to_string();

        assert!(
            text.contains("skill: \"ship-fix not found, the worker got saaio's built-in prompt\""),
            "{text}"
        );
        assert!(
            text.contains(
                "Install the ship-fix skill for Claude Code, or set [kinds.fix] prompt \
                 (docs/configuration.md#without-matching-skills)"
            ),
            "{text}"
        );
    }
}
