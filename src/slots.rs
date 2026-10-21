//! `Slots`: `SAAIO_SLOT` claimed across every project under a flock; the slots flows hold

use std::fs::{self, File};
use std::io::ErrorKind;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::private::{self, write_atomic};
use crate::registry::Registry;
use crate::view::FlowView;

/// Slots across every project: SAAIO_SLOT, which setup hooks turn into ports.
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

    /// Gives a claim back when saaio start fails after claiming; a slot claimed by another flow since
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
        private::create_dir(&self.dir)
            .with_context(|| format!("could not create {}", self.dir.display()))?;
        let file = private::options()
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

/// The slots unfinished flows hold.
pub fn taken(flows: &[FlowView]) -> Vec<u8> {
    flows
        .iter()
        .filter(|flow| !flow.is_finished())
        .filter_map(|flow| flow.workspace.as_ref().and_then(|workspace| workspace.slot))
        .collect()
}

/// The lowest slot 1..=MAX_SLOTS no unfinished flow of this project holds; saaio start decides across
/// projects.
pub fn free_slot(flows: &[FlowView]) -> Option<u8> {
    let held = taken(flows);
    (1..=MAX_SLOTS).find(|slot| !held.contains(slot))
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Barrier};
    use std::thread;

    use tempfile::TempDir;

    use super::*;
    use crate::event::{Event, EventKind, Mode};
    use crate::fold;

    fn flow_in_slot(slot: u8, finished: bool) -> FlowView {
        let mut events = vec![Event::new(
            0,
            EventKind::Started {
                name: format!("f{slot}"),
                request: "x".into(),
                mode: Mode::Guided,
                phases: None,
            },
        )];
        events.push(Event::new(
            1,
            EventKind::Workspace {
                path: "/p".into(),
                branch: Some("b".into()),
                slot: Some(slot),
                window: None,
                agent: "claude".into(),
            },
        ));
        if finished {
            events.push(Event::new(2, EventKind::Ended));
        }
        fold::fold("id", &events, None, 3).unwrap()
    }

    #[test]
    fn gives_the_lowest_slot_no_running_flow_holds() {
        assert_eq!(free_slot(&[]), Some(1));
        assert_eq!(
            free_slot(&[flow_in_slot(1, false), flow_in_slot(3, false)]),
            Some(2)
        );
        assert_eq!(free_slot(&[flow_in_slot(1, true)]), Some(1));
        let full: Vec<FlowView> = (1..=MAX_SLOTS)
            .map(|slot| flow_in_slot(slot, false))
            .collect();
        assert_eq!(free_slot(&full), None);
    }

    #[test]
    fn lists_the_slots_of_unfinished_flows() {
        let flows = [flow_in_slot(4, false), flow_in_slot(2, true)];
        assert_eq!(taken(&flows), vec![4]);
    }

    #[test]
    fn a_pr_open_worker_keeps_its_slot_until_it_ends() {
        let mut live = flow_in_slot(4, false);
        live.state = crate::view::State::Done;
        live.merge_request = Some("https://github.com/a/b/pull/7".into());
        assert_eq!(taken(&[live.clone()]), vec![4]);
        live.state = crate::view::State::Ended;
        assert!(taken(&[live]).is_empty());
    }

    fn slots(dir: &TempDir) -> Slots {
        Slots::at(&Registry::at(dir.path().to_path_buf()))
    }

    fn claim(flow: &str) -> Claim {
        Claim {
            store: PathBuf::from("/state/saaio/projects/shop"),
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
