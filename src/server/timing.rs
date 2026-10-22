//! `Timing`: how long the server waits before it types, calls a start stalled, lets a line go or
//! closes a silent connection

use std::time::Duration;

/// How long the server waits before it acts on a session; `default` is what saaio runs with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timing {
    /// No key into a pane for this long before the server types into it, so nothing lands in the
    /// middle of what the person types there.
    pub quiet: Duration,
    /// A Codex worker with no first turn this long shows waiting.
    pub first_turn_grace: Duration,
    /// A typed line that began no turn this long lets the next one follow: a command such as
    /// /cost runs no hook.
    pub unstarted: Duration,
    /// A connection that says no hello this long is closed.
    pub hello: Duration,
}

impl Default for Timing {
    fn default() -> Timing {
        Timing {
            quiet: Duration::from_secs(10),
            first_turn_grace: Duration::from_secs(30),
            unstarted: Duration::from_secs(30),
            hello: Duration::from_secs(5),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waits_seconds_by_default() {
        assert_eq!(
            Timing::default(),
            Timing {
                quiet: Duration::from_secs(10),
                first_turn_grace: Duration::from_secs(30),
                unstarted: Duration::from_secs(30),
                hello: Duration::from_secs(5),
            }
        );
    }
}
