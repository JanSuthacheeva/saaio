//! OSC 7501, the Program Status Protocol: a program's reports of where it stands, read from its
//! output (pure)

use base64::Engine as _;
use base64::engine::general_purpose::{GeneralPurpose, GeneralPurposeConfig};
use base64::engine::DecodePaddingMode;

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
    /// A report, or None for `clear`.
    Status(Option<Report>),
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

/// `msg` as a program writes it, for tests and the fuzz oracle.
#[cfg(any(test, feature = "fuzzing"))]
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
}
