//! `Event`, `EventKind`, `Mode` and the JSON line format of the log

use std::time::{SystemTime, UNIX_EPOCH};

use clap::ValueEnum;
use serde::{Deserialize, Serialize};

use crate::id::SessionId;
use crate::text;
use crate::workflow::{Phase, Workflow};

pub const EVENT_VERSION: u32 = 1;
pub const TEXT_LIMIT: usize = 500;
/// The longest message or answer: what the master tells a worker, a turn's whole closing message.
pub const LONG_TEXT_LIMIT: usize = 4000;
/// The most questions of a dialog, and options of a question, an event keeps.
pub const MAX_QUESTIONS: usize = 8;
/// The longest request saaio start and saaio new take; past it they refuse. Kept whole, since it
/// becomes the worker's prompt, one argv string (Linux refuses one over 128 KiB).
pub const REQUEST_LIMIT: usize = 20_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Event {
    pub v: u32,
    pub at: u64,
    #[serde(flatten)]
    pub kind: EventKind,
}

/// One line per event: {"v":1,"at":1791945698308,"kind":"blocked","reason":"..."}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum EventKind {
    Started {
        name: String,
        request: String,
        mode: Mode,
        /// The flow's phase names in order, in logs from before its whole workflow was recorded;
        /// None in older ones, for a plain session and beside `workflow`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        phases: Option<Vec<Phase>>,
        /// The flow's kind's name: "fix"; None for a plain session and older flows.
        #[serde(rename = "flow_kind", default, skip_serializing_if = "Option::is_none")]
        kind: Option<String>,
        /// Its kind's phases with the skills that start them.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        workflow: Option<Workflow>,
        /// The first prompt's template, {request}, {id} and {name} still to fill.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        prompt: Option<String>,
        /// false: nothing to merge; the flow is done once it answered.
        #[serde(default = "merges", skip_serializing_if = "merges_by_default")]
        merges: bool,
    },
    Phase {
        phase: Phase,
    },
    Working,
    Delegated {
        description: String,
    },
    Waiting {
        question: String,
        /// The questions of the dialog the worker shows (Claude Code's AskUserQuestion); empty for
        /// a question asked in words or a permission prompt.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        questions: Vec<Question>,
        /// The harness asks permission for a tool call; question names the call.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        permission: bool,
    },
    /// Codex may approve a PermissionRequest itself. It becomes waiting only after a grace period.
    PermissionPending {
        question: String,
    },
    /// The turn ended without asking anything: the worker waits, but not on the person.
    Idle {
        note: String,
    },
    Blocked {
        reason: String,
    },
    Decision {
        text: String,
    },
    Tasks {
        done: u32,
        total: u32,
    },
    Review {
        fixed: u32,
        open: u32,
    },
    MergeRequest {
        url: String,
    },
    /// The merge request at url was merged, into the branch `into` when the forge said.
    Merged {
        url: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        into: Option<String>,
    },
    /// The merge request at url was closed without a merge.
    MergeRequestClosed {
        url: String,
    },
    /// What the master told this flow's worker, through `saaio send`.
    Message {
        text: String,
    },
    /// A line the master queued for the server to type into this flow's pane, through
    /// `saaio send --type`.
    Typed {
        text: String,
    },
    /// A turn's whole closing message, for the master to read; Waiting and Idle keep its short form.
    Answer {
        text: String,
    },
    Ended,
    /// The harness's own session id and transcript, for one that picks them itself (Codex).
    Session {
        id: SessionId,
        transcript: Option<String>,
    },
    Workspace {
        path: String,
        branch: Option<String>,
        slot: Option<u8>,
        window: Option<String>,
        agent: String,
        /// The branch the worktree started from (`saaio start --base`).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        base: Option<String>,
    },
    /// The branch this flow stands on (its workspace's base) merged; it now stands on `onto`.
    BaseMerged {
        base: String,
        onto: String,
    },
    /// Where the flow's services stand: saaio server ran or is running the repo's .saaio/pause
    /// or .saaio/resume.
    Services {
        status: ServiceStatus,
        /// The script's last error line, for pause-failed and resume-failed.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    /// The repo's .saaio/setup ran for this flow: why it failed (its log's last line), None when it
    /// passed; where its whole output is.
    Setup {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        failed: Option<String>,
        log: String,
    },
    /// A tool call that runs `command` waits for the project's turn, which `holder` (a flow's
    /// name) has.
    TurnWaiting {
        command: String,
        holder: String,
    },
    /// The waiting call has its turn, or stopped waiting for it.
    TurnTaken,
    /// The worker got a prompt: a call that waited for its turn is over, though an Esc that
    /// killed its hook said nothing.
    Prompted,
    /// The worker's main agent changed a file; kept only when it moves the flow back to its
    /// build phase.
    Edited {
        path: String,
    },
}

/// A step of pausing or resuming a flow's services.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ServiceStatus {
    Pausing,
    Paused,
    Resuming,
    Running,
    PauseFailed,
    ResumeFailed,
}

/// One question of a dialog: its header, text, options and whether several may be picked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Question {
    pub header: String,
    pub question: String,
    pub options: Vec<Choice>,
    pub multi_select: bool,
}

/// An option of a question: what the dialog lists and the line below it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Choice {
    pub label: String,
    pub description: String,
}

fn merges() -> bool {
    true
}

#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde's skip_serializing_if passes a reference"
)]
fn merges_by_default(merges: &bool) -> bool {
    *merges
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum Mode {
    #[default]
    Guided,
    Auto,
    /// An agent in a folder: no worktree, branch, slot, prompt or master.
    #[value(skip)]
    Plain,
}

/// A time as Unix ms, the clock every event and log uses; 0 before 1970.
#[must_use]
pub fn unix_ms(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH).map_or(0, |since| {
        u64::try_from(since.as_millis()).unwrap_or(u64::MAX)
    })
}

impl Event {
    /// An event of the current version at `at` (Unix ms), each text without control characters
    /// but its newlines and tabs and cut to its limit, at most
    /// MAX_QUESTIONS questions and options; except a Started request: it is the worker's prompt,
    /// kept whole (its command checks REQUEST_LIMIT).
    pub fn new(at: u64, mut kind: EventKind) -> Event {
        kind.clean_texts();
        Event {
            v: EVENT_VERSION,
            at,
            kind,
        }
    }

    /// One line of JSON, no trailing newline.
    pub fn to_line(&self) -> String {
        serde_json::to_string(self).expect("an event always serialises")
    }

    /// None for a broken line, another version or an unknown kind,
    /// so a log written by a newer saaio still folds.
    pub fn from_line(line: &str) -> Option<Event> {
        serde_json::from_str::<Event>(line)
            .ok()
            .filter(|event| event.v == EVENT_VERSION)
    }

    /// Every readable event of a log's text, in order: from_line of each line.
    pub fn from_log(text: &str) -> Vec<Event> {
        text.lines().filter_map(Event::from_line).collect()
    }
}

impl EventKind {
    /// A question asked in words: no dialog, no permission prompt.
    pub fn waiting(question: impl Into<String>) -> EventKind {
        EventKind::Waiting {
            question: question.into(),
            questions: Vec::new(),
            permission: false,
        }
    }

    fn clean_texts(&mut self) {
        match self {
            EventKind::Started { name: text, .. }
            | EventKind::Delegated { description: text }
            | EventKind::PermissionPending { question: text }
            | EventKind::Idle { note: text }
            | EventKind::Blocked { reason: text }
            | EventKind::Decision { text }
            | EventKind::MergeRequest { url: text }
            | EventKind::Merged { url: text, .. }
            | EventKind::MergeRequestClosed { url: text }
            | EventKind::Services {
                reason: Some(text), ..
            } => clean(text, TEXT_LIMIT),
            EventKind::BaseMerged { base, onto } => {
                clean(base, TEXT_LIMIT);
                clean(onto, TEXT_LIMIT);
            }
            EventKind::TurnWaiting { command, holder } => {
                clean(command, TEXT_LIMIT);
                clean(holder, TEXT_LIMIT);
            }
            EventKind::Setup { failed, log } => {
                if let Some(reason) = failed {
                    clean(reason, TEXT_LIMIT);
                }
                clean(log, LONG_TEXT_LIMIT);
            }
            EventKind::Message { text }
            | EventKind::Typed { text }
            | EventKind::Answer { text } => clean(text, LONG_TEXT_LIMIT),
            EventKind::Waiting {
                question,
                questions,
                ..
            } => {
                clean(question, TEXT_LIMIT);
                questions.truncate(MAX_QUESTIONS);
                for asked in questions {
                    asked.clean_texts();
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
impl Question {
    /// A question headed by its first word, each option described as "<label> it is".
    pub fn of(question: &str, labels: &[&str], multi_select: bool) -> Question {
        Question {
            header: question.split(' ').next().unwrap_or_default().to_string(),
            question: question.to_string(),
            options: labels
                .iter()
                .map(|label| Choice {
                    label: (*label).to_string(),
                    description: format!("{label} it is"),
                })
                .collect(),
            multi_select,
        }
    }
}

impl Question {
    fn clean_texts(&mut self) {
        clean(&mut self.header, TEXT_LIMIT);
        clean(&mut self.question, TEXT_LIMIT);
        self.options.truncate(MAX_QUESTIONS);
        for choice in &mut self.options {
            clean(&mut choice.label, TEXT_LIMIT);
            clean(&mut choice.description, TEXT_LIMIT);
        }
    }
}

/// `text` without control characters but its newlines and tabs, cut to `limit` chars.
fn clean(text: &mut String, limit: usize) {
    *text = text::printable_lines(text);
    cut_to(text, limit);
}

fn cut_to(text: &mut String, limit: usize) {
    if let Some((end, _)) = text.char_indices().nth(limit) {
        text.truncate(end);
    }
}

impl Mode {
    /// auto, guided, plain - the new-flow dialog's order.
    pub const ALL: [Mode; 3] = [Mode::Auto, Mode::Guided, Mode::Plain];

    /// The name the command line and the logs use: guided, auto, plain.
    pub fn name(self) -> &'static str {
        match self {
            Mode::Guided => "guided",
            Mode::Auto => "auto",
            Mode::Plain => "plain",
        }
    }
}

impl ServiceStatus {
    /// The name the JSON and TOON output use: paused, resume-failed, ...
    pub fn name(self) -> &'static str {
        match self {
            ServiceStatus::Pausing => "pausing",
            ServiceStatus::Paused => "paused",
            ServiceStatus::Resuming => "resuming",
            ServiceStatus::Running => "running",
            ServiceStatus::PauseFailed => "pause-failed",
            ServiceStatus::ResumeFailed => "resume-failed",
        }
    }
}

#[cfg(test)]
mod tests;
