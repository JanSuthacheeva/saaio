use std::thread;
use std::time::Duration;

use crate::fold;
use crate::store::Store;
use crate::view::FlowView;
use crate::wake;

/// How long the master's wait keeps collecting after the first change, so flows that stop
/// together wake it once.
const SETTLE: Duration = Duration::from_secs(2);

/// What a worker's waiter finds on one look.
#[derive(Debug, PartialEq, Eq)]
pub enum WorkerNews {
    /// The flow is finished or gone: stop waiting.
    Gone,
    Nothing,
    /// The master's messages it had not delivered yet, now marked delivered.
    News(Vec<String>),
}

/// The master's messages to flow `id` that its worker has not read yet.
pub fn worker_news(store: &Store, id: &str, now: u64) -> WorkerNews {
    let Ok(events) = store.events(id) else {
        return WorkerNews::Gone;
    };
    if fold::fold(id, &events, None, now).is_none_or(|flow| flow.is_finished()) {
        return WorkerNews::Gone;
    }
    let delivered = store.delivered(id);
    let messages = wake::undelivered(&events, delivered);
    if messages.is_empty() {
        return WorkerNews::Nothing;
    }
    if store.set_delivered(id, delivered + messages.len()).is_err() {
        return WorkerNews::Gone;
    }
    WorkerNews::News(messages.into_iter().map(str::to_string).collect())
}

/// The flows that stopped since the master last saw them, collected for SETTLE so several arrive
/// as one wake, and marked seen; none while `is_mine` no longer holds.
pub fn master_news(store: &Store, clock: fn() -> u64, is_mine: &dyn Fn() -> bool) -> Vec<FlowView> {
    let flows = fold::views(&store.logs().0, clock());
    let mut seen = store.seen().unwrap_or_else(|| wake::first_seen(&flows));
    if wake::changed_flows(&flows, &seen).is_empty() {
        return Vec::new();
    }
    thread::sleep(SETTLE);
    let flows = fold::views(&store.logs().0, clock());
    let changed = wake::changed_flows(&flows, &seen);
    if changed.is_empty() || !is_mine() {
        return Vec::new();
    }
    wake::mark_seen(&mut seen, &changed);
    if store.set_seen(&seen).is_err() {
        return Vec::new();
    }
    changed.into_iter().cloned().collect()
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;
    use crate::event::{Event, EventKind, Mode};

    fn started(store: &Store) -> String {
        let first = Event::new(
            1,
            EventKind::Started {
                name: "csv".to_string(),
                request: "SHOP-221 add CSV export".to_string(),
                mode: Mode::Auto,
            },
        );
        store.create(&first).unwrap()
    }

    fn send(store: &Store, id: &str, text: &str) {
        let message = EventKind::Message {
            text: text.to_string(),
        };
        store.append(id, &Event::new(2, message)).unwrap();
    }

    #[test]
    fn delivers_each_message_once() {
        let dir = TempDir::new().unwrap();
        let store = Store::at(dir.path().to_path_buf());
        let id = started(&store);

        assert_eq!(worker_news(&store, &id, 3), WorkerNews::Nothing);
        send(&store, &id, "use the old API");
        send(&store, &id, "and keep the tests");

        assert_eq!(
            worker_news(&store, &id, 3),
            WorkerNews::News(vec![
                "use the old API".to_string(),
                "and keep the tests".to_string()
            ])
        );
        assert_eq!(worker_news(&store, &id, 3), WorkerNews::Nothing);
        send(&store, &id, "one more");
        assert_eq!(
            worker_news(&store, &id, 3),
            WorkerNews::News(vec!["one more".to_string()])
        );
    }

    #[test]
    fn a_finished_or_missing_flow_is_gone() {
        let dir = TempDir::new().unwrap();
        let store = Store::at(dir.path().to_path_buf());
        let id = started(&store);
        send(&store, &id, "never read");
        store.append(&id, &Event::new(2, EventKind::Ended)).unwrap();

        assert_eq!(worker_news(&store, &id, 3), WorkerNews::Gone);
        assert_eq!(worker_news(&store, "nope", 3), WorkerNews::Gone);
    }

    #[test]
    fn the_master_has_no_news_before_a_flow_stops() {
        let dir = TempDir::new().unwrap();
        let store = Store::at(dir.path().to_path_buf());
        started(&store);

        assert!(master_news(&store, || 3, &|| true).is_empty());
    }
}
