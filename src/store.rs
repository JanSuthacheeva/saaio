use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use uuid::Uuid;

use crate::event::Event;
use crate::wake::Seen;

/// What the waiters keep: each session's waiter pid, deliveries, the master's seen marks.
const WAITS_DIR: &str = "waits";
const SEEN_FILE: &str = "master.seen";

/// One flow's events and its last heartbeat, as read from disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlowLog {
    pub id: String,
    pub events: Vec<Event>,
    pub beat: Option<u64>,
}

/// What tells whether a flow's files changed: the log's size and the newest change of log or heartbeat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stamp {
    pub modified: SystemTime,
    pub len: u64,
}

/// Whose wait: the master's or a flow's worker's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Waiter {
    Master,
    Flow(String),
}

impl Waiter {
    fn key(&self) -> &str {
        match self {
            Waiter::Master => "master",
            Waiter::Flow(id) => id,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Store {
    root: PathBuf, // holds flows/<id>.jsonl and flows/<id>.alive
}

impl Store {
    /// $FLOW_HOME, else $XDG_STATE_HOME/flow, else $HOME/.local/state/flow.
    pub fn locate(
        flow_home: Option<PathBuf>,
        xdg_state: Option<PathBuf>,
        home: Option<PathBuf>,
    ) -> Result<Store> {
        let set = |path: Option<PathBuf>| path.filter(|path| !path.as_os_str().is_empty());
        let root = set(flow_home)
            .or_else(|| set(xdg_state).map(|state| state.join("flow")))
            .or_else(|| set(home).map(|home| home.join(".local/state/flow")));
        match root {
            Some(root) => Ok(Store { root }),
            None => bail!("cannot find a state directory"),
        }
    }

    /// The store in this folder, as the registry lists it.
    pub fn at(root: PathBuf) -> Store {
        Store { root }
    }

    /// The state directory itself, for server.sock, server.log and master.json.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// A new flow with a fresh UUID v4 and its first event; returns the id.
    pub fn create(&self, first: &Event) -> Result<String> {
        let flows = self.root.join("flows");
        fs::create_dir_all(&flows)
            .with_context(|| format!("could not create flow in {}", flows.display()))?;
        let id = Uuid::new_v4().to_string();
        let mut log = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(self.log(&id))
            .with_context(|| format!("could not create flow in {}", flows.display()))?;
        writeln!(log, "{}", first.to_line())
            .with_context(|| format!("could not write flow {id}"))?;
        Ok(id)
    }

    /// Appends one event to an existing log.
    pub fn append(&self, id: &str, event: &Event) -> Result<()> {
        let mut log = match OpenOptions::new().append(true).open(self.log(id)) {
            Ok(log) => log,
            Err(error) if error.kind() == ErrorKind::NotFound => bail!("no flow {id}"),
            Err(error) => return Err(error).with_context(|| format!("could not open flow {id}")),
        };
        log.write_all(format!("{}\n", event.to_line()).as_bytes())
            .with_context(|| format!("could not write flow {id}"))
    }

    /// Every readable event of one flow, in order.
    pub fn events(&self, id: &str) -> Result<Vec<Event>> {
        let text = match fs::read_to_string(self.log(id)) {
            Ok(text) => text,
            Err(error) if error.kind() == ErrorKind::NotFound => bail!("no flow {id}"),
            Err(error) => return Err(error).with_context(|| format!("could not read flow {id}")),
        };
        Ok(text.lines().filter_map(Event::from_line).collect())
    }

    /// Ids of all flows, by log file name.
    pub fn ids(&self) -> Result<Vec<String>> {
        let flows = self.root.join("flows");
        let entries = match fs::read_dir(&flows) {
            Ok(entries) => entries,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => {
                return Err(error).with_context(|| format!("could not list {}", flows.display()));
            }
        };
        let mut ids = Vec::new();
        for entry in entries {
            let name = entry?.file_name();
            if let Some(id) = name.to_string_lossy().strip_suffix(".jsonl") {
                ids.push(id.to_string());
            }
        }
        ids.sort();
        Ok(ids)
    }

    /// Every readable flow log, and a warning per log that could not be read.
    pub fn logs(&self) -> (Vec<FlowLog>, Vec<String>) {
        let ids = match self.ids() {
            Ok(ids) => ids,
            Err(error) => return (Vec::new(), vec![format!("{error:#}")]),
        };
        let mut logs = Vec::new();
        let mut warnings = Vec::new();
        for id in ids {
            match self
                .events(&id)
                .and_then(|events| Ok((events, self.last_beat(&id)?)))
            {
                Ok((events, beat)) => logs.push(FlowLog { id, events, beat }),
                Err(error) => warnings.push(format!("cannot read {id}: {error:#}")),
            }
        }
        (logs, warnings)
    }

    /// Size and modification time of each log and heartbeat file, by flow id.
    pub fn stamps(&self) -> Result<Vec<(String, Stamp)>> {
        let mut stamps = Vec::new();
        for id in self.ids()? {
            let log = match fs::metadata(self.log(&id)) {
                Ok(log) => log,
                Err(error) if error.kind() == ErrorKind::NotFound => continue,
                Err(error) => {
                    return Err(error).with_context(|| format!("could not read flow {id}"));
                }
            };
            let mut modified = log.modified()?;
            if let Ok(alive) = fs::metadata(self.alive(&id)) {
                modified = modified.max(alive.modified()?);
            }
            stamps.push((
                id,
                Stamp {
                    modified,
                    len: log.len(),
                },
            ));
        }
        Ok(stamps)
    }

    /// Touches <id>.alive; its mtime is the last heartbeat.
    pub fn beat(&self, id: &str) -> Result<()> {
        if !self.log(id).exists() {
            bail!("no flow {id}");
        }
        let alive = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(self.alive(id))
            .with_context(|| format!("could not mark flow {id} alive"))?;
        alive
            .set_modified(SystemTime::now())
            .with_context(|| format!("could not mark flow {id} alive"))
    }

    /// Unix ms of the last heartbeat; None when the flow never sent one.
    pub fn last_beat(&self, id: &str) -> Result<Option<u64>> {
        let modified = match fs::metadata(self.alive(id)) {
            Ok(metadata) => metadata.modified()?,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(error).with_context(|| format!("could not read heartbeat of {id}"));
            }
        };
        let since_epoch = modified.duration_since(UNIX_EPOCH).unwrap_or_default();
        Ok(Some(since_epoch.as_millis() as u64))
    }

    /// Makes `pid` the session's one waiter; an older waiter sees it was replaced.
    pub fn claim_waiter(&self, waiter: &Waiter, pid: u32) -> Result<()> {
        self.write_wait(&format!("{}.pid", waiter.key()), &pid.to_string())
    }

    /// The pid of the session's current waiter.
    pub fn waiter(&self, waiter: &Waiter) -> Option<u32> {
        self.read_wait(&format!("{}.pid", waiter.key()))?
            .trim()
            .parse()
            .ok()
    }

    /// How many of the master's messages the flow's worker was given.
    pub fn delivered(&self, id: &str) -> usize {
        self.read_wait(&format!("{id}.delivered"))
            .and_then(|text| text.trim().parse().ok())
            .unwrap_or(0)
    }

    pub fn set_delivered(&self, id: &str, count: usize) -> Result<()> {
        self.write_wait(&format!("{id}.delivered"), &count.to_string())
    }

    /// What the master was last woken for; None before its first wake.
    pub fn seen(&self) -> Option<Seen> {
        serde_json::from_str(&self.read_wait(SEEN_FILE)?).ok()
    }

    pub fn set_seen(&self, seen: &Seen) -> Result<()> {
        let text = serde_json::to_string(seen).expect("seen marks serialise");
        self.write_wait(SEEN_FILE, &text)
    }

    /// Deletes the log, the heartbeat file and what its waiter kept.
    pub fn remove(&self, id: &str) -> Result<()> {
        match fs::remove_file(self.log(id)) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => bail!("no flow {id}"),
            Err(error) => return Err(error).with_context(|| format!("could not remove flow {id}")),
        }
        let waits = self.root.join(WAITS_DIR);
        for file in [
            self.alive(id),
            waits.join(format!("{id}.pid")),
            waits.join(format!("{id}.delivered")),
        ] {
            if let Err(error) = fs::remove_file(&file)
                && error.kind() != ErrorKind::NotFound
            {
                return Err(error).with_context(|| format!("could not remove {}", file.display()));
            }
        }
        Ok(())
    }

    fn read_wait(&self, name: &str) -> Option<String> {
        fs::read_to_string(self.root.join(WAITS_DIR).join(name)).ok()
    }

    /// Written beside and renamed over, so a reader never sees half a file.
    fn write_wait(&self, name: &str, text: &str) -> Result<()> {
        let waits = self.root.join(WAITS_DIR);
        fs::create_dir_all(&waits)
            .with_context(|| format!("could not create {}", waits.display()))?;
        let file = waits.join(name);
        let partial = waits.join(format!(".{name}.{}", std::process::id()));
        fs::write(&partial, text)
            .and_then(|()| fs::rename(&partial, &file))
            .with_context(|| format!("could not write {}", file.display()))
    }

    fn log(&self, id: &str) -> PathBuf {
        self.root.join("flows").join(format!("{id}.jsonl"))
    }

    fn alive(&self, id: &str) -> PathBuf {
        self.root.join("flows").join(format!("{id}.alive"))
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use tempfile::TempDir;

    use super::*;
    use crate::event::{EventKind, Mode};
    use crate::view::State;
    use crate::wake::Mark;

    fn started() -> Event {
        Event::new(
            1,
            EventKind::Started {
                name: "shop-221".into(),
                request: "SHOP-221".into(),
                mode: Mode::Guided,
            },
        )
    }

    #[test]
    fn reads_every_log_with_its_heartbeat() {
        let dir = TempDir::new().unwrap();
        let store = Store::at(dir.path().to_path_buf());
        let first = store.create(&started()).unwrap();
        let second = store.create(&started()).unwrap();
        store.beat(&second).unwrap();

        let (logs, warnings) = store.logs();

        assert!(warnings.is_empty());
        assert_eq!(logs.len(), 2);
        let log = |id: &str| logs.iter().find(|log| log.id == id).unwrap();
        assert_eq!(log(&first).events, vec![started()]);
        assert_eq!(log(&first).beat, None);
        assert!(log(&second).beat.is_some());
    }

    #[test]
    fn a_stamp_changes_with_an_event_or_a_heartbeat() {
        let dir = TempDir::new().unwrap();
        let store = Store::at(dir.path().to_path_buf());
        let id = store.create(&started()).unwrap();
        let stamp = |store: &Store| store.stamps().unwrap()[0].1;

        let before = stamp(&store);
        store.append(&id, &started()).unwrap();
        let appended = stamp(&store);
        std::thread::sleep(std::time::Duration::from_millis(20));
        store.beat(&id).unwrap();
        let beaten = stamp(&store);

        assert!(appended.len > before.len);
        assert!(beaten.modified > appended.modified);
        assert_eq!(beaten.len, appended.len);
    }

    #[test]
    fn locates_flow_home_first_then_xdg_state_then_home() {
        let home = Some(PathBuf::from("/home/jan"));
        let xdg = Some(PathBuf::from("/state"));
        let explicit = Some(PathBuf::from("/flows"));

        assert_eq!(
            Store::locate(explicit, xdg.clone(), home.clone())
                .unwrap()
                .root,
            Path::new("/flows")
        );
        assert_eq!(
            Store::locate(None, xdg, home.clone()).unwrap().root,
            Path::new("/state/flow")
        );
        assert_eq!(
            Store::locate(None, None, home).unwrap().root,
            Path::new("/home/jan/.local/state/flow")
        );
        assert!(Store::locate(None, None, None).is_err());
    }

    #[test]
    fn ignores_empty_environment_values() {
        let store = Store::locate(
            Some(PathBuf::new()),
            Some(PathBuf::new()),
            Some(PathBuf::from("/home/jan")),
        )
        .unwrap();
        assert_eq!(store.root, Path::new("/home/jan/.local/state/flow"));
    }

    #[test]
    fn creates_a_flow_and_appends_to_it_in_order() {
        let dir = TempDir::new().unwrap();
        let store = Store::at(dir.path().to_path_buf());

        let id = store.create(&started()).unwrap();
        let working = Event::new(2, EventKind::Working);
        store.append(&id, &working).unwrap();

        assert!(Uuid::parse_str(&id).is_ok());
        assert_eq!(store.events(&id).unwrap(), vec![started(), working]);
        assert_eq!(store.ids().unwrap(), vec![id]);
    }

    #[test]
    fn refuses_to_append_to_a_flow_that_does_not_exist() {
        let dir = TempDir::new().unwrap();
        let store = Store::at(dir.path().to_path_buf());

        let error = store
            .append("nope", &Event::new(1, EventKind::Working))
            .unwrap_err();

        assert_eq!(error.to_string(), "no flow nope");
        assert!(!dir.path().join("flows/nope.jsonl").exists());
    }

    #[test]
    fn skips_unreadable_lines_when_reading_events() {
        let dir = TempDir::new().unwrap();
        let store = Store::at(dir.path().to_path_buf());
        let id = store.create(&started()).unwrap();
        let log = dir.path().join("flows").join(format!("{id}.jsonl"));
        let mut text = fs::read_to_string(&log).unwrap();
        text.push_str("garbage\n{\"v\":9,\"at\":3,\"kind\":\"working\"}\n");
        fs::write(&log, text).unwrap();

        assert_eq!(store.events(&id).unwrap(), vec![started()]);
    }

    #[test]
    fn remembers_the_last_heartbeat_of_a_flow() {
        let dir = TempDir::new().unwrap();
        let store = Store::at(dir.path().to_path_buf());
        let id = store.create(&started()).unwrap();
        assert_eq!(store.last_beat(&id).unwrap(), None);

        let before = now_ms();
        store.beat(&id).unwrap();
        let beat = store.last_beat(&id).unwrap().unwrap();

        assert!(
            beat + 1_000 >= before && beat <= now_ms() + 1_000,
            "{beat} vs {before}"
        );
    }

    #[test]
    fn refuses_a_heartbeat_for_a_flow_that_does_not_exist() {
        let dir = TempDir::new().unwrap();
        let store = Store::at(dir.path().to_path_buf());

        assert_eq!(store.beat("nope").unwrap_err().to_string(), "no flow nope");
    }

    #[test]
    fn removes_the_log_and_the_heartbeat() {
        let dir = TempDir::new().unwrap();
        let store = Store::at(dir.path().to_path_buf());
        let id = store.create(&started()).unwrap();
        store.beat(&id).unwrap();

        store.remove(&id).unwrap();

        assert_eq!(store.ids().unwrap(), Vec::<String>::new());
        assert_eq!(store.last_beat(&id).unwrap(), None);
        assert_eq!(
            store.remove(&id).unwrap_err().to_string(),
            format!("no flow {id}")
        );
    }

    fn now_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
    }

    #[test]
    fn lists_no_ids_before_any_flow_exists() {
        let dir = TempDir::new().unwrap();
        let store = Store::at(dir.path().join("missing"));

        assert_eq!(store.ids().unwrap(), Vec::<String>::new());
    }

    #[test]
    fn the_last_waiter_to_claim_a_session_is_its_waiter() {
        let dir = TempDir::new().unwrap();
        let store = Store::at(dir.path().to_path_buf());
        let worker = Waiter::Flow("7f3c".into());

        assert_eq!(store.waiter(&worker), None);
        store.claim_waiter(&worker, 41).unwrap();
        store.claim_waiter(&Waiter::Master, 7).unwrap();
        store.claim_waiter(&worker, 42).unwrap();

        assert_eq!(store.waiter(&worker), Some(42));
        assert_eq!(store.waiter(&Waiter::Master), Some(7));
    }

    #[test]
    fn remembers_what_was_delivered_and_what_the_master_saw() {
        let dir = TempDir::new().unwrap();
        let store = Store::at(dir.path().to_path_buf());

        assert_eq!(store.delivered("7f3c"), 0);
        store.set_delivered("7f3c", 3).unwrap();
        assert_eq!(store.delivered("7f3c"), 3);

        assert_eq!(store.seen(), None);
        let seen = Seen::from([(
            "7f3c".to_string(),
            Mark {
                state: State::Waiting,
                changed_at: 5,
            },
        )]);
        store.set_seen(&seen).unwrap();
        assert_eq!(store.seen(), Some(seen));
    }

    #[test]
    fn removing_a_flow_forgets_its_waiter_and_deliveries() {
        let dir = TempDir::new().unwrap();
        let store = Store::at(dir.path().to_path_buf());
        let id = store.create(&started()).unwrap();
        store.claim_waiter(&Waiter::Flow(id.clone()), 9).unwrap();
        store.set_delivered(&id, 2).unwrap();

        store.remove(&id).unwrap();

        assert_eq!(store.waiter(&Waiter::Flow(id.clone())), None);
        assert_eq!(store.delivered(&id), 0);
    }
}
