//! `Backlog`: pane output read and not yet fed to its terminal, which a pane's reader waits on, so
//! a pane that prints without pause slows itself and not the server

use std::sync::{Condvar, Mutex, PoisonError};

/// Past this many bytes waiting for the server, every pane's reader waits for it.
const MAX_BACKLOG: usize = 256 << 10;

#[derive(Debug, Default)]
pub(super) struct Backlog {
    state: Mutex<State>,
    changed: Condvar,
}

#[derive(Debug, Default)]
struct State {
    bytes: usize,
    /// The server no longer feeds anything: nobody waits again.
    closed: bool,
}

impl Backlog {
    /// Counts `bytes` read from a pane, first waiting while the backlog is past MAX_BACKLOG.
    pub(super) fn add(&self, bytes: usize) {
        let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let mut state = self
            .changed
            .wait_while(state, |state| state.bytes > MAX_BACKLOG && !state.closed)
            .unwrap_or_else(PoisonError::into_inner);
        state.bytes += bytes;
    }

    /// Counts `bytes` fed to a terminal off the backlog.
    pub(super) fn fed(&self, bytes: usize) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.bytes = state.bytes.saturating_sub(bytes);
        self.changed.notify_all();
    }

    /// Lets every reader go on without waiting, for good.
    pub(super) fn close(&self) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.closed = true;
        self.changed.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    use super::*;

    /// Adds `bytes` on a thread of its own; the receiver hears once the add returned.
    fn add_later(backlog: &Arc<Backlog>, bytes: usize) -> mpsc::Receiver<()> {
        let (done, added) = mpsc::channel();
        let backlog = Arc::clone(backlog);
        thread::spawn(move || {
            backlog.add(bytes);
            let _ = done.send(());
        });
        added
    }

    #[test]
    fn a_reader_waits_past_the_limit_until_the_server_feeds() {
        let backlog = Arc::new(Backlog::default());
        backlog.add(MAX_BACKLOG + 1);

        let added = add_later(&backlog, 10);
        assert!(added.recv_timeout(Duration::from_millis(100)).is_err());

        backlog.fed(MAX_BACKLOG);
        added.recv_timeout(Duration::from_secs(30)).unwrap();
    }

    #[test]
    fn a_closed_backlog_keeps_nobody_waiting() {
        let backlog = Arc::new(Backlog::default());
        backlog.add(MAX_BACKLOG + 1);
        let added = add_later(&backlog, 10);

        backlog.close();

        added.recv_timeout(Duration::from_secs(30)).unwrap();
        backlog.add(MAX_BACKLOG);
    }
}
