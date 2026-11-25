//! OSC 7501, the Program Status Protocol: a program's reports of where it stands, read from its
//! output, and XTVERSION, the program asking which terminal it runs in (pure)

use base64::Engine as _;
use base64::engine::DecodePaddingMode;
use base64::engine::general_purpose::{GeneralPurpose, GeneralPurposeConfig};

/// What a terminal that supports the protocol answers to `ESC ] 7501 ; ? ESC \`.
pub const QUERY_ANSWER: &[u8] =
    b"\x1b]7501;?:states=idle,working,done,blocked,error:kinds=permission,question,auth\x1b\\";
/// What saaio answers to XTVERSION (`CSI > q`, `CSI > 0 q`): its name and version.
pub const VERSION_ANSWER: &[u8] =
    concat!("\x1bP>|saaio ", env!("CARGO_PKG_VERSION"), "\x1b\\").as_bytes();
/// The longest sequence the spec allows; a longer one is dropped whole.
const MAX_SEQUENCE: usize = 4096;
const PREFIX: &[u8] = b"7501;";
const ESC: u8 = 0x1b;
const BEL: u8 = 0x07;
const CAN: u8 = 0x18;
const SUB: u8 = 0x1a;

/// `msg` is standard base64 with its padding optional.
const MESSAGE: GeneralPurpose = GeneralPurpose::new(
    &base64::alphabet::STANDARD,
    GeneralPurposeConfig::new().with_decode_padding_mode(DecodePaddingMode::Indifferent),
);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Idle,
    Working,
    Blocked,
    Done,
    Error,
}

/// What a blocked program needs from the person (`kind`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Need {
    Permission,
    Question,
    Auth,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub status: Status,
    /// Only with Blocked; an unknown kind is None.
    pub need: Option<Need>,
    /// `msg`, decoded.
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Sequence {
    Query,
    /// XTVERSION: the program asks the terminal's name and version.
    Version,
    /// A report, or None for `clear`.
    Status(Option<Report>),
}

/// One sequence in a program's output, with the offset just past its terminator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub end: usize,
    pub sequence: Sequence,
}

/// Finds OSC 7501 sequences and XTVERSION queries in a program's output across reads.
#[derive(Debug, Default)]
pub struct Scanner {
    state: Scan,
    body: Vec<u8>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum Scan {
    #[default]
    Ground,
    /// After an ESC.
    Escape,
    /// In an OSC that may be ours, its body so far in `body`.
    Osc,
    /// After an ESC in an OSC: a `\\` ends it, anything else aborts it.
    OscEscape,
    /// In an OSC that is not ours or grew too long, until its end.
    Skip,
    /// After `ESC [`: the bytes of a CSI that may be XTVERSION so far (`>`, then `0` or none),
    /// in `body`.
    Csi,
}

impl Scanner {
    /// The sequences that end in `bytes`, with the offset just past each one's terminator;
    /// a sequence cut off at the end of `bytes` is kept for the next call.
    pub fn scan(&mut self, bytes: &[u8]) -> Vec<Found> {
        let mut found = Vec::new();
        for (at, &byte) in bytes.iter().enumerate() {
            let sequence = match self.state {
                Scan::Ground => {
                    if byte == ESC {
                        self.state = Scan::Escape;
                    }
                    None
                }
                Scan::Escape => {
                    self.escape(byte);
                    None
                }
                Scan::Osc => {
                    if self.osc(byte) {
                        self.ended()
                    } else {
                        None
                    }
                }
                Scan::OscEscape if byte == b'\\' => {
                    self.state = Scan::Ground;
                    self.ended()
                }
                Scan::OscEscape => {
                    self.escape(byte);
                    None
                }
                Scan::Skip => {
                    self.state = match byte {
                        ESC => Scan::Escape,
                        BEL | CAN | SUB => Scan::Ground,
                        _ => Scan::Skip,
                    };
                    None
                }
                Scan::Csi => self.csi(byte),
            };
            if let Some(sequence) = sequence {
                found.push(Found {
                    end: at + 1,
                    sequence,
                });
            }
        }
        found
    }

    /// The byte after an ESC: `]` opens an OSC, `[` a CSI, another ESC waits again.
    fn escape(&mut self, byte: u8) {
        self.state = match byte {
            b']' => {
                self.body.clear();
                Scan::Osc
            }
            b'[' => {
                self.body.clear();
                Scan::Csi
            }
            ESC => Scan::Escape,
            _ => Scan::Ground,
        };
    }

    /// The OSC that just ended, when it is ours and reads.
    fn ended(&self) -> Option<Sequence> {
        parse(self.body.strip_prefix(PREFIX)?)
    }

    /// One byte of a CSI that may be XTVERSION: `>`, then at most one `0`, then `q` ends it.
    fn csi(&mut self, byte: u8) -> Option<Sequence> {
        match (self.body.as_slice(), byte) {
            ([], b'>') | ([b'>'], b'0') => self.body.push(byte),
            ([b'>'] | [b'>', b'0'], b'q') => {
                self.state = Scan::Ground;
                return Some(Sequence::Version);
            }
            (_, ESC) => self.state = Scan::Escape,
            _ => self.state = Scan::Ground,
        }
        None
    }

    /// One byte of an OSC's body; true when it ends the sequence (BEL).
    fn osc(&mut self, byte: u8) -> bool {
        match byte {
            BEL => {
                self.state = Scan::Ground;
                return true;
            }
            ESC => self.state = Scan::OscEscape,
            CAN | SUB => self.state = Scan::Ground,
            _ => {
                self.body.push(byte);
                let ours = PREFIX.starts_with(&self.body) || self.body.starts_with(PREFIX);
                if !ours || self.body.len() > MAX_SEQUENCE {
                    self.body.clear();
                    self.state = Scan::Skip;
                }
            }
        }
        false
    }
}

impl Report {
    /// Idle, done and error: the program waits for the person.
    pub fn at_rest(&self) -> bool {
        matches!(self.status, Status::Idle | Status::Done | Status::Error)
    }
}

/// One OSC 7501 body (between `7501;` and the terminator): None when it breaks the spec
/// (no or unknown state, bad base64, a control character, an `id` of a child record).
pub fn parse(body: &[u8]) -> Option<Sequence> {
    if body.iter().any(u8::is_ascii_control) {
        return None;
    }
    let body = std::str::from_utf8(body).ok()?;
    if body.trim() == "?" {
        return Some(Sequence::Query);
    }
    let mut state = None;
    let mut kind = None;
    let mut msg = None;
    let mut id = false;
    for pair in body.split(':') {
        let Some((key, value)) = pair.split_once('=') else {
            continue;
        };
        let value = value.trim();
        match key.trim() {
            "state" => state = Some(value),
            "kind" => kind = Some(value),
            "msg" => msg = Some(value),
            "id" => id = true,
            _ => {}
        }
    }
    if id {
        return None;
    }
    let status = match state? {
        "clear" => return Some(Sequence::Status(None)),
        "idle" => Status::Idle,
        "working" => Status::Working,
        "blocked" => Status::Blocked,
        "done" => Status::Done,
        "error" => Status::Error,
        _ => return None,
    };
    let need = match (status, kind) {
        (Status::Blocked, Some("permission")) => Some(Need::Permission),
        (Status::Blocked, Some("question")) => Some(Need::Question),
        (Status::Blocked, Some("auth")) => Some(Need::Auth),
        _ => None,
    };
    let message = match msg {
        Some(encoded) => Some(message(encoded)?),
        None => None,
    };
    Some(Sequence::Status(Some(Report {
        status,
        need,
        message,
    })))
}

/// `msg` decoded; None when it is not base64 of UTF-8 without control characters.
fn message(encoded: &str) -> Option<String> {
    let bytes = MESSAGE.decode(encoded).ok()?;
    let text = String::from_utf8(bytes).ok()?;
    (!text.chars().any(char::is_control)).then_some(text)
}

/// `msg` as a program writes it, for tests.
#[cfg(test)]
pub fn encoded(text: &str) -> String {
    MESSAGE.encode(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(body: &str) -> Option<Report> {
        match parse(body.as_bytes()) {
            Some(Sequence::Status(report)) => report,
            other => panic!("{body}: {other:?}"),
        }
    }

    fn scan(bytes: &[u8]) -> Vec<Found> {
        Scanner::default().scan(bytes)
    }

    fn idle() -> Sequence {
        Sequence::Status(Some(Report {
            status: Status::Idle,
            need: None,
            message: None,
        }))
    }

    #[test]
    fn finds_a_report_ended_by_st_or_bel_where_it_ends() {
        assert_eq!(
            scan(b"ab\x1b]7501;state=idle\x1b\\cd"),
            vec![Found {
                end: 21,
                sequence: idle()
            }]
        );
        assert_eq!(
            scan(b"\x1b]7501;?\x07\x1b]7501;state=idle\x07"),
            vec![
                Found {
                    end: 9,
                    sequence: Sequence::Query
                },
                Found {
                    end: 27,
                    sequence: idle()
                },
            ]
        );
    }

    #[test]
    fn finds_a_sequence_split_anywhere_across_reads() {
        let bytes = b"x\x1b]7501;state=idle\x1b\\y";
        for split in 0..=bytes.len() {
            let mut scanner = Scanner::default();
            let (first, second) = bytes.split_at(split);

            let mut found = scanner.scan(first);
            found.extend(scanner.scan(second).into_iter().map(|mut found| {
                found.end += split;
                found
            }));

            assert_eq!(
                found,
                vec![Found {
                    end: 20,
                    sequence: idle()
                }],
                "split at {split}"
            );
        }
    }

    #[test]
    fn ignores_other_sequences_and_text() {
        assert_eq!(
            scan(b"\x1b]52;c;aGk=\x07\x1b]0;7501;state=idle\x07"),
            vec![]
        );
        assert_eq!(
            scan(b"7501;state=idle\x1b\\\x1b[7501;state=idle\x07"),
            vec![]
        );
        assert_eq!(scan(b"\x1b]75011;state=idle\x07"), vec![]);
    }

    #[test]
    fn an_esc_or_can_aborts_the_sequence_and_an_esc_starts_the_next() {
        assert_eq!(scan(b"\x1b]7501;state=working\x18state=idle\x07"), vec![]);
        assert_eq!(scan(b"\x1b]7501;state=working\x1a\x07"), vec![]);
        assert_eq!(
            scan(b"\x1b]7501;state=working\x1b]7501;state=idle\x07"),
            vec![Found {
                end: 38,
                sequence: idle()
            }]
        );
        assert_eq!(scan(b"\x1b]7501;state=working\x1bxstate=idle\x07"), vec![]);
    }

    #[test]
    fn drops_a_sequence_longer_than_the_spec_allows_and_finds_the_next() {
        let long = format!("\x1b]7501;state=idle:app={}\x07", "a".repeat(MAX_SEQUENCE));
        let fits = format!(
            "\x1b]7501;state=idle:app={}\x07",
            "a".repeat(MAX_SEQUENCE - 20)
        );
        let next = format!("{long}\x1b]7501;state=idle\x07");

        assert_eq!(scan(long.as_bytes()), vec![]);
        assert_eq!(scan(fits.as_bytes()).len(), 1);
        assert_eq!(
            scan(next.as_bytes()),
            vec![Found {
                end: next.len(),
                sequence: idle()
            }]
        );
    }

    #[test]
    fn the_query_answer_says_every_state_and_kind() {
        let answer = std::str::from_utf8(QUERY_ANSWER).unwrap();

        assert!(answer.starts_with("\x1b]7501;?:"));
        assert!(answer.ends_with("\x1b\\"));
        for word in [
            "idle",
            "working",
            "done",
            "blocked",
            "error",
            "permission",
            "question",
            "auth",
        ] {
            assert!(answer.contains(word), "{word}");
        }
    }

    #[test]
    fn reads_each_state() {
        for (text, status) in [
            ("idle", Status::Idle),
            ("working", Status::Working),
            ("blocked", Status::Blocked),
            ("done", Status::Done),
            ("error", Status::Error),
        ] {
            let read = report(&format!("state={text}:app=claude-code")).unwrap();
            assert_eq!(read.status, status);
            assert_eq!(read.need, None);
            assert_eq!(read.message, None);
        }
    }

    #[test]
    fn idle_done_and_error_are_at_rest() {
        let at_rest = |text: &str| report(&format!("state={text}")).unwrap().at_rest();

        assert!(at_rest("idle"));
        assert!(at_rest("done"));
        assert!(at_rest("error"));
        assert!(!at_rest("working"));
        assert!(!at_rest("blocked"));
    }

    #[test]
    fn reads_the_query_and_a_clear() {
        assert_eq!(parse(b"?"), Some(Sequence::Query));
        assert_eq!(parse(b"state=clear"), Some(Sequence::Status(None)));
    }

    #[test]
    fn reads_what_a_blocked_program_needs_and_only_then() {
        let need = |body: &str| report(body).unwrap().need;

        assert_eq!(
            need("state=blocked:kind=permission"),
            Some(Need::Permission)
        );
        assert_eq!(need("state=blocked:kind=question"), Some(Need::Question));
        assert_eq!(need("state=blocked:kind=auth"), Some(Need::Auth));
        assert_eq!(need("state=blocked:kind=coffee"), None);
        assert_eq!(need("state=working:kind=permission"), None);
    }

    #[test]
    fn decodes_the_message_with_or_without_padding() {
        let message = |body: &str| report(body).unwrap().message;

        assert_eq!(
            message(&format!("state=blocked:msg={}", encoded("Allow Bash?"))),
            Some("Allow Bash?".to_string())
        );
        assert_eq!(message("state=done:msg=aGk="), Some("hi".to_string()));
        assert_eq!(message("state=done:msg=aGk"), Some("hi".to_string()));
        assert_eq!(message("state=done:msg="), Some(String::new()));
    }

    #[test]
    fn drops_a_report_whose_message_breaks_the_spec() {
        assert_eq!(parse(b"state=done:msg=***"), None);
        assert_eq!(
            parse(format!("state=done:msg={}", encoded("a\nb")).as_bytes()),
            None
        );
        assert_eq!(parse(b"state=done:msg=/w=="), None, "not UTF-8");
    }

    #[test]
    fn drops_a_body_with_a_control_character() {
        assert_eq!(parse(b"state=idle\n"), None);
        assert_eq!(parse(b"state=\x7fidle"), None);
    }

    #[test]
    fn ignores_unknown_keys_and_malformed_pairs_and_keeps_the_last_of_a_key() {
        let read = report(" state = working : future=1 : junk : state=idle :progress=40").unwrap();

        assert_eq!(read.status, Status::Idle);
    }

    #[test]
    fn ignores_a_report_without_or_with_an_unknown_state() {
        assert_eq!(parse(b"app=claude-code"), None);
        assert_eq!(parse(b"state=sleeping"), None);
        assert_eq!(parse(b""), None);
    }

    #[test]
    fn ignores_a_child_record() {
        assert_eq!(parse(b"state=working:id=task-1"), None);
        assert_eq!(parse(b"state=clear:id=task-1"), None);
    }

    mod properties {
        use proptest::prelude::*;

        use super::super::*;
        use crate::fuzzing;
        use crate::strategies;

        /// A report as a program writes it, with ST or BEL.
        fn osc(report: &Report, bel: bool) -> Vec<u8> {
            let state = match report.status {
                Status::Idle => "idle",
                Status::Working => "working",
                Status::Blocked => "blocked",
                Status::Done => "done",
                Status::Error => "error",
            };
            let mut body = format!("state={state}:app=test");
            if let Some(need) = report.need {
                let kind = match need {
                    Need::Permission => "permission",
                    Need::Question => "question",
                    Need::Auth => "auth",
                };
                body = format!("{body}:kind={kind}");
            }
            if let Some(message) = &report.message {
                body = format!("{body}:msg={}", encoded(message));
            }
            let end = if bel { "\x07" } else { "\x1b\\" };
            format!("\x1b]7501;{body}{end}").into_bytes()
        }

        /// Output between reports: anything but an ESC, which could open a sequence of its own.
        fn filler() -> impl Strategy<Value = Vec<u8>> {
            prop::collection::vec(
                any::<u8>().prop_filter("no ESC", |byte| *byte != ESC),
                0..40,
            )
        }

        proptest! {
            #[test]
            fn finds_every_report_between_any_output(
                parts in prop::collection::vec((filler(), strategies::report(), any::<bool>()), 0..5),
                tail in filler(),
            ) {
                let mut bytes = Vec::new();
                let mut reports = Vec::new();
                for (before, report, bel) in parts {
                    bytes.extend(before);
                    bytes.extend(osc(&report, bel));
                    reports.push(Sequence::Status(Some(report)));
                }
                bytes.extend(tail);

                let found: Vec<Sequence> = Scanner::default()
                    .scan(&bytes)
                    .into_iter()
                    .map(|found| found.sequence)
                    .collect();
                prop_assert_eq!(found, reports);
                fuzzing::program_status(&bytes);
            }

            #[test]
            fn any_output_scans_the_same_split_anywhere(
                bytes in prop::collection::vec(any::<u8>(), 0..200),
            ) {
                fuzzing::program_status(&bytes);
            }
        }
    }
}
