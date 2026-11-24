//! The parsers' properties as oracles: each takes any input, runs a parser on it and panics when
//! a property breaks. `cargo test` drives them with proptest, the targets in `.fuzz/` with
//! libFuzzer.

use crate::event::{EVENT_VERSION, Event};
use crate::fold;
use crate::id::FlowId;

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

/// Any text as a whole log: every event it reads keeps log_line's property, and the events fold.
///
/// # Panics
///
/// When an event does not survive its line, or folding panics.
pub fn log(text: &str) {
    let events = Event::from_log(text);
    for event in &events {
        log_line(&event.to_line());
    }
    let id = FlowId::parse("01a14000-0000-7000-8000-000000000000").expect("a flow id's text");
    fold::fold(id, &events, None, NOW);
    fold::fold(id, &events, Some(NOW), NOW);
}
