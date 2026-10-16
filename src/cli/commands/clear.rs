use super::retire::{Uncommitted, Unmerged, close_pane, drop_branch, retire_worktree};
use super::views;
use crate::cli::output::{self, Document, Failure, Retired};
use crate::store::Store;

/// Forgets the flows that are done or ended, with their panes, worktrees and merged branches.
pub fn run(store: &Store, now: u64) -> Result<Document, Failure> {
    let (flows, _) = views(store, now);
    let mut names = Vec::new();
    let mut kept = Vec::new();
    for flow in flows.iter().filter(|flow| flow.is_finished()) {
        close_pane(store, flow)?;
        match retire_worktree(flow, Uncommitted::Keep) {
            Retired::Kept(path) => kept.push((flow.name.clone(), path)),
            Retired::NoWorktree | Retired::Removed => {
                drop_branch(flow, Unmerged::Keep);
            }
        }
        store.remove(&flow.id).map_err(|error| {
            Failure::from(error.context(format!("could not clear {}", flow.name)))
        })?;
        names.push(flow.name.clone());
    }
    Ok(output::cleared(&names, &kept))
}
