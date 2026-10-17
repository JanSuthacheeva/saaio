use std::time::{Duration, Instant};

use super::Server;
use crate::cli::output::news;
use crate::deliver::{self, WorkerNews};
use crate::proto::{PaneKey, PaneStatus};
use crate::server_log;
use crate::store::{Busy, Waiter};
use crate::view::FlowView;

/// No key into a pane for this long before the server types news into it, so it does not land in
/// the middle of what the person types there.
#[cfg(not(test))]
pub(super) const QUIET: Duration = Duration::from_secs(10);
#[cfg(test)]
pub(super) const QUIET: Duration = Duration::from_millis(200);

impl Server {
    /// Types the news of each session that cannot wake itself into its pane, when it sits idle at
    /// its prompt and nobody typed there for QUIET: a Codex worker's messages from the master, a
    /// Codex master's stopped flows. A busy session gets them from its Stop hook instead.
    pub(super) fn deliver_news(&mut self) {
        let now = Instant::now();
        let running = self
            .host
            .panes()
            .into_iter()
            .filter(|pane| pane.status == PaneStatus::Running)
            .map(|pane| pane.key);
        let due: Vec<(PaneKey, Waiter)> = running
            .filter_map(|key| self.server_delivered(&key).map(|waiter| (key, waiter)))
            .filter(|(key, waiter)| {
                let busy = self.store.busy(waiter);
                quiet(busy, self.typed_at.get(key).copied(), now)
            })
            .collect();
        for (key, waiter) in due {
            let Some(text) = self.take_news(&waiter) else {
                continue;
            };
            if let Err(error) = self.type_into(&key, &waiter, &text) {
                server_log::write(
                    (self.clock)(),
                    &format!("could not hand over news: {error:#}"),
                );
            }
        }
    }

    /// The waiter whose news the server types into this pane: a session whose adapter does not
    /// wake itself and that had its first turn (so Codex is past its startup and hooks review); no
    /// plain session, and the master only when it wakes.
    fn server_delivered(&self, key: &PaneKey) -> Option<Waiter> {
        match key {
            PaneKey::Master => {
                let adapter = self.config.master_agent()?.1.adapter?;
                let delivers = !adapter.wakes_itself()
                    && self.config.master.wake
                    && self.store.master_session().is_some();
                delivers.then_some(Waiter::Master)
            }
            PaneKey::Flow(id) => {
                let flow = self.board.flows.iter().find(|flow| &flow.id == id)?;
                let agent = &flow.workspace.as_ref()?.agent;
                let adapter = self.config.agent(Some(agent))?.1.adapter?;
                let delivers = !adapter.wakes_itself()
                    && !flow.is_plain()
                    && !flow.is_finished()
                    && flow.session.is_some();
                delivers.then(|| Waiter::Flow(id.clone()))
            }
        }
    }

    /// The waiter's news, marked delivered or seen; None when there is none.
    fn take_news(&self, waiter: &Waiter) -> Option<String> {
        match waiter {
            Waiter::Flow(id) => match deliver::worker_news(&self.store, id, (self.clock)()) {
                WorkerNews::News(messages) => {
                    let messages: Vec<&str> = messages.iter().map(String::as_str).collect();
                    Some(news::for_worker(&messages))
                }
                WorkerNews::Nothing | WorkerNews::Gone => None,
            },
            Waiter::Master => {
                let changed = deliver::master_news_now(&self.store, &self.board.flows);
                let changed: Vec<&FlowView> = changed.iter().collect();
                (!changed.is_empty()).then(|| news::for_master(&changed))
            }
        }
    }
}

/// True when the server may type into a session: not running a turn, and no key typed into its
/// pane within QUIET.
pub(super) fn quiet(busy: Busy, typed_at: Option<Instant>, now: Instant) -> bool {
    busy == Busy::Idle && typed_at.is_none_or(|at| now.duration_since(at) >= QUIET)
}

/// `text` as one bracketed paste, then Enter: a single prompt however many lines it has, which
/// Codex's composer takes whole instead of as typed keys.
pub(super) fn paste(text: &str) -> Vec<u8> {
    let mut bytes = b"\x1b[200~".to_vec();
    bytes.extend_from_slice(text.as_bytes());
    bytes.extend_from_slice(b"\x1b[201~\r");
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn types_only_into_an_idle_session_nobody_typed_into_lately() {
        let typed = Instant::now();

        assert!(quiet(Busy::Idle, None, typed));
        assert!(!quiet(Busy::Turn, None, typed), "running a turn");
        assert!(!quiet(Busy::Typed, None, typed), "typed into, no turn yet");
        assert!(
            !quiet(Busy::Idle, Some(typed), typed + QUIET / 2),
            "typed into lately"
        );
        assert!(quiet(Busy::Idle, Some(typed), typed + QUIET));
    }

    #[test]
    fn pastes_the_news_as_one_prompt() {
        assert_eq!(paste("a\nb"), b"\x1b[200~a\nb\x1b[201~\r");
    }
}
