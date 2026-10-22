//! `Effect`, what `App` asks of the loop; `Job`, the saaio commands it runs, and their outcome

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::event::Mode;
use crate::proto::{ClientMsg, Modes};
use crate::refusal::Refusal;
use crate::ui::newflow::NewFlowForm;
use crate::ui::selection::Selection;

/// A `saaio remove`, with what the person agreed to lose.
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

/// A saaio command saaio ui runs in the background.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Job {
    Start(NewFlowForm),
    Remove(Removal),
    /// `saaio status --all --json`: every project's board.
    Overview,
}

impl Job {
    /// The arguments after the flow binary: options first, then `--` before a request or a flow
    /// name, so one that starts with `-` is never read as an option.
    pub fn args(&self) -> Vec<String> {
        match self {
            Job::Start(form) if form.mode == Mode::Plain => {
                let mut args = vec![
                    "start".to_string(),
                    format!("--plain={}", form.folder.trim()),
                ];
                if let Some(agent) = form.picked_agent() {
                    args.extend(["--agent".to_string(), agent.to_string()]);
                }
                args
            }
            Job::Start(form) => {
                let mut args = vec![
                    "start".to_string(),
                    "--mode".to_string(),
                    form.mode.name().to_string(),
                ];
                if let Some(kind) = form.picked_type() {
                    args.extend(["--type".to_string(), kind.to_string()]);
                }
                if let Some(agent) = form.picked_agent() {
                    args.extend(["--agent".to_string(), agent.to_string()]);
                }
                args.extend(["--".to_string(), form.request.trim().to_string()]);
                args
            }
            Job::Remove(removal) => {
                let mut args = vec!["remove".to_string()];
                if removal.force {
                    args.push("--force".to_string());
                }
                if removal.delete_unmerged {
                    args.push("--delete-unmerged".to_string());
                }
                args.extend(["--".to_string(), removal.name.clone()]);
                args
            }
            Job::Overview => vec![
                "status".to_string(),
                "--all".to_string(),
                "--json".to_string(),
            ],
        }
    }

    /// How server.log names it: `saaio start in ~/projects/shop`, `saaio remove --force -- shop-230`;
    /// without the request, which can be long.
    pub fn summary(&self) -> String {
        match self {
            Job::Start(form) if form.mode != Mode::Plain => {
                format!("saaio start in {}", form.repo.display())
            }
            _ => format!("saaio {}", self.args().join(" ")),
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
    /// Copy the selected text as drawn.
    CopySelection(Selection),
    /// Copy text a pane's program put on the clipboard.
    Copy(String),
    /// Trust the repository this folder is in: the person said yes in saaio ui.
    Trust(PathBuf),
    Quit(String),
}

/// Why a flow command failed: its error, and the refusal it was, if saaio ui asks about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobError {
    pub message: String,
    pub refusal: Option<Refusal>,
}

impl JobError {
    /// An error saaio ui only shows.
    pub fn other(message: impl Into<String>) -> JobError {
        JobError {
            message: message.into(),
            refusal: None,
        }
    }
}

/// What a flow command printed: its first line when it worked, the error without "error: " when not.
pub fn outcome(success: bool, stdout: &str) -> Result<String, JobError> {
    let first = stdout.lines().next().unwrap_or("").trim();
    if success {
        return Ok(first.to_string());
    }
    let message = error_message(first)
        .unwrap_or_else(|| first.strip_prefix("error: ").unwrap_or(first).to_string());
    Err(JobError {
        refusal: Refusal::of(&message),
        message,
    })
}

/// The value of an `error: ...` line, decoded as TOON writes it (quoted when it holds `: `).
fn error_message(line: &str) -> Option<String> {
    #[derive(Deserialize)]
    struct Printed {
        error: String,
    }
    serde_toon::from_str::<Printed>(line)
        .ok()
        .map(|printed| printed.error)
}
