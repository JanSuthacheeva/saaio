use std::collections::HashMap;

use anyhow::Result;

use crate::store::{Stamp, Store};

/// Refold at least this often, so a flow turns not responding without any file changing.
const REFOLD_EVERY_MS: u64 = 5_000;

/// Tells the server when the board must be folded again, from file sizes and times alone.
#[derive(Debug, Default)]
pub struct StoreWatch {
    stamps: HashMap<String, Stamp>,
    folded_at: Option<u64>,
}

impl StoreWatch {
    /// True when a log or heartbeat changed, a flow came or went, or 5 s passed since the last fold.
    pub fn changed(&mut self, store: &Store, now: u64) -> Result<bool> {
        let stamps: HashMap<String, Stamp> = store.stamps()?.into_iter().collect();
        let due = self
            .folded_at
            .is_none_or(|at| now.saturating_sub(at) >= REFOLD_EVERY_MS);
        if !due && stamps == self.stamps {
            return Ok(false);
        }
        self.stamps = stamps;
        self.folded_at = Some(now);
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use std::thread;
    use std::time::Duration;

    use tempfile::TempDir;

    use super::*;
    use crate::event::{Event, EventKind, Mode};

    fn started() -> Event {
        Event::new(
            0,
            EventKind::Started {
                name: "csv".into(),
                request: "add csv".into(),
                mode: Mode::Auto,
            },
        )
    }

    #[test]
    fn sees_a_new_flow_an_append_a_beat_and_a_removal() {
        let dir = TempDir::new().unwrap();
        let store = Store::at(dir.path().to_path_buf());
        let mut watch = StoreWatch::default();

        assert!(watch.changed(&store, 0).unwrap(), "the first look folds");
        assert!(!watch.changed(&store, 100).unwrap());

        let id = store.create(&started()).unwrap();
        assert!(watch.changed(&store, 200).unwrap(), "a new flow");
        assert!(!watch.changed(&store, 300).unwrap());

        store
            .append(&id, &Event::new(1, EventKind::Working))
            .unwrap();
        assert!(watch.changed(&store, 400).unwrap(), "an append");

        thread::sleep(Duration::from_millis(10));
        store.beat(&id).unwrap();
        assert!(watch.changed(&store, 500).unwrap(), "a heartbeat");
        assert!(!watch.changed(&store, 600).unwrap());

        store.remove(&id).unwrap();
        assert!(watch.changed(&store, 700).unwrap(), "a removal");
    }

    #[test]
    fn folds_again_every_five_seconds_without_a_change() {
        let dir = TempDir::new().unwrap();
        let store = Store::at(dir.path().to_path_buf());
        let mut watch = StoreWatch::default();
        watch.changed(&store, 1_000).unwrap();

        assert!(!watch.changed(&store, 5_999).unwrap());
        assert!(watch.changed(&store, 6_000).unwrap());
        assert!(!watch.changed(&store, 6_500).unwrap());
    }
}
