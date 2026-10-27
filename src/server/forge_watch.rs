//! `ForgeWatch`: rounds asking gh and glab about the flows' merge requests, off the server thread

use std::io;
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};

use super::{Event, Server, spawn_named};
use crate::event::{Event as LogEvent, EventKind};
use crate::fold;
use crate::forge::{self, Ask, Forge, Found};
use crate::server_log;
use crate::stack;
use crate::view::FlowView;
use crate::workspace;

/// How often a round of questions starts; the first starts with the server.
pub const ASK_EVERY: Duration = Duration::from_mins(2);

/// How long gh or glab may take to answer before it is killed.
pub const ASK_LIMIT: Duration = Duration::from_mins(1);

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

/// A query succeeded (and may have listed no current PR), or no forge could be queried.
#[derive(Debug)]
enum Lookup {
    Listed(Option<Found>),
    Unavailable,
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
pub fn ask_in_background(asks: Vec<Ask>, events: Sender<Event>) -> io::Result<()> {
    spawn_named("forge".to_string(), move || {
        for one in asks {
            let answer = match ask(&one) {
                Ok(Lookup::Listed(found)) => Answer::Found { id: one.id, found },
                Ok(Lookup::Unavailable) => continue,
                Err(error) => Answer::Failed(format!("{error:#}")),
            };
            if events.send(Event::Forge(answer)).is_err() {
                return;
            }
        }
        let _ = events.send(Event::Forge(Answer::Done));
    })
}

/// One flow's question, asked in the main checkout: its remotes -> Forge::of -> gh or glab ->
/// parse -> pick. Unavailable for a gone worktree or no known forge; Listed(None) for a successful
/// query with no merge request opened since the flow started.
fn ask(ask: &Ask) -> Result<Lookup> {
    if !ask.dir.is_dir() {
        return Ok(Lookup::Unavailable);
    }
    let remotes = workspace::git::remote_urls(&ask.repo);
    let Some(forge) = Forge::of(&remotes, workspace::git::ssh_hostname) else {
        return Ok(Lookup::Unavailable);
    };
    let json = workspace::process::output_in(&ask.repo, &forge.argv(&ask.branch), ASK_LIMIT)
        .with_context(|| format!("{} failed", forge.program()))?;
    let found = with_shown(forge, forge.parse(&json)?, ask.shown.as_deref(), |argv| {
        workspace::process::output_in(&ask.repo, argv, ASK_LIMIT)
    })?;
    Ok(Lookup::Listed(forge::pick(found, ask.since)))
}

/// Complete a limited list with the request already shown for this flow.
fn with_shown(
    forge: Forge,
    mut found: Vec<Found>,
    shown: Option<&str>,
    query: impl FnOnce(&[String]) -> Result<String>,
) -> Result<Vec<Found>> {
    if let Some(shown) = shown
        && !found.iter().any(|one| one.url == shown)
    {
        let json = query(&forge.view_argv(shown)?)
            .with_context(|| format!("{} could not look up {shown}", forge.program()))?;
        let one = forge.parse_view(&json)?;
        anyhow::ensure!(
            one.url == shown,
            "{} returned a different merge request for {shown}",
            forge.program()
        );
        found.push(one);
    }
    Ok(found)
}

impl Server {
    /// Starts a round when ForgeWatch says one is due; from refresh.
    pub(super) fn ask_forges(&mut self) {
        if let Some(asks) = self.forges.due(&self.board.flows, Instant::now())
            && let Err(error) = ask_in_background(asks, self.events.clone())
        {
            let line = format!("could not ask about merge requests: {error}");
            server_log::write((self.clock)(), &line);
            self.forge_answer(Answer::Done);
        }
    }

    /// Folds the flow again from its log and appends forge::news, if any, and once it is Merged
    /// stack::on_merge's events to the flows on its branch; at the end of a round, the moves still
    /// pending; logs a failure once.
    pub(super) fn forge_answer(&mut self, answer: Answer) {
        let now = (self.clock)();
        match answer {
            Answer::Found { id, found } => {
                let Ok(events) = self.store.events(&id) else {
                    return;
                };
                let Some(flow) = fold::fold(&id, &events, None, now) else {
                    return;
                };
                let Some(kind) = forge::news(&flow, found.as_ref()) else {
                    return;
                };
                let merged = matches!(kind, EventKind::Merged { .. });
                if let Err(error) = self.store.append(&id, &LogEvent::new(now, kind)) {
                    server_log::write(now, &format!("{error:#}"));
                    return;
                }
                if merged && let Some(found) = found {
                    let events = stack::on_merge(&self.board.flows, &flow, &found.target);
                    self.restack(events, now);
                }
            }
            Answer::Failed(error) => {
                if let Some(error) = self.forges.worth_logging(error) {
                    server_log::write(now, &format!("could not ask about merge requests: {error}"));
                }
            }
            // Once a round, from the board refolded since the last: the moves a failed append
            // or an earlier server missed.
            Answer::Done => {
                self.forges.done();
                self.restack(stack::pending(&self.board.flows), now);
            }
        }
    }

    /// Appends each (id, kind); logs a failure to server.log and goes on.
    fn restack(&self, events: Vec<(String, EventKind)>, now: u64) {
        for (id, kind) in events {
            if let Err(error) = self.store.append(&id, &LogEvent::new(now, kind)) {
                server_log::write(now, &format!("{error:#}"));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::Mode;

    #[test]
    fn looks_up_a_shown_open_request_beyond_ten_closed_results() {
        for (forge, shown, listed, viewed) in [
            (
                Forge::GitHub,
                "https://github.com/acme/shop/pull/7",
                (8..=17)
                    .map(|i| format!(r#"{{"url":"https://github.com/acme/shop/pull/{i}","state":"CLOSED","createdAt":"2099-01-01T00:00:00Z"}}"#))
                    .collect::<Vec<_>>()
                    .join(","),
                r#"{"url":"https://github.com/acme/shop/pull/7","state":"OPEN","createdAt":"2099-01-01T00:00:00Z"}"#,
            ),
            (
                Forge::GitLab,
                "https://gitlab.com/acme/shop/-/merge_requests/7",
                (8..=17)
                    .map(|i| format!(r#"{{"web_url":"https://gitlab.com/acme/shop/-/merge_requests/{i}","state":"closed","created_at":"2099-01-01T00:00:00Z"}}"#))
                    .collect::<Vec<_>>()
                    .join(","),
                r#"{"web_url":"https://gitlab.com/acme/shop/-/merge_requests/7","state":"opened","created_at":"2099-01-01T00:00:00Z"}"#,
            ),
        ] {
            let listed = forge.parse(&format!("[{listed}]")).unwrap();
            assert_eq!(listed.len(), 10);
            let found = with_shown(forge, listed, Some(shown), |argv| {
                assert_eq!(argv, &forge.view_argv(shown).unwrap());
                Ok(viewed.to_string())
            })
            .unwrap();
            let picked = forge::pick(found, 0).unwrap();
            assert_eq!(picked.url, shown);
            assert_eq!(picked.status, forge::Status::Open);
        }
    }

    #[test]
    fn a_failed_direct_lookup_preserves_the_shown_request() {
        let found = Forge::GitHub
            .parse(r#"[{"url":"https://github.com/acme/shop/pull/8","state":"CLOSED","createdAt":"2099-01-01T00:00:00Z"}]"#)
            .unwrap();
        let error = with_shown(
            Forge::GitHub,
            found,
            Some("https://github.com/acme/shop/pull/7"),
            |_| anyhow::bail!("offline"),
        )
        .unwrap_err();
        assert!(error.to_string().contains("could not look up"));
    }

    fn flows(merged: bool) -> Vec<FlowView> {
        let mut kinds = vec![
            EventKind::Started {
                name: "csv".into(),
                request: "add csv".into(),
                mode: Mode::Auto,
                phases: None,
            },
            EventKind::Workspace {
                path: "/work/shop/.saaio/worktrees/csv".into(),
                branch: Some("flow/csv".into()),
                slot: None,
                window: None,
                agent: "claude".into(),
                base: None,
            },
        ];
        if merged {
            kinds.push(EventKind::Merged {
                url: "u".into(),
                into: None,
            });
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
