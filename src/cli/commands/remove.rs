//! `saaio remove`: stops a flow's worker, tears down its worktree and branch, forgets the flow

use std::path::Path;

use super::retire::{
    Uncommitted, Unmerged, close_pane, drop_branch, refuse_uncommitted_work,
    refuse_unmerged_commits, retire_worktree,
};
use super::{no_flow, views};
use crate::cli::output::remove::{self, BranchFate, Retired};
use crate::cli::output::{Document, Failure};
use crate::fold;
use crate::store::Store;
use crate::trust::Trust;

#[derive(Debug, clap::Args)]
pub struct Args {
    /// Flow name or id
    flow: String,
    /// Also remove a worktree with uncommitted work
    #[arg(long)]
    force: bool,
    /// Also delete the flow's branch when it has commits on no other branch
    #[arg(long)]
    delete_unmerged: bool,
}

/// Stops a flow's worker, tears down its worktree, deletes a merged branch and forgets the flow.
pub fn run(
    args: &Args,
    store: &Store,
    socket: &Path,
    trust: &Trust,
    now: u64,
) -> Result<Document, Failure> {
    let uncommitted = if args.force {
        Uncommitted::Discard
    } else {
        Uncommitted::Keep
    };
    let unmerged = if args.delete_unmerged {
        Unmerged::Delete
    } else {
        Unmerged::Keep
    };
    let (flows, _) = views(store, now);
    let flow = fold::resolve(&flows, &args.flow).ok_or_else(|| no_flow(&args.flow))?;
    if uncommitted == Uncommitted::Keep {
        refuse_uncommitted_work(flow)?;
    }
    if unmerged == Unmerged::Keep {
        refuse_unmerged_commits(flow)?;
    }
    let pane = close_pane(socket, flow)?;
    let retired = retire_worktree(trust, flow, uncommitted);
    let branch = match retired.0 {
        Retired::Kept(_) => BranchFate::Kept,
        Retired::NoWorktree | Retired::Removed => drop_branch(flow, unmerged),
    };
    store
        .remove(&flow.id)
        .map_err(|error| Failure::from(error.context(format!("could not remove {}", flow.name))))?;
    Ok(remove::removed(&flow.name, pane, &retired, branch))
}
