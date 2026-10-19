use std::time::{Duration, Instant};

use super::Server;
use super::news::QUIET;
use crate::proto::{PaneKey, PaneStatus};
use crate::server_log;
use crate::view::FlowView;
use crate::wake;

/// How long after an Esc the server may press it again into a dialog still open.
#[cfg(not(test))]
pub(super) const AGAIN: Duration = Duration::from_secs(30);
#[cfg(test)]
pub(super) const AGAIN: Duration = Duration::from_secs(3);

const ESC: &[u8] = b"\x1b";

impl Server {
    /// Presses Esc into each running pane whose flow waits on a question dialog the master
    /// answered, once nobody typed there for QUIET, so Claude Code closes it and hands the held
    /// message over; again after AGAIN while it stays open.
    pub(super) fn dismiss_dialogs(&mut self) {
        let now = Instant::now();
        let answered: Vec<(PaneKey, String)> = self
            .host
            .panes()
            .into_iter()
            .filter(|pane| pane.status == PaneStatus::Running)
            .filter_map(|pane| {
                let name = self.answered_dialog(&pane.key)?.name.clone();
                Some((pane.key, name))
            })
            .collect();
        self.dismissed_at
            .retain(|key, _| answered.iter().any(|(answered, _)| answered == key));
        for (key, name) in answered {
            let typed_at = self.typed_at.get(&key).copied();
            let opened_at = self.opened_at.get(&key).copied();
            let dismissed_at = self.dismissed_at.get(&key).copied();
            if !due(typed_at, opened_at, dismissed_at, now) {
                continue;
            }
            self.dismissed_at.insert(key.clone(), now);
            let line = match self.host.write(&key, ESC) {
                Ok(()) => format!("closed {name}'s question dialog for the master's message"),
                Err(error) => format!("could not close {name}'s question dialog: {error:#}"),
            };
            server_log::write((self.clock)(), &line);
        }
    }

    /// The flow behind a running worker pane whose dialog the master answered.
    fn answered_dialog(&self, key: &PaneKey) -> Option<&FlowView> {
        let PaneKey::Flow(id) = key else {
            return None;
        };
        let flow = self.board.flows.iter().find(|flow| &flow.id == id)?;
        if flow.questions.is_empty() {
            return None;
        }
        let events = match self.store.events(id) {
            Ok(events) => events,
            Err(error) => {
                server_log::write((self.clock)(), &format!("{error:#}"));
                return None;
            }
        };
        wake::dialog_answered(flow, &events).then_some(flow)
    }
}

/// True when Esc may go into a pane: open and untouched by the person for QUIET, no Esc within
/// AGAIN.
fn due(
    typed_at: Option<Instant>,
    opened_at: Option<Instant>,
    dismissed_at: Option<Instant>,
    now: Instant,
) -> bool {
    let past =
        |at: Option<Instant>, wait: Duration| at.is_none_or(|at| now.duration_since(at) >= wait);
    let settled = opened_at.is_some_and(|at| now.duration_since(at) >= QUIET);
    settled && past(typed_at, QUIET) && past(dismissed_at, AGAIN)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presses_esc_once_the_pane_is_quiet_and_again_only_after_a_while() {
        let opened = Instant::now();
        let later = opened + QUIET;

        assert!(due(None, Some(opened), None, later));
        assert!(!due(None, None, None, later), "not open");
        assert!(
            !due(None, Some(opened), None, opened + QUIET / 2),
            "still starting up"
        );
        assert!(
            !due(Some(later), Some(opened), None, later + QUIET / 2),
            "the person types there"
        );
        assert!(
            !due(None, Some(opened), Some(later), later + AGAIN / 2),
            "pressed a moment ago"
        );
        assert!(due(None, Some(opened), Some(later), later + AGAIN));
    }
}
