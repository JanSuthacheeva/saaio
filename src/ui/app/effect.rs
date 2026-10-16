use std::path::{Path, PathBuf};

use crate::proto::{ClientMsg, Modes};
use crate::ui::newflow::NewFlowForm;

/// A `flow remove`, with what the person agreed to lose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Removal {
    pub name: String,
    /// Discard uncommitted work in the worktree (--force).
    pub force: bool,
    /// Delete the branch with commits on no other branch (--delete-unmerged).
    pub delete_unmerged: bool,
}

impl Removal {
    pub fn of(name: impl Into<String>) -> Removal {
        Removal {
            name: name.into(),
            force: false,
            delete_unmerged: false,
        }
    }
}

/// A flow command flow ui runs in the background.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Job {
    Start(NewFlowForm),
    Remove(Removal),
    /// `flow status --all --json`: every project's board.
    Overview,
}

impl Job {
    /// The arguments after the flow binary.
    pub fn args(&self) -> Vec<String> {
        match self {
            Job::Start(form) => {
                let mut args = vec![
                    "start".to_string(),
                    form.request.trim().to_string(),
                    "--mode".to_string(),
                    form.mode.name().to_string(),
                ];
                if let Some(kind) = form.picked_type() {
                    args.extend(["--type".to_string(), kind.to_string()]);
                }
                args
            }
            Job::Remove(removal) => {
                let mut args = vec!["remove".to_string(), removal.name.clone()];
                if removal.force {
                    args.push("--force".to_string());
                }
                if removal.delete_unmerged {
                    args.push("--delete-unmerged".to_string());
                }
                args
            }
            Job::Overview => vec![
                "status".to_string(),
                "--all".to_string(),
                "--json".to_string(),
            ],
        }
    }

    /// Where it runs: the repo a flow starts in.
    pub fn dir(&self) -> Option<&Path> {
        match self {
            Job::Start(form) => Some(&form.repo),
            Job::Remove(_) | Job::Overview => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    Send(ClientMsg),
    /// Read the repo's branch convention for the new-flow dialog; answered by on_branching.
    LoadBranching(PathBuf),
    Mirror(Modes),
    Run(Job),
    Quit(String),
}

/// What a flow command printed: its first line when it worked, the error without "error: " when not.
pub fn outcome(success: bool, stdout: &str) -> Result<String, String> {
    let first = stdout.lines().next().unwrap_or("").trim();
    if success {
        Ok(first.to_string())
    } else {
        Err(first.strip_prefix("error: ").unwrap_or(first).to_string())
    }
}
