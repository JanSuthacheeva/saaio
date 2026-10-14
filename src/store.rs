use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use uuid::Uuid;

use crate::event::Event;

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

    pub fn at(root: PathBuf) -> Store {
        Store { root }
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

    fn log(&self, id: &str) -> PathBuf {
        self.root.join("flows").join(format!("{id}.jsonl"))
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
    fn lists_no_ids_before_any_flow_exists() {
        let dir = TempDir::new().unwrap();
        let store = Store::at(dir.path().join("missing"));

        assert_eq!(store.ids().unwrap(), Vec::<String>::new());
    }
}
