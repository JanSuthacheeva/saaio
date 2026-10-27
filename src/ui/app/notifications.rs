//! Which state changes raise a desktop notification, and what it says

use super::{App, Effect};
use crate::proto::PaneKey;
use crate::view::{Board, FlowView, State};

impl App {
    /// Raises notifications from now on, or none.
    pub fn set_notify(&mut self, on: bool) {
        self.notify = on;
    }

    /// One notification for each flow of both boards that came to Waiting or Blocked out of sight.
    pub(super) fn worker_notifications(&self, old: &Board, new: &Board) -> Vec<Effect> {
        if !self.notify {
            return vec![];
        }
        new.flows
            .iter()
            .filter(|flow| {
                old.flows
                    .iter()
                    .find(|was| was.id == flow.id)
                    .is_some_and(|was| calls_for_you(Some(was.state), flow.state))
            })
            .filter(|flow| self.out_of_sight(&PaneKey::Flow(flow.id.clone())))
            .map(|flow| Effect::Notify(worker_text(flow)))
            .collect()
    }

    /// A notification when the master came to Waiting or Blocked out of sight; none for the turn
    /// the first master message names, which only says where it stood at attach.
    pub(super) fn master_notification(
        &self,
        was: Option<State>,
        state: Option<State>,
    ) -> Vec<Effect> {
        let attached = self.master.cwd.is_some();
        match state {
            Some(state)
                if self.notify
                    && attached
                    && calls_for_you(was, state)
                    && self.out_of_sight(&PaneKey::Master) =>
            {
                vec![Effect::Notify(master_text(state))]
            }
            _ => vec![],
        }
    }

    /// The terminal has lost focus, or `key` is not the pane in view.
    fn out_of_sight(&self, key: &PaneKey) -> bool {
        !self.terminal_focused || &self.focus != key
    }
}

/// Waiting or Blocked now, and not the same a moment ago.
fn calls_for_you(was: Option<State>, now: State) -> bool {
    matches!(now, State::Waiting | State::Blocked) && was != Some(now)
}

/// "shop-230 asks: <first question>", "shop-230 waits for you",
/// "shop-230 is blocked: <blocker>", "shop-230 is blocked".
fn worker_text(flow: &FlowView) -> String {
    let name = &flow.name;
    match (flow.state, flow.questions.first(), &flow.blocker) {
        (State::Waiting, Some(first), _) => format!("{name} asks: {}", first.question),
        (State::Blocked, _, Some(blocker)) => format!("{name} is blocked: {blocker}"),
        (State::Blocked, _, None) => format!("{name} is blocked"),
        _ => format!("{name} waits for you"),
    }
}

/// "master waits for you", "master is blocked".
fn master_text(state: State) -> String {
    match state {
        State::Blocked => "master is blocked".to_string(),
        _ => "master waits for you".to_string(),
    }
}
