//! `saaio clear`: forgets ended flows and done ones that merge nothing, with their panes,
//! worktrees and merged branches, the flows on a merged one moved

use std::path::Path;

use super::retire::{Uncommitted, Unmerged, close_pane, drop_branch, restack, retire_worktree};
use super::views;
use crate::cli::output::remove::{self, Retired, Teardown};
use crate::cli::output::{Document, Failure};
use crate::store::Store;
use crate::trust::Trust;

/// Forgets ended flows and done ones that merge nothing, with their panes, worktrees and merged branches.
pub fn run(store: &Store, socket: &Path, trust: &Trust, now: u64) -> Result<Document, Failure> {
    let (flows, _) = views(store, now);
    let mut names = Vec::new();
    let mut kept = Vec::new();
    let mut untrusted = Vec::new();
    for flow in flows.iter().filter(|flow| flow.is_clearable()) {
        close_pane(socket, flow)?;
        let (retired, teardown) = retire_worktree(trust, flow, Uncommitted::Keep);
        if teardown == Teardown::Untrusted {
            untrusted.push(flow.name.clone());
        }
        match retired {
            Retired::Kept(path) => kept.push((flow.name.clone(), path)),
            Retired::NoWorktree | Retired::Removed => {
                drop_branch(flow, Unmerged::Keep);
            }
        }
        restack(store, &flows, flow, now)?;
        store.remove(&flow.id).map_err(|error| {
            Failure::from(error.context(format!("could not clear {}", flow.name)))
        })?;
        names.push(flow.name.clone());
    }
    Ok(remove::cleared(&names, &kept, &untrusted))
}
