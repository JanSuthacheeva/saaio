use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};

use super::{Event, Server, spawn_named};
use crate::event::Event as LogEvent;
use crate::fold;
use crate::forge::{self, Ask, Forge, Found};
use crate::server_log;
use crate::view::FlowView;
use crate::workspace;

/// How often a round of questions starts; the first starts with the server.
pub const ASK_EVERY: Duration = Duration::from_mins(2);

/// What the forge thread sends back.
#[derive(Debug)]
pub enum Answer {
    /// What the forge lists for a flow's branch; None when it lists nothing.
    Found { id: String, found: Option<Found> },
    /// A question that failed: gh or glab missing, not logged in, offline, unreadable JSON.
    Failed(String),
    /// The round is over.
    Done,
}

/// When the next round starts, whether one runs, the last error logged.
#[derive(Debug, Default)]
pub struct ForgeWatch {
    asked_at: Option<Instant>,
    asking: bool,
    failed: bool,
    logged: Option<String>,
}

impl ForgeWatch {
    /// The asks of a round when one is due, none runs and `flows` has any; marks it running.
    pub fn due(&mut self, flows: &[FlowView], now: Instant) -> Option<Vec<Ask>> {
        let due = !self.asking
            && self
                .asked_at
                .is_none_or(|at| now.saturating_duration_since(at) >= ASK_EVERY);
        if !due {
            return None;
        }
        let asks = forge::asks(flows);
        if asks.is_empty() {
            return None;
        }
        self.asking = true;
        self.failed = false;
        self.asked_at = Some(now);
        Some(asks)
    }

    /// The round ended; one without a failure forgets the error logged last.
    pub fn done(&mut self) {
        self.asking = false;
        if !self.failed {
            self.logged = None;
        }
    }

    /// `error` when it is not the one logged last.
    pub fn worth_logging(&mut self, error: String) -> Option<String> {
        self.failed = true;
        if self.logged.as_ref() == Some(&error) {
            return None;
        }
        self.logged = Some(error.clone());
        Some(error)
    }
}

/// Asks about each flow in turn on the "forge" thread, sending an Answer for each and Done last.
pub fn ask_in_background(asks: Vec<Ask>, events: Sender<Event>) {
    spawn_named("forge".to_string(), move || {
        for one in asks {
            let answer = match ask(&one) {
                Ok(found) => Answer::Found { id: one.id, found },
                Err(error) => Answer::Failed(format!("{error:#}")),
            };
            if events.send(Event::Forge(answer)).is_err() {
                return;
            }
        }
        let _ = events.send(Event::Forge(Answer::Done));
    });
}

/// One flow's question: its worktree's remotes -> Forge::of -> gh or glab -> parse -> pick.
/// Ok(None) for a worktree that is gone, a remote of no known forge, or no merge request.
fn ask(ask: &Ask) -> Result<Option<Found>> {
    if !ask.dir.is_dir() {
        return Ok(None);
    }
    let remotes = workspace::remote_urls(&ask.dir);
    let Some(forge) = Forge::of(&remotes, workspace::ssh_hostname) else {
        return Ok(None);
    };
    let json = workspace::output_in(&ask.dir, &forge.argv(&ask.branch))
        .with_context(|| format!("{} failed", forge.program()))?;
    Ok(forge::pick(forge.parse(&json)?))
}

impl Server {
    /// Starts a round when ForgeWatch says one is due; from refresh.
    pub(super) fn ask_forges(&mut self) {
        if let Some(asks) = self.forges.due(&self.board.flows, Instant::now()) {
            ask_in_background(asks, self.events.clone());
        }
    }

    /// Folds the flow again from its log and appends forge::news, if any; logs a failure once.
    pub(super) fn forge_answer(&mut self, answer: Answer) {
        let now = (self.clock)();
        match answer {
            Answer::Found { id, found } => {
                let Ok(events) = self.store.events(&id) else {
                    return;
                };
                let news = fold::fold(&id, &events, None, now)
                    .and_then(|flow| forge::news(&flow, found.as_ref()));
                if let Some(kind) = news
                    && let Err(error) = self.store.append(&id, &LogEvent::new(now, kind))
                {
                    server_log::write(now, &format!("{error:#}"));
                }
            }
            Answer::Failed(error) => {
                if let Some(error) = self.forges.worth_logging(error) {
                    server_log::write(now, &format!("could not ask about merge requests: {error}"));
                }
            }
            Answer::Done => self.forges.done(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{EventKind, Mode};

    fn flows(merged: bool) -> Vec<FlowView> {
        let mut kinds = vec![
            EventKind::Started {
                name: "csv".into(),
                request: "add csv".into(),
                mode: Mode::Auto,
                phases: None,
            },
            EventKind::Workspace {
                path: "/work/csv".into(),
                branch: Some("flow/csv".into()),
                slot: None,
                window: None,
                agent: "claude".into(),
            },
        ];
        if merged {
            kinds.push(EventKind::Merged { url: "u".into() });
        }
        let events: Vec<LogEvent> = kinds
            .into_iter()
            .map(|kind| LogEvent::new(1, kind))
            .collect();
        vec![fold::fold("csv-1", &events, None, 2).unwrap()]
    }

    #[test]
    fn asks_at_once_then_every_two_minutes_one_round_at_a_time() {
        let mut watch = ForgeWatch::default();
        let start = Instant::now();

        assert_eq!(
            watch.due(&flows(false), start).map(|asks| asks.len()),
            Some(1)
        );
        assert!(
            watch.due(&flows(false), start + ASK_EVERY).is_none(),
            "a round runs"
        );
        watch.done();
        assert!(
            watch.due(&flows(false), start + ASK_EVERY / 2).is_none(),
            "too soon"
        );
        assert!(watch.due(&flows(false), start + ASK_EVERY).is_some());
    }

    #[test]
    fn starts_no_round_without_a_flow_to_ask_about() {
        let mut watch = ForgeWatch::default();
        let start = Instant::now();

        assert!(watch.due(&flows(true), start).is_none());
        assert!(watch.due(&[], start).is_none());
        assert!(
            watch.due(&flows(false), start).is_some(),
            "not counted as a round"
        );
    }

    #[test]
    fn logs_an_error_once_until_a_round_goes_without_one() {
        let mut watch = ForgeWatch::default();
        let offline = || "gh failed: could not resolve host".to_string();

        assert_eq!(watch.worth_logging(offline()), Some(offline()));
        assert_eq!(watch.worth_logging(offline()), None);
        watch.done();
        assert_eq!(watch.worth_logging(offline()), None, "the round had it too");
        assert_eq!(
            watch.worth_logging("glab failed".into()),
            Some("glab failed".into())
        );
        watch.done();
        watch.due(&flows(false), Instant::now());
        watch.done();
        assert_eq!(
            watch.worth_logging(offline()),
            Some(offline()),
            "after a clean round"
        );
    }
}
