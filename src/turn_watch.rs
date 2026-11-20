//! `TurnWatch`: whether a taken turn is still in use, one look at a time (pure)

use std::time::Duration;

/// How long a turn waits for its run to show before it is let go: the tool was refused or never
/// started. Time the flow spends asking for permission, or the processes cannot be listed, does
/// not count.
pub const GRACE: Duration = Duration::from_secs(10);

/// Quiet looks in a row, after a run was seen, that end it.
const QUIET_LOOKS: u32 = 2;

/// What one look at the flow's processes found.
#[derive(Debug, Clone, Copy)]
pub struct Look {
    /// The flow's agent still runs.
    pub agent_alive: bool,
    /// A process below the agent runs a listed command; None when the processes could not be
    /// listed, which says nothing about the run.
    pub runs: Option<bool>,
    /// The flow is asking for permission.
    pub asks: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Keep {
    Hold,
    Release(Release),
}

/// Why a turn was let go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Release {
    AgentGone,
    RunEnded,
    NeverRan,
}

/// One turn's watch: since when, whether its run was seen, the quiet looks since, and the time
/// not counted against GRACE (asking, or blind).
#[derive(Debug)]
pub struct TurnWatch {
    since: u64,
    last: u64,
    seen: bool,
    quiet: u32,
    paused_ms: u64,
}

impl TurnWatch {
    pub fn new(now: u64) -> TurnWatch {
        TurnWatch {
            since: now,
            last: now,
            seen: false,
            quiet: 0,
            paused_ms: 0,
        }
    }

    /// Hold or release after `look` at `now` (ms): AgentGone at once; RunEnded after two quiet
    /// looks in a row once a run was seen; NeverRan past GRACE with no run seen, the time `asks`
    /// not counted. A look that could not list the processes holds and changes nothing, its
    /// time not counted either.
    pub fn step(&mut self, now: u64, look: Look) -> Keep {
        let between = now.saturating_sub(self.last);
        self.last = now;
        if !look.agent_alive {
            return Keep::Release(Release::AgentGone);
        }
        let Some(runs) = look.runs else {
            self.paused_ms += between;
            return Keep::Hold;
        };
        if runs {
            self.seen = true;
            self.quiet = 0;
            return Keep::Hold;
        }
        if self.seen {
            self.quiet += 1;
            return if self.quiet >= QUIET_LOOKS {
                Keep::Release(Release::RunEnded)
            } else {
                Keep::Hold
            };
        }
        if look.asks {
            self.paused_ms += between;
        }
        let waited = now
            .saturating_sub(self.since)
            .saturating_sub(self.paused_ms);
        if u128::from(waited) > GRACE.as_millis() {
            Keep::Release(Release::NeverRan)
        } else {
            Keep::Hold
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RUNS: Look = Look {
        agent_alive: true,
        runs: Some(true),
        asks: false,
    };
    const QUIET: Look = Look {
        agent_alive: true,
        runs: Some(false),
        asks: false,
    };
    const ASKS: Look = Look {
        agent_alive: true,
        runs: Some(false),
        asks: true,
    };
    const BLIND: Look = Look {
        agent_alive: true,
        runs: None,
        asks: false,
    };
    const GONE: Look = Look {
        agent_alive: false,
        runs: Some(true),
        asks: false,
    };

    #[test]
    fn lets_go_at_once_when_the_agent_is_gone() {
        let mut watch = TurnWatch::new(0);
        assert_eq!(watch.step(500, RUNS), Keep::Hold);
        assert_eq!(watch.step(1_000, GONE), Keep::Release(Release::AgentGone));
    }

    #[test]
    fn holds_while_the_run_lasts_and_lets_go_after_two_quiet_looks() {
        let mut watch = TurnWatch::new(0);
        for at in (500..=60_000).step_by(500) {
            assert_eq!(watch.step(at, RUNS), Keep::Hold, "{at}");
        }
        assert_eq!(watch.step(60_500, QUIET), Keep::Hold);
        assert_eq!(watch.step(61_000, QUIET), Keep::Release(Release::RunEnded));
    }

    #[test]
    fn one_quiet_look_between_runs_keeps_the_turn() {
        let mut watch = TurnWatch::new(0);
        watch.step(500, RUNS);
        assert_eq!(watch.step(1_000, QUIET), Keep::Hold);
        assert_eq!(watch.step(1_500, RUNS), Keep::Hold);
        assert_eq!(watch.step(2_000, QUIET), Keep::Hold);
    }

    #[test]
    fn waits_the_grace_for_a_run_that_never_shows() {
        let mut watch = TurnWatch::new(0);
        assert_eq!(watch.step(10_000, QUIET), Keep::Hold);
        assert_eq!(watch.step(10_500, QUIET), Keep::Release(Release::NeverRan));
    }

    #[test]
    fn a_run_seen_late_within_the_grace_keeps_the_turn() {
        let mut watch = TurnWatch::new(0);
        assert_eq!(watch.step(9_500, QUIET), Keep::Hold);
        assert_eq!(watch.step(10_000, RUNS), Keep::Hold);
        assert_eq!(watch.step(30_000, RUNS), Keep::Hold);
    }

    #[test]
    fn time_spent_asking_does_not_count_against_the_grace() {
        let mut watch = TurnWatch::new(0);
        assert_eq!(watch.step(5_000, QUIET), Keep::Hold);
        for at in (5_500..=60_000).step_by(500) {
            assert_eq!(watch.step(at, ASKS), Keep::Hold, "{at}");
        }
        assert_eq!(watch.step(64_000, QUIET), Keep::Hold);
        assert_eq!(watch.step(66_000, QUIET), Keep::Release(Release::NeverRan));
    }

    #[test]
    fn a_look_that_cannot_list_never_lets_go() {
        let mut watch = TurnWatch::new(0);
        assert_eq!(watch.step(500, RUNS), Keep::Hold);
        for at in (1_000..=60_000).step_by(500) {
            assert_eq!(watch.step(at, BLIND), Keep::Hold, "{at}");
        }
        assert_eq!(watch.step(60_500, QUIET), Keep::Hold);
        assert_eq!(watch.step(61_000, QUIET), Keep::Release(Release::RunEnded));

        let mut watch = TurnWatch::new(0);
        for at in (500..=60_000).step_by(500) {
            assert_eq!(watch.step(at, BLIND), Keep::Hold, "{at}");
        }
        assert_eq!(watch.step(60_500, RUNS), Keep::Hold);
    }
}
