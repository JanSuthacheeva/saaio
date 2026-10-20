use std::time::{Duration, Instant};

use super::Server;
use crate::event::EventKind;
use crate::proto::{PaneKey, PaneStatus};
use crate::server_log;
use crate::view::FlowView;
use crate::wake;

const ESC: &[u8] = b"\x1b";

impl Server {
    /// Presses Esc once into each running pane whose flow waits on a question dialog the master
    /// answered, once nobody typed there for `timing.quiet`, so Claude Code closes it and hands the
    /// held message over. Never twice into one dialog: a second Esc could interrupt the turn the
    /// first one began.
    pub(super) fn dismiss_dialogs(&mut self) {
        let now = Instant::now();
        let answered: Vec<(PaneKey, String, u64)> = self
            .host
            .panes()
            .into_iter()
            .filter(|pane| pane.status == PaneStatus::Running)
            .filter_map(|pane| {
                let (flow, asked_at) = self.answered_dialog(&pane.key)?;
                Some((pane.key, flow.name.clone(), asked_at))
            })
            .collect();
        for (key, name, dialog) in answered {
            let pane = self.pane(&key);
            if pane.dismissed == Some(dialog)
                || !due(pane.typed_at, pane.opened_at, now, self.timing.quiet)
            {
                continue;
            }
            match self.host.write(&key, ESC) {
                Ok(()) => {
                    self.pane_mut(&key).dismissed = Some(dialog);
                    server_log::write(
                        (self.clock)(),
                        &format!("closed {name}'s question dialog for the master's message"),
                    );
                }
                Err(error) => server_log::write(
                    (self.clock)(),
                    &format!("could not close {name}'s question dialog: {error:#}"),
                ),
            }
        }
    }

    /// The flow behind a running worker pane whose dialog the master answered, and when it asked.
    fn answered_dialog(&self, key: &PaneKey) -> Option<(&FlowView, u64)> {
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
        if !wake::dialog_answered(flow, &events) {
            return None;
        }
        let asked = events
            .iter()
            .rev()
            .find(|event| matches!(event.kind, EventKind::Waiting { .. }))?;
        Some((flow, asked.at))
    }
}

/// True when Esc may go into a pane: open and untouched by the person for `quiet`.
fn due(
    typed_at: Option<Instant>,
    opened_at: Option<Instant>,
    now: Instant,
    quiet: Duration,
) -> bool {
    let settled = opened_at.is_some_and(|at| now.duration_since(at) >= quiet);
    settled && typed_at.is_none_or(|at| now.duration_since(at) >= quiet)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::Timing;

    #[test]
    fn presses_esc_once_the_pane_is_open_and_quiet() {
        let quiet = Timing::default().quiet;
        let opened = Instant::now();
        let later = opened + quiet;

        assert!(due(None, Some(opened), later, quiet));
        assert!(!due(None, None, later, quiet), "not open");
        assert!(
            !due(None, Some(opened), opened + quiet / 2, quiet),
            "still starting up"
        );
        assert!(
            !due(Some(later), Some(opened), later + quiet / 2, quiet),
            "the person types there"
        );
        assert!(due(Some(later), Some(opened), later + quiet, quiet));
    }
}
