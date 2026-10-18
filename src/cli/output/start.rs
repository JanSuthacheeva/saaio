use std::fmt;

use serde::Serialize;

use super::{Document, toon};
use crate::view::{FlowView, Workspace};

/// Whether saaio ui opens a started worker now or once it runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Opening {
    Running,
    WhenUiRuns,
}

/// flow, id, worktree, branch, slot, setup (ran / none / failed: ...), agent, pane, help.
pub(crate) fn started(
    flow: &FlowView,
    workspace: &Workspace,
    setup: &Setup,
    opening: Opening,
) -> Document {
    #[derive(Serialize)]
    struct Started<'a> {
        flow: &'a str,
        id: &'a str,
        worktree: &'a str,
        branch: Option<&'a str>,
        slot: Option<u8>,
        setup: String,
        agent: &'a str,
        pane: &'a str,
        help: Vec<String>,
    }
    let (pane, watch) = pane(flow, opening, "worker");
    Document::Toon(toon(&Started {
        flow: &flow.name,
        id: &flow.id,
        worktree: &workspace.path,
        branch: workspace.branch.as_deref(),
        slot: workspace.slot,
        setup: setup.to_string(),
        agent: &workspace.agent,
        pane,
        help: vec![watch, "Run `saaio` for all flows".to_string()],
    }))
}

/// flow, id, folder, agent, pane, help: a plain session.
pub(crate) fn opened(flow: &FlowView, workspace: &Workspace, opening: Opening) -> Document {
    #[derive(Serialize)]
    struct Opened<'a> {
        flow: &'a str,
        id: &'a str,
        folder: &'a str,
        agent: &'a str,
        pane: &'a str,
        help: Vec<String>,
    }
    let (pane, watch) = pane(flow, opening, "session");
    Document::Toon(toon(&Opened {
        flow: &flow.name,
        id: &flow.id,
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

/// What `saaio start` did with the repo's .saaio/setup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Setup {
    Ran,
    None,
    Failed(String),
}

/// ran, none, failed: why
impl fmt::Display for Setup {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Setup::Ran => f.write_str("ran"),
            Setup::None => f.write_str("none"),
            Setup::Failed(reason) => write!(f, "failed: {reason}"),
        }
    }
}
