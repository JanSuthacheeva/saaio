//! `Event`, `EventKind`, `Phase`, `Mode` and the JSON line format of the log

use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};

use clap::ValueEnum;
use serde::{Deserialize, Serialize};

use crate::text;

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
        /// The flow's phases in order; None in a log from before they were configurable, and for
        /// a plain session.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        phases: Option<Vec<Phase>>,
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
        id: String,
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

/// A phase's name: plan, implement, ... as the workflow names it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Phase(String);

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

/// Whether `text` is a UUID: what saaio's flow and master ids and Codex's session ids are, and
/// what alone may become a file name or a resume argument.
#[must_use]
pub fn is_uuid(text: &str) -> bool {
    text.len() == 36 && uuid::Uuid::try_parse(text).is_ok()
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

impl TryFrom<String> for Phase {
    type Error = String;

    /// Lowercase letters, digits and '-'; not empty.
    fn try_from(name: String) -> Result<Phase, String> {
        let allowed = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-';
        if name.is_empty() || !name.chars().all(allowed) {
            return Err(format!(
                "{name:?} is no phase name: lowercase letters, digits and -"
            ));
        }
        Ok(Phase(name))
    }
}

impl From<Phase> for String {
    fn from(phase: Phase) -> String {
        phase.0
    }
}

impl FromStr for Phase {
    type Err = String;

    fn from_str(name: &str) -> Result<Phase, String> {
        Phase::try_from(name.to_string())
    }
}

#[cfg(test)]
impl Phase {
    /// A phase by a name the test knows is valid.
    pub fn of(name: &str) -> Phase {
        name.parse().expect("a valid phase name")
    }
}

impl Phase {
    /// The name the workflow, the command line and the logs use.
    pub fn name(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_modes_and_phases_as_the_log_does() {
        for mode in Mode::ALL {
            assert_eq!(serde_json::to_value(mode).unwrap(), mode.name());
        }
        let phase = phase("merge-request");
        assert_eq!(serde_json::to_value(&phase).unwrap(), "merge-request");
        assert_eq!(phase.name(), "merge-request");
    }

    fn phase(name: &str) -> Phase {
        name.parse().unwrap()
    }

    #[test]
    fn a_uuid_is_only_the_hyphenated_form() {
        for good in [
            "3b91bb6e-9457-4f37-9278-46e3a5e2ade4",
            "01a14000-0000-7000-8000-000000000001",
        ] {
            assert!(is_uuid(good), "{good}");
        }
        for bad in [
            "-cnotify=[\"sh\",\"-c\",\"id\"]",
            "../../x",
            "7f3c",
            "",
            "3b91bb6e94574f37927846e3a5e2ade4",
            "{3b91bb6e-9457-4f37-9278-46e3a5e2ade4}",
            "3b91bb6e-9457-4f37-9278-46e3a5e2ade4 ",
        ] {
            assert!(!is_uuid(bad), "{bad}");
        }
    }

    #[test]
    fn a_phase_is_named_in_lowercase_letters_digits_and_hyphens() {
        assert_eq!(phase("step-2").name(), "step-2");
        for bad in ["", "Plan", "my phase", "plan!", "über"] {
            let error = bad.parse::<Phase>().unwrap_err();
            assert!(error.contains("lowercase letters, digits and -"), "{error}");
        }
        assert!(
            serde_json::from_str::<Phase>(r#""Review""#).is_err(),
            "a log cannot smuggle one in"
        );
    }

    fn every_kind() -> Vec<EventKind> {
        vec![
            EventKind::Started {
                name: "shop-221".into(),
                request: "SHOP-221 add CSV export".into(),
                mode: Mode::Auto,
                phases: Some(vec![phase("plan"), phase("ship")]),
            },
            EventKind::Started {
                name: "scratch".into(),
                request: String::new(),
                mode: Mode::Plain,
                phases: None,
            },
            EventKind::Phase {
                phase: phase("merge-request"),
            },
            EventKind::Working,
            EventKind::Delegated {
                description: "implement plan".into(),
            },
            EventKind::waiting("Approve the plan?"),
            EventKind::Waiting {
                question: "Which color?".into(),
                questions: vec![color()],
                permission: false,
            },
            EventKind::Blocked {
                reason: "Task 5: migration needs a default".into(),
            },
            EventKind::Decision {
                text: "approach B".into(),
            },
            EventKind::Tasks { done: 4, total: 9 },
            EventKind::Review { fixed: 3, open: 1 },
            EventKind::MergeRequest {
                url: "https://gitlab.com/acme/shop/-/merge_requests/88".into(),
            },
            EventKind::Merged {
                url: "https://github.com/acme/shop/pull/42".into(),
                into: Some("main".into()),
            },
            EventKind::MergeRequestClosed {
                url: "https://github.com/acme/shop/pull/41".into(),
            },
            EventKind::Typed {
                text: "/compact".into(),
            },
            EventKind::Ended,
            EventKind::Workspace {
                path: "/Users/jan/scratch".into(),
                branch: None,
                slot: None,
                window: None,
                agent: "claude".into(),
                base: None,
            },
            EventKind::Workspace {
                path: "/repo/.saaio/worktrees/shop-222".into(),
                branch: Some("flow/shop-222".into()),
                slot: Some(3),
                window: None,
                agent: "claude".into(),
                base: Some("flow/shop-221".into()),
            },
            EventKind::BaseMerged {
                base: "flow/shop-221".into(),
                onto: "main".into(),
            },
            EventKind::Services {
                status: ServiceStatus::Paused,
                reason: None,
            },
            EventKind::Services {
                status: ServiceStatus::ResumeFailed,
                reason: Some("port 8003 is taken".into()),
            },
        ]
    }

    fn color() -> Question {
        Question {
            header: "Color".into(),
            question: "Which color?".into(),
            options: vec![
                Choice {
                    label: "Red".into(),
                    description: "A warm, bold color".into(),
                },
                Choice {
                    label: "Blue".into(),
                    description: "A cool, steady color".into(),
                },
            ],
            multi_select: false,
        }
    }

    #[test]
    fn every_kind_survives_a_round_trip_through_one_line() {
        for kind in every_kind() {
            let event = Event::new(1_791_945_698_308, kind);
            let line = event.to_line();
            assert!(!line.contains('\n'));
            assert_eq!(Event::from_line(&line), Some(event));
        }
    }

    #[test]
    fn writes_the_version_time_and_kind_as_flat_fields() {
        let event = Event::new(
            7,
            EventKind::Blocked {
                reason: "no default".into(),
            },
        );

        assert_eq!(
            event.to_line(),
            r#"{"v":1,"at":7,"kind":"blocked","reason":"no default"}"#
        );
        assert_eq!(
            Event::new(
                7,
                EventKind::Phase {
                    phase: phase("merge-request")
                }
            )
            .to_line(),
            r#"{"v":1,"at":7,"kind":"phase","phase":"merge-request"}"#
        );
        assert_eq!(
            Event::new(
                7,
                EventKind::Typed {
                    text: "/compact".into()
                }
            )
            .to_line(),
            r#"{"v":1,"at":7,"kind":"typed","text":"/compact"}"#
        );
    }

    #[test]
    fn writes_services_with_a_reason_only_when_one_failed() {
        let services = |status, reason: Option<&str>| {
            Event::new(
                7,
                EventKind::Services {
                    status,
                    reason: reason.map(str::to_string),
                },
            )
            .to_line()
        };

        assert_eq!(
            services(ServiceStatus::Paused, None),
            r#"{"v":1,"at":7,"kind":"services","status":"paused"}"#
        );
        assert_eq!(
            services(ServiceStatus::ResumeFailed, Some("port 8003 is taken")),
            r#"{"v":1,"at":7,"kind":"services","status":"resume-failed","reason":"port 8003 is taken"}"#
        );
        for status in [
            ServiceStatus::Pausing,
            ServiceStatus::Paused,
            ServiceStatus::Resuming,
            ServiceStatus::Running,
            ServiceStatus::PauseFailed,
            ServiceStatus::ResumeFailed,
        ] {
            assert_eq!(serde_json::to_value(status).unwrap(), status.name());
        }
    }

    #[test]
    fn cuts_a_typed_line_to_the_long_limit() {
        let long = "x".repeat(LONG_TEXT_LIMIT + 5);

        let event = Event::new(0, EventKind::Typed { text: long });

        let EventKind::Typed { text } = event.kind else {
            panic!("kind changed");
        };
        assert_eq!(text.chars().count(), LONG_TEXT_LIMIT);
    }

    #[test]
    fn cuts_every_text_to_the_limit_in_characters_but_the_request() {
        let long = "ä".repeat(TEXT_LIMIT + 20);
        let event = Event::new(
            0,
            EventKind::Started {
                name: long.clone(),
                request: long.clone(),
                mode: Mode::Guided,
                phases: None,
            },
        );

        let EventKind::Started { name, request, .. } = event.kind else {
            panic!("kind changed");
        };
        assert_eq!(name.chars().count(), TEXT_LIMIT);
        assert_eq!(
            request, long,
            "the request is the worker's prompt, kept whole"
        );

        let short = Event::new(0, EventKind::waiting("ok?"));
        assert_eq!(short.kind, EventKind::waiting("ok?"));
    }

    #[test]
    fn cuts_every_text_of_a_dialog_to_the_limit() {
        let long = "x".repeat(TEXT_LIMIT + 5);
        let mut asked = color();
        asked.header = long.clone();
        asked.question = long.clone();
        asked.options[0].label = long.clone();
        asked.options[1].description = long.clone();

        let event = Event::new(
            0,
            EventKind::Waiting {
                question: long,
                questions: vec![asked],
                permission: false,
            },
        );

        let EventKind::Waiting {
            question,
            questions,
            ..
        } = event.kind
        else {
            panic!("kind changed");
        };
        let asked = &questions[0];
        for text in [
            &question,
            &asked.header,
            &asked.question,
            &asked.options[0].label,
            &asked.options[1].description,
        ] {
            assert_eq!(text.chars().count(), TEXT_LIMIT);
        }
    }

    #[test]
    fn keeps_no_control_character_and_at_most_8_questions_of_8_options() {
        let dirty = "a\x1b[2Jb\u{9b}c\x07\r\nd";
        let asked = Question {
            header: dirty.into(),
            question: dirty.into(),
            options: (0..50)
                .map(|_| Choice {
                    label: dirty.into(),
                    description: dirty.into(),
                })
                .collect(),
            multi_select: false,
        };

        let event = Event::new(
            0,
            EventKind::Waiting {
                question: dirty.into(),
                questions: vec![asked; 20],
                permission: false,
            },
        );

        let EventKind::Waiting {
            question,
            questions,
            ..
        } = event.kind
        else {
            panic!("kind changed");
        };
        assert_eq!(question, "a[2Jbc\nd");
        assert_eq!(questions.len(), MAX_QUESTIONS);
        for asked in &questions {
            assert_eq!(asked.options.len(), MAX_QUESTIONS);
            assert_eq!(
                (asked.header.as_str(), asked.question.as_str()),
                ("a[2Jbc\nd", "a[2Jbc\nd")
            );
            for choice in &asked.options {
                assert_eq!(
                    (choice.label.as_str(), choice.description.as_str()),
                    ("a[2Jbc\nd", "a[2Jbc\nd")
                );
            }
        }
    }

    #[test]
    fn a_text_keeps_its_lines_and_tabs_but_no_other_control_character() {
        let dirty = "Done.\n\n\tcode\x1b[31m red\u{9b}2J\r\n- next\x07";

        for kind in [
            EventKind::Answer { text: dirty.into() },
            EventKind::Message { text: dirty.into() },
            EventKind::Typed { text: dirty.into() },
        ] {
            let (EventKind::Answer { text }
            | EventKind::Message { text }
            | EventKind::Typed { text }) = Event::new(0, kind).kind
            else {
                panic!("kind changed");
            };
            assert_eq!(text, "Done.\n\n\tcode[31m red2J\n- next");
        }
        let EventKind::Blocked { reason } = Event::new(
            0,
            EventKind::Blocked {
                reason: dirty.into(),
            },
        )
        .kind
        else {
            panic!("kind changed");
        };
        assert_eq!(reason, "Done.\n\n\tcode[31m red2J\n- next");
    }

    #[test]
    fn writes_a_dialog_in_camel_case_and_reads_a_waiting_line_without_one() {
        let line = Event::new(
            7,
            EventKind::Waiting {
                question: "Which color?".into(),
                questions: vec![color()],
                permission: false,
            },
        )
        .to_line();
        assert!(line.contains(r#""multiSelect":false"#), "{line}");

        let old = r#"{"v":1,"at":7,"kind":"waiting","question":"ok?"}"#;
        assert_eq!(
            Event::from_line(old).map(|event| event.kind),
            Some(EventKind::waiting("ok?"))
        );
        assert_eq!(
            Event::new(7, Event::from_line(old).unwrap().kind).to_line(),
            old
        );
    }

    #[test]
    fn writes_a_permission_only_when_the_harness_asks_one() {
        let asked = EventKind::Waiting {
            question: "permission: Bash(ls)".into(),
            questions: Vec::new(),
            permission: true,
        };
        let line = Event::new(7, asked.clone()).to_line();
        assert_eq!(
            line,
            r#"{"v":1,"at":7,"kind":"waiting","question":"permission: Bash(ls)","permission":true}"#
        );
        assert_eq!(Event::from_line(&line).map(|event| event.kind), Some(asked));

        let worded = Event::new(7, EventKind::waiting("ok?")).to_line();
        assert_eq!(
            worded,
            r#"{"v":1,"at":7,"kind":"waiting","question":"ok?"}"#
        );
        let old = r#"{"v":1,"at":7,"kind":"waiting","question":"permission: Bash(ls)"}"#;
        assert_eq!(
            Event::from_line(old).map(|event| event.kind),
            Some(EventKind::waiting("permission: Bash(ls)"))
        );
    }

    #[test]
    fn reads_a_workspace_line_written_before_plain_sessions() {
        let line = r#"{"v":1,"at":7,"kind":"workspace","path":"/repo/.saaio/worktrees/shop-221","branch":"flow/shop-221","slot":2,"window":"saaio-ui","agent":"claude"}"#;

        let Some(Event {
            kind: EventKind::Workspace {
                branch, slot, base, ..
            },
            ..
        }) = Event::from_line(line)
        else {
            panic!("an old workspace line parses");
        };
        assert_eq!(branch.as_deref(), Some("flow/shop-221"));
        assert_eq!(slot, Some(2));
        assert_eq!(base, None);
        assert_eq!(Event::from_line(line).unwrap().to_line(), line);
    }

    #[test]
    fn reads_a_started_line_written_before_phases_were_recorded() {
        let line =
            r#"{"v":1,"at":7,"kind":"started","name":"shop-221","request":"x","mode":"guided"}"#;

        let Some(Event {
            kind: EventKind::Started { phases, .. },
            ..
        }) = Event::from_line(line)
        else {
            panic!("an old started line parses");
        };
        assert_eq!(phases, None);
        let plain = Event::new(
            7,
            EventKind::Started {
                name: "scratch".into(),
                request: String::new(),
                mode: Mode::Plain,
                phases: None,
            },
        );
        assert!(!plain.to_line().contains("phases"), "left out when None");
    }

    #[test]
    fn plain_is_not_a_mode_flag_takes() {
        assert!(Mode::from_str("plain", false).is_err());
        assert_eq!(Mode::from_str("auto", false), Ok(Mode::Auto));
    }

    #[test]
    fn skips_broken_lines_other_versions_and_unknown_kinds() {
        assert_eq!(Event::from_line("not json"), None);
        assert_eq!(Event::from_line(r#"{"v":1,"at":7"#), None);
        assert_eq!(Event::from_line(r#"{"v":2,"at":7,"kind":"working"}"#), None);
        assert_eq!(
            Event::from_line(r#"{"v":1,"at":7,"kind":"teleported"}"#),
            None
        );
        assert_eq!(
            Event::from_line(r#"{"v":1,"at":7,"kind":"working"}"#),
            Some(Event {
                v: 1,
                at: 7,
                kind: EventKind::Working
            })
        );
    }
}
