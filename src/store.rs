use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use uuid::Uuid;

use crate::event::Event;

/// One flow's events and its last heartbeat, as read from disk.
#[derive(Debug, Clone, PartialEq)]
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

    #[cfg(test)]
    pub fn at(root: PathBuf) -> Store {
        Store { root }
    }

    /// The state directory itself, for server.sock, server.log and master.json.
    #[allow(dead_code)] // until flow ui uses it (plan-flow-ui.md)
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
    #[allow(dead_code)] // until flow ui uses it (plan-flow-ui.md)
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

    /// Deletes the log and the heartbeat file.
    pub fn remove(&self, id: &str) -> Result<()> {
        match fs::remove_file(self.log(id)) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => bail!("no flow {id}"),
            Err(error) => return Err(error).with_context(|| format!("could not remove flow {id}")),
        }
        match fs::remove_file(self.alive(id)) {
            Err(error) if error.kind() != ErrorKind::NotFound => {
                Err(error).with_context(|| format!("could not remove heartbeat of {id}"))
            }
            _ => Ok(()),
        }
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
}
