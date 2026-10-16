use std::fs::{self, File, OpenOptions};
use std::io::ErrorKind;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::registry::Registry;
use crate::store::write_atomic;

/// Slots across every project: FLOW_SLOT, which setup hooks turn into ports.
pub const MAX_SLOTS: u8 = 20;

/// slots/<n>: which flow in which store took slot n.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Claim {
    pub store: PathBuf,
    pub flow: String,
}

/// Hands out slots so no two unfinished flows in any project share one.
#[derive(Debug, Clone)]
pub struct Slots {
    dir: PathBuf, // <registry root>/slots
}

impl Slots {
    pub fn at(registry: &Registry) -> Slots {
        Slots {
            dir: registry.root().join("slots"),
        }
    }

    /// Under an exclusive lock: the lowest slot that is neither in `held` nor claimed by a flow `live`
    /// says is unfinished, written as this claim; None when all are taken.
    pub fn claim(
        &self,
        claim: &Claim,
        held: &[u8],
        live: impl Fn(&Claim) -> bool,
    ) -> Result<Option<u8>> {
        let _lock = self.lock()?;
        let free = (1..=MAX_SLOTS).find(|slot| {
            !held.contains(slot) && self.read(*slot).is_none_or(|taken| !live(&taken))
        });
        let Some(slot) = free else {
            return Ok(None);
        };
        write_atomic(&self.path(slot), serde_json::to_string(claim)?.as_bytes())
            .with_context(|| format!("could not claim slot {slot}"))?;
        Ok(Some(slot))
    }

    /// Gives a claim back when flow start fails after claiming; a slot claimed by another flow since
    /// stays as it is.
    pub fn release(&self, slot: u8, claim: &Claim) -> Result<()> {
        let _lock = self.lock()?;
        if self.read(slot).as_ref() != Some(claim) {
            return Ok(());
        }
        if let Err(error) = fs::remove_file(self.path(slot))
            && error.kind() != ErrorKind::NotFound
        {
            return Err(error).with_context(|| format!("could not release slot {slot}"));
        }
        Ok(())
    }

    /// An exclusive lock on slots/.lock, released when the file closes.
    fn lock(&self) -> Result<File> {
        fs::create_dir_all(&self.dir)
            .with_context(|| format!("could not create {}", self.dir.display()))?;
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(self.dir.join(".lock"))
            .with_context(|| format!("could not open {}", self.dir.join(".lock").display()))?;
        file.lock().context("could not lock the slots")?;
        Ok(file)
    }

    /// The claim in slots/<n>; None when there is none or it cannot be read.
    fn read(&self, slot: u8) -> Option<Claim> {
        let text = fs::read_to_string(self.path(slot)).ok()?;
        serde_json::from_str(&text).ok()
    }

    fn path(&self, slot: u8) -> PathBuf {
        self.dir.join(slot.to_string())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Barrier};
    use std::thread;

    use tempfile::TempDir;

    use super::*;

    fn slots(dir: &TempDir) -> Slots {
        Slots::at(&Registry::at(dir.path().to_path_buf()))
    }

    fn claim(flow: &str) -> Claim {
        Claim {
            store: PathBuf::from("/state/flow/projects/shop"),
            flow: flow.to_string(),
        }
    }

    #[test]
    fn claims_the_lowest_slot_nobody_holds() {
        let dir = TempDir::new().unwrap();
        let slots = slots(&dir);

        assert_eq!(slots.claim(&claim("a"), &[], |_| true).unwrap(), Some(1));
        assert_eq!(slots.claim(&claim("b"), &[2], |_| true).unwrap(), Some(3));
        assert_eq!(slots.claim(&claim("c"), &[2], |_| true).unwrap(), Some(4));

        let written = fs::read_to_string(dir.path().join("slots").join("3")).unwrap();
        assert_eq!(serde_json::from_str::<Claim>(&written).unwrap(), claim("b"));
    }

    #[test]
    fn reuses_the_slot_of_a_finished_flow() {
        let dir = TempDir::new().unwrap();
        let slots = slots(&dir);
        slots.claim(&claim("done"), &[], |_| true).unwrap();
        slots.claim(&claim("busy"), &[], |_| true).unwrap();

        let slot = slots
            .claim(&claim("next"), &[], |taken| taken.flow == "busy")
            .unwrap();

        assert_eq!(slot, Some(1));
    }

    #[test]
    fn has_no_slot_when_all_are_taken() {
        let dir = TempDir::new().unwrap();
        let slots = slots(&dir);
        let held: Vec<u8> = (1..=MAX_SLOTS).collect();

        assert_eq!(slots.claim(&claim("a"), &held, |_| true).unwrap(), None);
    }

    #[test]
    fn releases_only_its_own_claim() {
        let dir = TempDir::new().unwrap();
        let slots = slots(&dir);
        slots.claim(&claim("a"), &[], |_| true).unwrap();

        slots.release(1, &claim("other")).unwrap();
        assert!(dir.path().join("slots").join("1").exists());

        slots.release(1, &claim("a")).unwrap();
        assert!(!dir.path().join("slots").join("1").exists());
    }

    #[test]
    fn claims_at_the_same_time_get_different_slots() {
        let dir = TempDir::new().unwrap();
        let slots = slots(&dir);
        let start = Arc::new(Barrier::new(8));
        let handles: Vec<_> = (0..8)
            .map(|index| {
                let slots = slots.clone();
                let start = start.clone();
                thread::spawn(move || {
                    start.wait();
                    slots
                        .claim(&claim(&index.to_string()), &[], |_| true)
                        .unwrap()
                        .unwrap()
                })
            })
            .collect();

        let mut taken: Vec<u8> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        taken.sort_unstable();

        assert_eq!(taken, (1..=8).collect::<Vec<u8>>());
    }
}
