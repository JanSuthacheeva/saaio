//! One look for news on the store: a worker's undelivered messages, the master's stopped flows

use std::thread;
use std::time::Duration;

use crate::fold;
use crate::store::Store;
use crate::view::FlowView;
use crate::wake::{self, Note};

/// How long the master's wait keeps collecting after the first change, so flows that stop
/// together wake it once.
const SETTLE: Duration = Duration::from_secs(2);

/// What a worker's waiter finds on one look.
#[derive(Debug, PartialEq, Eq)]
pub enum WorkerNews {
    /// The flow is finished or gone: stop waiting.
    Gone,
    Nothing,
    /// The notes it had not delivered yet, now marked delivered.
    News(Vec<Note>),
}

/// The master's messages and saaio's notes to flow `id` that its worker has not read yet.
pub fn worker_news(store: &Store, id: &str, now: u64) -> WorkerNews {
    let Ok(events) = store.events(id) else {
        return WorkerNews::Gone;
    };
    if fold::fold(id, &events, None, now).is_none_or(|flow| flow.is_finished()) {
        return WorkerNews::Gone;
    }
    let delivered = store.delivered(id);
    let notes = wake::undelivered(&events, delivered);
    if notes.is_empty() {
        return WorkerNews::Nothing;
    }
    if store.set_delivered(id, delivered + notes.len()).is_err() {
        return WorkerNews::Gone;
    }
    WorkerNews::News(notes)
}

/// The flows that stopped since the master last saw them, collected for SETTLE so several arrive
/// as one wake, and marked seen; none while `is_mine` no longer holds.
pub fn master_news(store: &Store, clock: fn() -> u64, is_mine: &dyn Fn() -> bool) -> Vec<FlowView> {
    let flows = fold::views(&store.logs().0, clock());
    let seen = store.seen().unwrap_or_else(|| wake::first_seen(&flows));
    if wake::changed_flows(&flows, &seen).is_empty() {
        return Vec::new();
    }
    thread::sleep(SETTLE);
    if !is_mine() {
        return Vec::new();
    }
    master_news_now(store, &fold::views(&store.logs().0, clock()))
}

/// The flows that stopped since the master last saw them, marked seen, without waiting for more:
/// what a Stop hands over and saaio server types into an idle master.
pub fn master_news_now(store: &Store, flows: &[FlowView]) -> Vec<FlowView> {
    let mut seen = store.seen().unwrap_or_else(|| wake::first_seen(flows));
    let changed = wake::changed_flows(flows, &seen);
    if changed.is_empty() {
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
    use crate::event::{Event, EventKind};
    use crate::fixtures;

    fn started(store: &Store) -> String {
        store
            .create(&fixtures::started("csv", "SHOP-221 add CSV export"))
            .unwrap()
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
                Note::Master("use the old API".to_string()),
                Note::Master("and keep the tests".to_string())
            ])
        );
        assert_eq!(worker_news(&store, &id, 3), WorkerNews::Nothing);
        send(&store, &id, "one more");
        assert_eq!(
            worker_news(&store, &id, 3),
            WorkerNews::News(vec![Note::Master("one more".to_string())])
        );
    }

    #[test]
    fn delivers_a_merged_base_once_and_counts_it_with_the_messages() {
        let dir = TempDir::new().unwrap();
        let store = Store::at(dir.path().to_path_buf());
        let id = started(&store);
        send(&store, &id, "read first");
        let merged = EventKind::BaseMerged {
            base: "flow/a".into(),
            onto: "main".into(),
        };
        store.append(&id, &Event::new(2, merged)).unwrap();

        assert_eq!(
            worker_news(&store, &id, 3),
            WorkerNews::News(vec![
                Note::Master("read first".into()),
                Note::BaseMerged {
                    base: "flow/a".into(),
                    onto: "main".into()
                }
            ])
        );
        assert_eq!(store.delivered(&id), 2);
        assert_eq!(worker_news(&store, &id, 3), WorkerNews::Nothing);
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
    fn a_pr_open_worker_can_receive_the_masters_message() {
        let dir = TempDir::new().unwrap();
        let store = Store::at(dir.path().to_path_buf());
        let id = started(&store);
        store
            .append(
                &id,
                &Event::new(
                    2,
                    EventKind::MergeRequest {
                        url: "https://x/1".into(),
                    },
                ),
            )
            .unwrap();
        send(&store, &id, "Fix the race first");
        assert_eq!(
            worker_news(&store, &id, 3),
            WorkerNews::News(vec![Note::Master("Fix the race first".into())])
        );
    }

    #[test]
    fn the_master_has_no_news_before_a_flow_stops() {
        let dir = TempDir::new().unwrap();
        let store = Store::at(dir.path().to_path_buf());
        started(&store);

        assert_eq!(master_news(&store, || 3, &|| true), [] as [FlowView; 0]);
    }

    #[test]
    fn the_master_hears_of_a_stopped_flow_once_at_once() {
        let dir = TempDir::new().unwrap();
        let store = Store::at(dir.path().to_path_buf());
        let id = started(&store);
        let idle = EventKind::Idle {
            note: "plan written".to_string(),
        };
        store.append(&id, &Event::new(2, idle)).unwrap();
        let flows = || fold::views(&store.logs().0, 3);

        let news = master_news_now(&store, &flows());

        assert_eq!(
            news.iter().map(|flow| flow.id.as_str()).collect::<Vec<_>>(),
            [id.as_str()]
        );
        assert!(master_news_now(&store, &flows()).is_empty(), "seen now");
    }
}
