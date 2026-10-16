use std::time::{SystemTime, UNIX_EPOCH};

use clap::ValueEnum;
use serde::{Deserialize, Serialize};

pub const EVENT_VERSION: u32 = 1;
pub const TEXT_LIMIT: usize = 500;
/// The longest message or answer: what the master tells a worker, a turn's whole closing message.
pub const LONG_TEXT_LIMIT: usize = 4000;
/// The longest request flow start and flow new take; past it they refuse. Kept whole, since it
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
    /// What the master told this flow's worker, through `flow send`.
    Message {
        text: String,
    },
    /// A turn's whole closing message, for the master to read; Waiting and Idle keep its short form.
    Answer {
        text: String,
    },
    Ended,
    Workspace {
        path: String,
        branch: String,
        slot: u8,
        window: Option<String>,
        agent: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum Phase {
    Design,
    Implement,
    Review,
    MergeRequest,
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
    /// An event of the current version at `at` (Unix ms), each text cut to its limit, except a
    /// Started request: it is the worker's prompt, kept whole (its command checks REQUEST_LIMIT).
    pub fn new(at: u64, mut kind: EventKind) -> Event {
        kind.cut_texts();
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
    /// so a log written by a newer flow still folds.
    pub fn from_line(line: &str) -> Option<Event> {
        serde_json::from_str::<Event>(line)
            .ok()
            .filter(|event| event.v == EVENT_VERSION)
    }
}

impl EventKind {
    fn cut_texts(&mut self) {
        match self {
            EventKind::Started { name: text, .. }
            | EventKind::Delegated { description: text }
            | EventKind::Waiting { question: text }
            | EventKind::Idle { note: text }
            | EventKind::Blocked { reason: text }
            | EventKind::Decision { text }
            | EventKind::MergeRequest { url: text } => cut_to(text, TEXT_LIMIT),
            EventKind::Message { text } | EventKind::Answer { text } => {
                cut_to(text, LONG_TEXT_LIMIT);
            }
            _ => {}
        }
    }
}

fn cut_to(text: &mut String, limit: usize) {
    if let Some((end, _)) = text.char_indices().nth(limit) {
        text.truncate(end);
    }
}

impl Mode {
    /// The name the command line and the logs use: guided, auto, plain.
    pub fn name(self) -> &'static str {
        match self {
            Mode::Guided => "guided",
            Mode::Auto => "auto",
            Mode::Plain => "plain",
        }
    }

    pub fn toggled(self) -> Mode {
        match self {
            Mode::Guided => Mode::Auto,
            Mode::Auto | Mode::Plain => Mode::Guided,
        }
    }
}

impl Phase {
    /// design, implement, review, merge-request - in flow order.
    pub const ALL: [Phase; 4] = [
        Phase::Design,
        Phase::Implement,
        Phase::Review,
        Phase::MergeRequest,
    ];

    /// Where the phase stands in ALL.
    pub fn index(self) -> usize {
        match self {
            Phase::Design => 0,
            Phase::Implement => 1,
            Phase::Review => 2,
            Phase::MergeRequest => 3,
        }
    }

    /// The name the command line and the logs use: design, implement, review, merge-request.
    pub fn name(self) -> &'static str {
        match self {
            Phase::Design => "design",
            Phase::Implement => "implement",
            Phase::Review => "review",
            Phase::MergeRequest => "merge-request",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_modes_and_phases_as_the_log_does() {
        for mode in [Mode::Guided, Mode::Auto, Mode::Plain] {
            assert_eq!(serde_json::to_value(mode).unwrap(), mode.name());
        }
        for (index, phase) in Phase::ALL.into_iter().enumerate() {
            assert_eq!(serde_json::to_value(phase).unwrap(), phase.name());
            assert_eq!(phase.index(), index);
        }
    }

    fn every_kind() -> Vec<EventKind> {
        vec![
            EventKind::Started {
                name: "shop-221".into(),
                request: "SHOP-221 add CSV export".into(),
                mode: Mode::Auto,
            },
            EventKind::Started {
                name: "scratch".into(),
                request: String::new(),
                mode: Mode::Plain,
            },
            EventKind::Phase {
                phase: Phase::MergeRequest,
            },
            EventKind::Working,
            EventKind::Delegated {
                description: "implement plan".into(),
            },
            EventKind::Waiting {
                question: "Approve the plan?".into(),
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
            EventKind::Ended,
        ]
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
                    phase: Phase::MergeRequest
                }
            )
            .to_line(),
            r#"{"v":1,"at":7,"kind":"phase","phase":"merge-request"}"#
        );
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

        let short = Event::new(
            0,
            EventKind::Waiting {
                question: "ok?".into(),
            },
        );
        assert_eq!(
            short.kind,
            EventKind::Waiting {
                question: "ok?".into()
            }
        );
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
