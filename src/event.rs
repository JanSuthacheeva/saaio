use clap::ValueEnum;
use serde::{Deserialize, Serialize};

pub const EVENT_VERSION: u32 = 1;
pub const TEXT_LIMIT: usize = 500;
/// The longest message or answer: what the master tells a worker, a turn's whole closing message.
pub const LONG_TEXT_LIMIT: usize = 4000;

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
}

impl Event {
    /// An event of the current version at `at` (Unix ms), each text cut to TEXT_LIMIT chars.
    pub fn new(at: u64, kind: EventKind) -> Event {
        Event {
            v: EVENT_VERSION,
            at,
            kind: kind.with_texts_cut(),
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
    fn with_texts_cut(self) -> EventKind {
        match self {
            EventKind::Started {
                name,
                request,
                mode,
            } => EventKind::Started {
                name: cut(name),
                request: cut(request),
                mode,
            },
            EventKind::Delegated { description } => EventKind::Delegated {
                description: cut(description),
            },
            EventKind::Waiting { question } => EventKind::Waiting {
                question: cut(question),
            },
            EventKind::Idle { note } => EventKind::Idle { note: cut(note) },
            EventKind::Blocked { reason } => EventKind::Blocked {
                reason: cut(reason),
            },
            EventKind::Decision { text } => EventKind::Decision { text: cut(text) },
            EventKind::MergeRequest { url } => EventKind::MergeRequest { url: cut(url) },
            EventKind::Message { text } => EventKind::Message {
                text: cut_to(text, LONG_TEXT_LIMIT),
            },
            EventKind::Answer { text } => EventKind::Answer {
                text: cut_to(text, LONG_TEXT_LIMIT),
            },
            other => other,
        }
    }
}

fn cut(text: String) -> String {
    cut_to(text, TEXT_LIMIT)
}

fn cut_to(text: String, limit: usize) -> String {
    match text.char_indices().nth(limit) {
        Some((end, _)) => text[..end].to_string(),
        None => text,
    }
}

impl Mode {
    /// The name the command line and the logs use: guided, auto.
    pub fn name(self) -> &'static str {
        match self {
            Mode::Guided => "guided",
            Mode::Auto => "auto",
        }
    }

    pub fn toggled(self) -> Mode {
        match self {
            Mode::Guided => Mode::Auto,
            Mode::Auto => Mode::Guided,
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
        for mode in [Mode::Guided, Mode::Auto] {
            assert_eq!(serde_json::to_value(mode).unwrap(), mode.name());
            assert_eq!(mode.toggled().toggled(), mode);
        }
        for phase in Phase::ALL {
            assert_eq!(serde_json::to_value(phase).unwrap(), phase.name());
        }
    }

    fn every_kind() -> Vec<EventKind> {
        vec![
            EventKind::Started {
                name: "shop-221".into(),
                request: "SHOP-221 add CSV export".into(),
                mode: Mode::Auto,
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
    fn cuts_every_text_to_the_limit_in_characters() {
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
        assert_eq!(request.chars().count(), TEXT_LIMIT);

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
