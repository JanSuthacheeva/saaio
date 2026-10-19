use std::time::Instant;

use super::Server;
use super::news::QUIET;
use crate::proto::{PaneKey, PaneStatus};
use crate::server_log;
use crate::view::FlowView;
use crate::wake;

const ESC: &[u8] = b"\x1b";

impl Server {
    /// Presses Esc once into each running pane whose flow waits on a question dialog the master
    /// answered, once nobody typed there for QUIET, so Claude Code closes it and hands the held
    /// message over. Never twice into one dialog: a second Esc could interrupt the turn the first
    /// one began.
    pub(super) fn dismiss_dialogs(&mut self) {
        let now = Instant::now();
        let answered: Vec<(PaneKey, String, u64)> = self
            .host
            .panes()
            .into_iter()
            .filter(|pane| pane.status == PaneStatus::Running)
            .filter_map(|pane| {
                let flow = self.answered_dialog(&pane.key)?;
                Some((pane.key, flow.name.clone(), flow.changed_at))
            })
            .collect();
        self.dismissed
            .retain(|key, _| answered.iter().any(|(answered, ..)| answered == key));
        for (key, name, dialog) in answered {
            let typed_at = self.typed_at.get(&key).copied();
            let opened_at = self.opened_at.get(&key).copied();
            if self.dismissed.get(&key) == Some(&dialog) || !due(typed_at, opened_at, now) {
                continue;
            }
            match self.host.write(&key, ESC) {
                Ok(()) => {
                    self.dismissed.insert(key, dialog);
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

/// True when Esc may go into a pane: open and untouched by the person for QUIET.
fn due(typed_at: Option<Instant>, opened_at: Option<Instant>, now: Instant) -> bool {
    let settled = opened_at.is_some_and(|at| now.duration_since(at) >= QUIET);
    settled && typed_at.is_none_or(|at| now.duration_since(at) >= QUIET)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presses_esc_once_the_pane_is_open_and_quiet() {
        let opened = Instant::now();
        let later = opened + QUIET;

        assert!(due(None, Some(opened), later));
        assert!(!due(None, None, later), "not open");
        assert!(
            !due(None, Some(opened), opened + QUIET / 2),
            "still starting up"
        );
        assert!(
            !due(Some(later), Some(opened), later + QUIET / 2),
            "the person types there"
        );
        assert!(due(Some(later), Some(opened), later + QUIET));
    }
}
