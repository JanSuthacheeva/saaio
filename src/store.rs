use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::{Context, Result, bail};
use uuid::Uuid;

use crate::event::{Event, unix_ms};
use crate::private::{self, write_atomic};
use crate::view::MasterTurn;
use crate::wake::Seen;

/// What the waiters keep: each session's waiter pid, deliveries, the master's seen marks.
const WAITS_DIR: &str = "waits";
const SEEN_FILE: &str = "master.seen";
const MASTER_SESSION_FILE: &str = "master.session";
const MASTER_TURN_FILE: &str = "master.turn";
const BUSY_TURN: &str = "turn";
const BUSY_TYPED: &str = "typed";

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

impl Stamp {
    /// The file's size and time; None when it is missing or unreadable.
    pub fn of(path: &Path) -> Option<Stamp> {
        let metadata = fs::metadata(path).ok()?;
        Some(Stamp {
            modified: metadata.modified().ok()?,
            len: metadata.len(),
        })
    }
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

/// Whether a session runs a turn, as `waits/<key>.busy` says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Busy {
    /// At its prompt: no busy file.
    Idle,
    /// A hook or `saaio wait` began a turn ("turn", or an empty file an earlier saaio wrote).
    Turn,
    /// The server typed into the pane and no turn began yet ("typed").
    Typed,
}

#[derive(Debug, Clone)]
pub struct Store {
    root: PathBuf, // holds flows/<id>.jsonl and flows/<id>.alive
}

impl Store {
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
        private::create_dir(&flows)
            .with_context(|| format!("could not create flow in {}", flows.display()))?;
        let id = Uuid::new_v4().to_string();
        let mut log = private::options()
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
            let name = entry
                .with_context(|| format!("could not list {}", flows.display()))?
                .file_name();
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
            let changed = |meta: &fs::Metadata| {
                meta.modified()
                    .with_context(|| format!("could not read when flow {id} changed"))
            };
            let mut modified = changed(&log)?;
            if let Ok(alive) = fs::metadata(self.alive(&id)) {
                modified = modified.max(changed(&alive)?);
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
        let alive = private::options()
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
        Ok(Some(unix_ms(modified)))
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

    /// How many of the flow's queued lines the server typed into its pane.
    pub fn typed(&self, id: &str) -> usize {
        self.read_wait(&format!("{id}.typed"))
            .and_then(|text| text.trim().parse().ok())
            .unwrap_or(0)
    }

    pub fn set_typed(&self, id: &str, count: usize) -> Result<()> {
        self.write_wait(&format!("{id}.typed"), &count.to_string())
    }

    /// Marks the session as running a turn, as typed into by the server, or as idle at its
    /// prompt: the server types news and queued lines only into an idle one.
    pub fn set_busy(&self, waiter: &Waiter, busy: Busy) -> Result<()> {
        let name = format!("{}.busy", waiter.key());
        match busy {
            Busy::Idle => self.remove_wait(&name),
            Busy::Turn => self.write_wait(&name, BUSY_TURN),
            Busy::Typed => self.write_wait(&name, BUSY_TYPED),
        }
    }

    pub fn busy(&self, waiter: &Waiter) -> Busy {
        match self.read_wait(&format!("{}.busy", waiter.key())) {
            None => Busy::Idle,
            Some(text) if text.trim() == BUSY_TYPED => Busy::Typed,
            Some(_) => Busy::Turn,
        }
    }

    /// The master's own session id, for a harness that picks it itself (Codex).
    pub fn set_master_session(&self, id: &str) -> Result<()> {
        self.write_wait(MASTER_SESSION_FILE, id)
    }

    /// Forgets the master's session, as a fresh master starts a new one.
    pub fn forget_master_session(&self) -> Result<()> {
        self.remove_wait(MASTER_SESSION_FILE)
    }

    pub fn master_session(&self) -> Option<String> {
        let id = self.read_wait(MASTER_SESSION_FILE)?;
        Some(id.trim().to_string()).filter(|id| !id.is_empty())
    }

    /// The master's turn; None before its first hook or when the file does not parse.
    pub fn master_turn(&self) -> Option<MasterTurn> {
        serde_json::from_str(&self.read_wait(MASTER_TURN_FILE)?).ok()
    }

    pub fn set_master_turn(&self, turn: &MasterTurn) -> Result<()> {
        let text = serde_json::to_string(turn).expect("a master turn serialises");
        self.write_wait(MASTER_TURN_FILE, &text)
    }

    /// Forgets it, as a master process starts without a turn.
    pub fn forget_master_turn(&self) -> Result<()> {
        self.remove_wait(MASTER_TURN_FILE)
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
            waits.join(format!("{id}.typed")),
            waits.join(format!("{id}.busy")),
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

    fn remove_wait(&self, name: &str) -> Result<()> {
        let file = self.root.join(WAITS_DIR).join(name);
        match fs::remove_file(&file) {
            Err(error) if error.kind() != ErrorKind::NotFound => {
                Err(error).with_context(|| format!("could not remove {}", file.display()))
            }
            _ => Ok(()),
        }
    }

    fn write_wait(&self, name: &str, text: &str) -> Result<()> {
        let waits = self.root.join(WAITS_DIR);
        private::create_dir(&waits)
            .with_context(|| format!("could not create {}", waits.display()))?;
        let file = waits.join(name);
        write_atomic(&file, text.as_bytes())
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
    use tempfile::TempDir;

    use super::*;
    use crate::event::EventKind;
    use crate::fixtures;
    use crate::view::{MasterTurn, State};
    use crate::wake::Mark;

    #[test]
    fn stamps_a_file_by_size_and_none_when_missing() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        assert_eq!(Stamp::of(&path), None);
        fs::write(&path, "abc").unwrap();
        assert_eq!(Stamp::of(&path).map(|stamp| stamp.len), Some(3));
    }

    fn started() -> Event {
        fixtures::started("shop-221", "SHOP-221")
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
    fn keeps_the_masters_session() {
        let dir = TempDir::new().unwrap();
        let store = Store::at(dir.path().to_path_buf());

        assert_eq!(store.master_session(), None);
        store.set_master_session("01a1").unwrap();
        assert_eq!(store.master_session().as_deref(), Some("01a1"));
    }

    #[test]
    fn keeps_and_forgets_the_masters_turn() {
        let dir = TempDir::new().unwrap();
        let store = Store::at(dir.path().to_path_buf());
        let turn = MasterTurn {
            state: State::Waiting,
            at: 1_792_157_011_000,
        };

        assert_eq!(store.master_turn(), None);
        store.set_master_turn(&turn).unwrap();
        assert_eq!(store.master_turn(), Some(turn));
        assert_eq!(
            fs::read_to_string(dir.path().join("waits/master.turn")).unwrap(),
            r#"{"state":"waiting","at":1792157011000}"#
        );

        store.forget_master_turn().unwrap();
        store.forget_master_turn().unwrap();
        assert_eq!(store.master_turn(), None, "forgetting twice is no error");
    }

    #[test]
    fn a_master_turn_that_does_not_parse_is_none() {
        let dir = TempDir::new().unwrap();
        let store = Store::at(dir.path().to_path_buf());
        fs::create_dir_all(dir.path().join("waits")).unwrap();
        fs::write(dir.path().join("waits/master.turn"), "{\"state\":").unwrap();

        assert_eq!(store.master_turn(), None);
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
        unix_ms(SystemTime::now())
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

        assert_eq!(store.typed("7f3c"), 0);
        store.set_typed("7f3c", 2).unwrap();
        assert_eq!(store.typed("7f3c"), 2);

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
    fn marks_a_session_idle_running_a_turn_or_typed_into() {
        let dir = TempDir::new().unwrap();
        let store = Store::at(dir.path().to_path_buf());
        let worker = Waiter::Flow("7f3c".into());

        assert_eq!(store.busy(&worker), Busy::Idle);
        store.set_busy(&worker, Busy::Typed).unwrap();
        assert_eq!(store.busy(&worker), Busy::Typed);
        store.set_busy(&worker, Busy::Turn).unwrap();
        assert_eq!(store.busy(&worker), Busy::Turn);
        store.set_busy(&worker, Busy::Idle).unwrap();
        assert_eq!(store.busy(&worker), Busy::Idle);
        assert!(!dir.path().join("waits/7f3c.busy").exists());
        store.set_busy(&worker, Busy::Idle).unwrap();

        fs::write(dir.path().join("waits/7f3c.busy"), "").unwrap();
        assert_eq!(
            store.busy(&worker),
            Busy::Turn,
            "the empty file an earlier saaio wrote"
        );
    }

    #[test]
    fn removing_a_flow_forgets_its_waiter_deliveries_and_typed_lines() {
        let dir = TempDir::new().unwrap();
        let store = Store::at(dir.path().to_path_buf());
        let id = store.create(&started()).unwrap();
        store.claim_waiter(&Waiter::Flow(id.clone()), 9).unwrap();
        store.set_delivered(&id, 2).unwrap();
        fs::write(dir.path().join(format!("waits/{id}.typed")), "1").unwrap();

        store.remove(&id).unwrap();

        assert_eq!(store.waiter(&Waiter::Flow(id.clone())), None);
        assert_eq!(store.delivered(&id), 0);
        assert_eq!(store.typed(&id), 0);
    }
}
