//! `PaneState`: what the server keeps about each pane, kept past its close

use std::time::Instant;

use super::{ConnId, Server};
use crate::adapter::Adapter;
use crate::proto::PaneKey;

/// What the server keeps about one pane; kept after it closes, so a reopened pane starts from it.
#[derive(Debug, Default)]
pub(super) struct PaneState {
    /// The adapter it started with; None for a plain session.
    pub(super) adapter: Option<Adapter>,
    /// When it was resumed, so a resume that fails at once starts fresh instead.
    pub(super) resumed: Option<Instant>,
    /// The connections waiting for it to close.
    pub(super) closing: Vec<ConnId>,
    /// When it last got a key from the person, so typed news waits until they pause.
    pub(super) typed_at: Option<Instant>,
    /// When it opened, so nothing is typed into a harness still starting up.
    pub(super) opened_at: Option<Instant>,
    /// When the server last typed into it, for a line that begins no turn.
    pub(super) line_at: Option<Instant>,
    /// The last question dialog (when its Waiting event was logged) the server pressed Esc into;
    /// kept, so no failed read of a log forgets it.
    pub(super) dismissed: Option<u64>,
}

static UNTOUCHED: PaneState = PaneState {
    adapter: None,
    resumed: None,
    closing: Vec::new(),
    typed_at: None,
    opened_at: None,
    line_at: None,
    dismissed: None,
};

impl Server {
    /// The pane's state; an empty one for a pane the server never touched.
    pub(super) fn pane(&self, key: &PaneKey) -> &PaneState {
        self.panes.get(key).unwrap_or(&UNTOUCHED)
    }

    /// The pane's state to change, made empty the first time.
    pub(super) fn pane_mut(&mut self, key: &PaneKey) -> &mut PaneState {
        self.panes.entry(key.clone()).or_default()
    }
}
