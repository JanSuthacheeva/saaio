use std::time::{Duration, Instant};

use anyhow::Result;

use super::Server;
use super::news::{self, paste};
use crate::proto::{PaneKey, PaneStatus};
use crate::server_log;
use crate::store::{Busy, Waiter};
use crate::wake;

impl Server {
    /// Types each flow's next queued line into its pane when the pane is ready, and lets a line
    /// that began no turn within `timing.unstarted` go.
    pub(super) fn type_lines(&mut self) {
        let now = Instant::now();
        let running: Vec<PaneKey> = self
            .host
            .panes()
            .into_iter()
            .filter(|pane| pane.status == PaneStatus::Running)
            .map(|pane| pane.key)
            .collect();
        for key in running {
            let waiter = match &key {
                PaneKey::Master => Waiter::Master,
                PaneKey::Flow(id) => Waiter::Flow(id.clone()),
            };
            let mut busy = self.store.busy(&waiter);
            if unstarted(
                busy,
                self.line_at.get(&key).copied(),
                now,
                self.timing.unstarted,
            ) {
                self.let_go(&key, &waiter);
                busy = Busy::Idle;
            }
            let Some(id) = self.takes_lines(&key) else {
                continue;
            };
            let opened_at = self.opened_at.get(&key).copied();
            let typed_at = self.typed_at.get(&key).copied();
            if !ready(busy, opened_at, typed_at, now, self.timing.quiet) {
                continue;
            }
            let events = match self.store.events(&id) {
                Ok(events) => events,
                Err(error) => {
                    server_log::write((self.clock)(), &format!("{error:#}"));
                    continue;
                }
            };
            let typed = self.store.typed(&id);
            let Some(line) = wake::untyped(&events, typed)
                .first()
                .map(ToString::to_string)
            else {
                continue;
            };
            // Counted before the paste, as news is marked delivered before it is typed: a server
            // that stops between the two loses the line rather than typing it twice.
            if let Err(error) = self.store.set_typed(&id, typed + 1) {
                server_log::write((self.clock)(), &format!("{error:#}"));
                continue;
            }
            if let Err(error) = self.type_into(&key, &waiter, &line) {
                server_log::write(
                    (self.clock)(),
                    &format!("could not type into {key}: {error:#}"),
                );
                if let Err(error) = self.store.set_typed(&id, typed) {
                    server_log::write((self.clock)(), &format!("{error:#}"));
                }
            }
        }
    }

    /// Pastes `text` and Enter into the pane, marking the session Busy::Typed first, so the turn
    /// a hook begins right away is not overwritten; news and queued lines both type through it.
    pub(super) fn type_into(&mut self, key: &PaneKey, waiter: &Waiter, text: &str) -> Result<()> {
        self.store.set_busy(waiter, Busy::Typed)?;
        if let Err(error) = self.host.write(key, &paste(text)) {
            if let Err(error) = self.store.set_busy(waiter, Busy::Idle) {
                server_log::write((self.clock)(), &format!("{error:#}"));
            }
            return Err(error);
        }
        self.line_at.insert(key.clone(), Instant::now());
        Ok(())
    }

    /// The flow whose queued lines this pane takes: a running worker of a flow that is not plain
    /// or finished, whose agent has an adapter and whose session ended a turn (answer set).
    fn takes_lines(&self, key: &PaneKey) -> Option<String> {
        let PaneKey::Flow(id) = key else {
            return None;
        };
        let flow = self.board.flows.iter().find(|flow| &flow.id == id)?;
        self.adapters.get(key)?;
        let takes = !flow.is_plain() && !flow.is_finished() && flow.answer.is_some();
        takes.then(|| id.clone())
    }

    /// Marks a session the server typed into idle again: what it typed began no turn.
    fn let_go(&mut self, key: &PaneKey, waiter: &Waiter) {
        self.line_at.remove(key);
        if let Err(error) = self.store.set_busy(waiter, Busy::Idle) {
            server_log::write((self.clock)(), &format!("{error:#}"));
            return;
        }
        let name = match key {
            PaneKey::Master => "the master",
            PaneKey::Flow(id) => self
                .board
                .flows
                .iter()
                .find(|flow| &flow.id == id)
                .map_or(id.as_str(), |flow| flow.name.as_str()),
        };
        server_log::write(
            (self.clock)(),
            &format!("no turn began after typing into {name}"),
        );
    }
}

/// True when the pane takes a line now: idle, open and untouched by the person for `quiet`.
fn ready(
    busy: Busy,
    opened_at: Option<Instant>,
    typed_at: Option<Instant>,
    now: Instant,
    quiet: Duration,
) -> bool {
    let settled = opened_at.is_some_and(|at| now.duration_since(at) >= quiet);
    settled && news::quiet(busy, typed_at, now, quiet)
}

/// True when the server's own typed line began no turn within `wait`.
fn unstarted(busy: Busy, line_at: Option<Instant>, now: Instant, wait: Duration) -> bool {
    busy == Busy::Typed && line_at.is_some_and(|at| now.duration_since(at) >= wait)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::Timing;

    #[test]
    fn takes_a_line_once_idle_open_and_untouched_for_a_while() {
        let quiet = Timing::default().quiet;
        let opened = Instant::now();
        let later = opened + quiet;

        assert!(ready(Busy::Idle, Some(opened), None, later, quiet));
        assert!(!ready(Busy::Idle, None, None, later, quiet), "not open");
        assert!(
            !ready(Busy::Idle, Some(opened), None, opened + quiet / 2, quiet),
            "still starting up"
        );
        assert!(!ready(Busy::Turn, Some(opened), None, later, quiet));
        assert!(!ready(Busy::Typed, Some(opened), None, later, quiet));
        assert!(
            !ready(Busy::Idle, Some(opened), Some(later), later, quiet),
            "the person types there"
        );
    }

    #[test]
    fn lets_a_typed_line_go_once_no_turn_began_for_a_while() {
        let wait = Timing::default().unstarted;
        let typed = Instant::now();

        assert!(unstarted(Busy::Typed, Some(typed), typed + wait, wait));
        assert!(!unstarted(Busy::Typed, Some(typed), typed + wait / 2, wait));
        assert!(
            !unstarted(Busy::Turn, Some(typed), typed + wait, wait),
            "a turn began"
        );
        assert!(!unstarted(Busy::Typed, None, typed + wait, wait));
    }
}
