//! `Turns`: who may run a heavy command now, as flocks on `turns/<n>.lock`

use std::fs::{File, TryLockError};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::private;
use crate::store::Store;

/// Who holds a turn: written into the locked file for the flows that wait.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Holder {
    pub flow: String,
    pub name: String,
    pub command: String,
}

/// The turns of one scope: `limit` lock files in `dir`. A project's store has one; a machine-wide
/// scope would keep its own in the shared folder.
#[derive(Debug, Clone)]
pub struct Turns {
    dir: PathBuf,
    limit: u8,
}

/// A turn taken: its file stays locked while it is open, and the kernel lets go once no process
/// holds it any more.
#[derive(Debug)]
pub struct Turn {
    file: File,
}

#[derive(Debug)]
pub enum Take {
    Taken(Turn),
    /// Every turn is held; the holder of the first.
    Held(Option<Holder>),
}

impl Turns {
    /// <store>/turns, one turn.
    pub fn of_project(store: &Store) -> Turns {
        Turns {
            dir: store.root().join("turns"),
            limit: 1,
        }
    }

    /// Tries every lock file without waiting; the first free one is taken with `holder` written
    /// into it.
    pub fn try_take(&self, holder: &Holder) -> Result<Take> {
        private::create_dir(&self.dir)
            .with_context(|| format!("could not create {}", self.dir.display()))?;
        let mut first = None;
        for n in 0..self.limit {
            let path = self.dir.join(format!("{n}.lock"));
            let mut file = private::options()
                .create(true)
                .truncate(false)
                .read(true)
                .write(true)
                .open(&path)
                .with_context(|| format!("could not open {}", path.display()))?;
            match file.try_lock() {
                Ok(()) => {
                    write_holder(&mut file, holder)
                        .with_context(|| format!("could not write {}", path.display()))?;
                    return Ok(Take::Taken(Turn { file }));
                }
                Err(TryLockError::WouldBlock) => {
                    if n == 0 {
                        first = read_holder(&mut file);
                    }
                }
                Err(TryLockError::Error(error)) => {
                    return Err(error)
                        .with_context(|| format!("could not lock {}", path.display()));
                }
            }
        }
        Ok(Take::Held(first))
    }
}

impl Turn {
    /// The locked file, to hand to the process that keeps the turn.
    pub fn into_file(self) -> File {
        self.file
    }
}

/// The holder in place: a new file would be a new inode, and so a second lock.
fn write_holder(file: &mut File, holder: &Holder) -> Result<()> {
    file.set_len(0)?;
    file.seek(SeekFrom::Start(0))?;
    file.write_all(serde_json::to_string(holder)?.as_bytes())?;
    Ok(())
}

/// The holder a locked file names; None while it is still being written or unreadable.
fn read_holder(file: &mut File) -> Option<Holder> {
    let mut text = String::new();
    file.read_to_string(&mut text).ok()?;
    serde_json::from_str(&text).ok()
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    fn holder(flow: &str, command: &str) -> Holder {
        Holder {
            flow: flow.to_string(),
            name: format!("{flow}-name"),
            command: command.to_string(),
        }
    }

    fn turns() -> (TempDir, Turns) {
        let dir = TempDir::new().unwrap();
        let turns = Turns::of_project(&Store::at(dir.path().to_path_buf()));
        (dir, turns)
    }

    #[test]
    fn takes_a_free_turn() {
        let (_dir, turns) = turns();
        assert!(matches!(
            turns.try_take(&holder("a", "pint")).unwrap(),
            Take::Taken(_)
        ));
    }

    #[test]
    fn names_the_holder_while_it_is_held() {
        let (_dir, turns) = turns();
        let _taken = turns.try_take(&holder("a", "artisan test")).unwrap();
        let Take::Held(by) = turns.try_take(&holder("b", "pint")).unwrap() else {
            panic!("the turn is held");
        };
        assert_eq!(by, Some(holder("a", "artisan test")));
    }

    #[test]
    fn the_same_flow_waits_for_its_own_turn_too() {
        let (_dir, turns) = turns();
        let _taken = turns.try_take(&holder("a", "pint")).unwrap();
        assert!(matches!(
            turns.try_take(&holder("a", "pint")).unwrap(),
            Take::Held(Some(_))
        ));
    }

    #[test]
    fn frees_the_turn_once_its_file_closes() {
        let (_dir, turns) = turns();
        let Take::Taken(turn) = turns.try_take(&holder("a", "pint")).unwrap() else {
            panic!("the turn is free");
        };
        let file = turn.into_file();
        assert!(matches!(
            turns.try_take(&holder("b", "pest")).unwrap(),
            Take::Held(_)
        ));
        drop(file);
        assert!(matches!(
            turns.try_take(&holder("b", "pest")).unwrap(),
            Take::Taken(_)
        ));
    }

    #[test]
    fn a_shorter_holder_replaces_a_longer_one_whole() {
        let (_dir, turns) = turns();
        drop(turns.try_take(&holder("a-long-flow", "artisan test --parallel")));
        let _taken = turns.try_take(&holder("b", "pint")).unwrap();
        let Take::Held(by) = turns.try_take(&holder("c", "pest")).unwrap() else {
            panic!("the turn is held");
        };
        assert_eq!(by, Some(holder("b", "pint")));
    }
}
