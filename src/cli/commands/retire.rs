//! What remove and clear share: closing the pane, the teardown hook, the worktree and branch

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::cli::output::Failure;
use crate::cli::output::remove::{self, BranchFate, Retired, Teardown};
use crate::client::{self, Closing};
use crate::launch::worktree;
use crate::trust::Trust;
use crate::view::FlowView;
use crate::workspace::hooks::{self, Hook};
use crate::workspace::{self, git};

/// How long `saaio remove` and `saaio clear` wait for a flow's session to end (SIGKILL comes after 3 s).
const CLOSE_WAIT: Duration = Duration::from_secs(5);

/// What `saaio remove` does with uncommitted work in the worktree (--force discards it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Uncommitted {
    Keep,
    Discard,
}

/// What `saaio remove` does with a branch whose commits are on no other branch
/// (--delete-unmerged deletes it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unmerged {
    Keep,
    Delete,
}

/// Fails before anything is touched when the worktree remove would tear down holds uncommitted work;
/// a plain session's folder is never torn down, so never checked.
pub fn refuse_uncommitted_work(flow: &FlowView) -> Result<(), Failure> {
    let Some(space) = flow.workspace.as_ref() else {
        return Ok(());
    };
    let Some(repo) = worktree::repo_of(space) else {
        return Ok(());
    };
    if !Path::new(&space.path).exists() {
        return Ok(());
    }
    // Fails closed: a worktree git cannot answer for is refused too; --force still removes it.
    match git::has_uncommitted_work(&repo, space) {
        Ok(false) => Ok(()),
        Ok(true) => Err(remove::uncommitted(&flow.name, &space.path)),
        Err(error) => Err(remove::unchecked(&flow.name, &space.path, &error)),
    }
}

/// Fails before anything is touched when the flow's branch has commits on no other branch; the
/// refusal names the `stacked` flows standing on it.
pub fn refuse_unmerged_commits(flow: &FlowView, stacked: &[&str]) -> Result<(), Failure> {
    let Some((repo, branch)) = branch_of(flow) else {
        return Ok(());
    };
    match git::commits_only_on(&repo, branch) {
        Ok(Some(count)) if count > 0 => Err(remove::unmerged(&flow.name, branch, count, stacked)),
        _ => Ok(()),
    }
}

/// Ends the flow's session in saaio ui before its worktree goes.
pub fn close_pane(socket: &Path, flow: &FlowView) -> Result<Closing, Failure> {
    client::close_flow(socket, &flow.id, CLOSE_WAIT).map_err(Failure::from)
}

/// Runs the teardown hook when trust covers the repo and removes the flow's worktree; keeps it with
/// uncommitted work unless forced.
pub fn retire_worktree(
    trust: &Trust,
    flow: &FlowView,
    uncommitted: Uncommitted,
) -> (Retired, Teardown) {
    let Some(space) = &flow.workspace else {
        return (Retired::NoWorktree, Teardown::None);
    };
    let Some(repo) = worktree::repo_of(space) else {
        return (Retired::NoWorktree, Teardown::None);
    };
    if !Path::new(&space.path).exists() {
        return (Retired::NoWorktree, Teardown::None);
    }
    let teardown = if !hooks::exists(&repo, Hook::Teardown) {
        Teardown::None
    } else if !trust.covers(&repo) {
        Teardown::Untrusted
    } else {
        let env = worktree::hook_env(&flow.id, &flow.name, &repo, space);
        match hooks::run(trust, &repo, Hook::Teardown, space, &env) {
            Ok(true) => Teardown::Ran,
            Ok(false) => Teardown::None,
            Err(error) => Teardown::Failed(format!("{error:#}")),
        }
    };
    let force = uncommitted == Uncommitted::Discard;
    let retired = if workspace::remove_worktree(&repo, space, force).unwrap_or(false) {
        Retired::Removed
    } else {
        Retired::Kept(space.path.clone())
    };
    (retired, teardown)
}

/// Deletes the flow's branch once its worktree is gone: with Unmerged::Delete always, else only
/// when all its commits are on another branch.
pub fn drop_branch(flow: &FlowView, unmerged: Unmerged) -> BranchFate {
    let Some((repo, branch)) = branch_of(flow) else {
        return BranchFate::NoBranch;
    };
    match git::commits_only_on(&repo, branch) {
        Ok(None) => BranchFate::NoBranch,
        Ok(Some(count)) if count == 0 || unmerged == Unmerged::Delete => {
            match git::delete_branch(&repo, branch) {
                Ok(()) => BranchFate::Deleted,
                Err(_) => BranchFate::Kept,
            }
        }
        Ok(Some(_)) | Err(_) => BranchFate::Kept,
    }
}

/// The repository and branch of a started flow.
fn branch_of(flow: &FlowView) -> Option<(PathBuf, &str)> {
    let space = flow.workspace.as_ref()?;
    Some((worktree::repo_of(space)?, space.branch.as_deref()?))
}
