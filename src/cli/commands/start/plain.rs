use std::path::{Path, PathBuf};

use super::{Args, append_workspace, live_names, opening, pick_agent};
use crate::cli::commands::views;
use crate::cli::output::start;
use crate::cli::output::{Document, Failure};
use crate::config::Config;
use crate::event::{Event, EventKind, Mode};
use crate::fold;
use crate::launch::{name, worktree};
use crate::store::Store;

/// Started{mode: plain} and a Workspace with the folder; saaio ui opens the agent there.
pub(super) fn open(
    folder: &Path,
    args: &Args,
    store: &Store,
    config: &Config,
    cwd: &Path,
    home: Option<&Path>,
    now: u64,
) -> Result<Document, Failure> {
    let agent = pick_agent(config, args.agent.as_deref())?;
    let folder = resolve(folder, cwd, home)?;
    let (flows, _) = views(store, now);
    let base = args.name.clone().unwrap_or_else(|| {
        folder.file_name().map_or("session".to_string(), |last| {
            last.to_string_lossy().into_owned()
        })
    });
    let name = name::flow_name(Some(&base), "", &live_names(&flows));
    let first = Event::new(
        now,
        EventKind::Started {
            name,
            request: String::new(),
            mode: Mode::Plain,
            phases: None,
        },
    );
    let id = store.create(&first)?;
    let space = worktree::folder_workspace(&folder, agent);
    if let Err(error) = append_workspace(store, &id, &space, now) {
        let _ = store.remove(&id);
        return Err(error);
    }

    let events = store.events(&id)?;
    let flow = fold::fold(&id, &events, None, now).expect("a started flow folds");
    Ok(start::opened(&flow, &space, opening(store)))
}

/// `given` made absolute: a leading ~ from $HOME (the dialog passes it unexpanded), relative to
/// `cwd`; fails unless it is an existing folder, naming it with ~ for $HOME.
fn resolve(given: &Path, cwd: &Path, home: Option<&Path>) -> Result<PathBuf, Failure> {
    let expanded = match (given.strip_prefix("~"), home) {
        (Ok(rest), Some(home)) => home.join(rest),
        _ => cwd.join(given),
    };
    match expanded.canonicalize() {
        Ok(folder) if folder.is_dir() => Ok(folder),
        _ => {
            let shown = match home.and_then(|home| expanded.strip_prefix(home).ok()) {
                Some(rest) => Path::new("~").join(rest),
                None => expanded,
            };
            Err(Failure::error(
                format!("no folder {}", shown.display()),
                vec!["Name an existing folder".to_string()],
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    #[test]
    fn resolves_home_relative_and_absolute_folders() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().canonicalize().unwrap();
        std::fs::create_dir_all(root.join("a/b")).unwrap();

        assert_eq!(
            resolve(Path::new("~/a"), Path::new("/"), Some(&root)).unwrap(),
            root.join("a")
        );
        assert_eq!(
            resolve(Path::new("~"), Path::new("/"), Some(&root)).unwrap(),
            root
        );
        assert_eq!(
            resolve(Path::new("b"), &root.join("a"), None).unwrap(),
            root.join("a/b")
        );
        assert_eq!(
            resolve(Path::new("a/b/.."), &root, None).unwrap(),
            root.join("a")
        );
        assert_eq!(
            resolve(&root.join("a/b"), Path::new("/x"), None).unwrap(),
            root.join("a/b")
        );
        assert!(
            resolve(Path::new("~/a"), &root, None).is_err(),
            "no $HOME, a folder named ~"
        );
    }
}
