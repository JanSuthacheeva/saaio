//! The parsers' properties as oracles: each takes any input, runs a parser on it and panics when
//! a property breaks. `cargo test` drives them with proptest, the targets in `.fuzz/` with
//! libFuzzer.

use std::fmt::Debug;
use std::io::Cursor;

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::event::{EVENT_VERSION, Event};
use crate::fold;
use crate::id::FlowId;
use crate::proto::{self, ClientMsg, Error, MAX_MESSAGE, ServerMsg};

/// A clock a little past today, for folding: logs are read long after they were written.
const NOW: u64 = 1_800_000_000_000;

/// Any text as a log line: an event it reads as writes one line that reads back as the same
/// event, at EVENT_VERSION.
///
/// # Panics
///
/// When the event does not survive its line.
pub fn log_line(text: &str) {
    let Some(event) = Event::from_line(text) else {
        return;
    };
    assert_eq!(event.v, EVENT_VERSION, "{text:?}");
    let line = event.to_line();
    assert!(!line.contains('\n'), "a line break in {line:?}");
    assert_eq!(Event::from_line(&line), Some(event), "{text:?} as {line:?}");
}

/// Any bytes as a whole log: every event it reads keeps log_line's property, and the events fold.
///
/// # Panics
///
/// When an event does not survive its line, or folding panics.
pub fn log(bytes: &[u8]) {
    let events = Event::from_log(bytes);
    for event in &events {
        log_line(&event.to_line());
    }
    let id = FlowId::parse("01a14000-0000-7000-8000-000000000000").expect("a flow id's text");
    fold::fold(id, &events, None, NOW);
    fold::fold(id, &events, Some(NOW), NOW);
}

/// Bytes as a stream of frames, read as ClientMsg and again as ServerMsg: each read ends as its
/// header says (End, Cut, TooLarge(len)) or as the body decodes (a message that survives its
/// frame, or Decode, which ends the stream); never Io or Encode.
///
/// # Panics
///
/// When a read ends otherwise than its header says, or a message changes on its way through
/// another frame.
pub fn frames(bytes: &[u8]) {
    read_all::<ClientMsg>(bytes);
    read_all::<ServerMsg>(bytes);
}

/// What the header in front of the rest of a stream says the next read must end as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Header {
    /// Nothing left: the stream ended between messages.
    End,
    /// The stream ends inside the length or the body.
    Cut,
    TooLarge(usize),
    /// A whole body of this length follows.
    Body(usize),
}

fn header(rest: &[u8]) -> Header {
    let Some((len, body)) = rest.split_first_chunk::<4>() else {
        return if rest.is_empty() {
            Header::End
        } else {
            Header::Cut
        };
    };
    let len = u32::from_le_bytes(*len) as usize;
    if len > MAX_MESSAGE {
        Header::TooLarge(len)
    } else if body.len() < len {
        Header::Cut
    } else {
        Header::Body(len)
    }
}

/// One message type read over the whole stream, each read checked against its header.
fn read_all<T: Serialize + DeserializeOwned + PartialEq + Debug>(bytes: &[u8]) {
    let mut cursor = Cursor::new(bytes);
    loop {
        let at = cursor.position() as usize;
        let header = header(&bytes[at..]);
        let Some(message) = settle(header, proto::read_msg::<_, T>(&mut cursor)) else {
            return;
        };
        if let Header::Body(len) = header {
            assert_eq!(cursor.position() as usize, at + 4 + len, "{message:?}");
        }
        let again = proto::encode(&message).expect("a decoded message encodes");
        let back = proto::read_msg::<_, T>(&mut again.as_slice()).expect("its frame reads");
        assert_eq!(
            back.as_ref(),
            Some(&message),
            "a message changed in its frame"
        );
    }
}

/// The message a read gave as its header promised, or None when the read ended the stream as
/// its header says.
fn settle<T: Debug>(header: Header, read: Result<Option<T>, Error>) -> Option<T> {
    match (header, read) {
        (Header::Body(_), Ok(Some(message))) => Some(message),
        (Header::End, Ok(None))
        | (Header::Cut, Err(Error::Cut))
        | (Header::Body(_), Err(Error::Decode(_))) => None,
        (Header::TooLarge(len), Err(Error::TooLarge(read))) if read == len => None,
        (header, read) => panic!("the header said {header:?}, the read gave {read:?}"),
    }
}

#[cfg(test)]
mod tests {
    use std::io;

    use super::*;

    fn frame(len: u32, body: &[u8]) -> Vec<u8> {
        let mut bytes = len.to_le_bytes().to_vec();
        bytes.extend_from_slice(body);
        bytes
    }

    #[test]
    fn reads_each_header_as_the_end_it_promises() {
        let too_large = MAX_MESSAGE as u32 + 1;

        assert_eq!(header(&[]), Header::End);
        assert_eq!(header(&[3, 0]), Header::Cut);
        assert_eq!(header(&frame(3, &[1, 2])), Header::Cut);
        assert_eq!(
            header(&frame(too_large, &[])),
            Header::TooLarge(too_large as usize)
        );
        assert_eq!(header(&frame(2, &[1, 2, 3])), Header::Body(2));
        assert_eq!(header(&frame(0, &[])), Header::Body(0));
    }

    #[test]
    fn a_stream_of_every_ending_passes() {
        let message = proto::encode(&ClientMsg::StopServer).unwrap();
        let mut stream = message.clone();
        stream.extend(&message);

        frames(&[]);
        frames(&stream);
        frames(&[stream.as_slice(), &[3, 0]].concat());
        frames(&[stream.as_slice(), &frame(MAX_MESSAGE as u32 + 1, &[])].concat());
        frames(&frame(1, &[0xff]));
    }

    #[test]
    fn settles_a_read_that_ends_as_its_header_says() {
        let decode = postcard::from_bytes::<u8>(&[]).unwrap_err();

        assert_eq!(settle(Header::Body(1), Ok(Some(7))), Some(7));
        assert_eq!(settle::<u8>(Header::End, Ok(None)), None);
        assert_eq!(settle::<u8>(Header::Cut, Err(Error::Cut)), None);
        assert_eq!(
            settle::<u8>(Header::TooLarge(9), Err(Error::TooLarge(9))),
            None
        );
        assert_eq!(
            settle::<u8>(Header::Body(1), Err(Error::Decode(decode))),
            None
        );
    }

    #[test]
    #[should_panic(expected = "the header said Body(1)")]
    fn a_read_that_fails_otherwise_breaks_the_property() {
        settle::<u8>(Header::Body(1), Err(Error::Io(io::Error::other("disk"))));
    }

    #[test]
    #[should_panic(expected = "the header said TooLarge(9)")]
    fn a_read_that_names_another_length_breaks_the_property() {
        settle::<u8>(Header::TooLarge(9), Err(Error::TooLarge(8)));
    }
}
