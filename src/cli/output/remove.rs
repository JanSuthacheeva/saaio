use std::fmt;

use serde::Serialize;

use super::{Document, Failure, toon};
use crate::client::Closing;
use crate::refusal::Refusal;
use crate::text::plural;

/// The refusal of flow `name`'s removal while its worktree at `path` has uncommitted work.
pub(crate) fn uncommitted(name: &str, path: &str) -> Failure {
    Failure::error(
        format!("{name} {} in {path}", Refusal::Uncommitted.phrase()),
        vec![
            format!("Commit or stash it in {path}, then run `saaio remove {name}` again"),
            format!("Run `saaio remove {name} --force` to discard the uncommitted work"),
        ],
    )
}

/// The refusal of flow `name`'s removal when git cannot say whether its worktree at `path` holds
/// uncommitted work, and why (no worktree git lists, git failing).
pub(crate) fn unchecked(name: &str, path: &str, reason: &anyhow::Error) -> Failure {
    Failure::error(
        format!("git cannot check {path} for uncommitted changes ({reason:#})"),
        vec![format!(
            "Run `saaio remove {name} --force` to remove it without the check"
        )],
    )
}

/// The refusal of flow `name`'s removal while its `branch` has `count` commits on no other branch.
pub(crate) fn unmerged(name: &str, branch: &str, count: u32) -> Failure {
    Failure::error(
        format!(
            "{branch} has {count} commit{} {}",
            plural(count),
            Refusal::Unmerged.phrase()
        ),
        vec![
            format!("Merge or push {branch}, then run `saaio remove {name}` again"),
            format!("Run `saaio remove {name} --delete-unmerged` to delete it with its commits"),
        ],
    )
}

/// What became of a flow's worktree when it was cleared or removed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Retired {
    NoWorktree,
    Removed,
    /// Kept for its uncommitted work, at this path.
    Kept(String),
}

/// none, removed, kept: path
impl fmt::Display for Retired {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Retired::NoWorktree => f.write_str("none"),
            Retired::Removed => f.write_str("removed"),
            Retired::Kept(path) => write!(f, "kept: {path}"),
        }
    }
}

/// What became of a flow's branch once its worktree was gone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BranchFate {
    NoBranch,
    Deleted,
    Kept,
}

impl BranchFate {
    pub fn name(self) -> &'static str {
        match self {
            BranchFate::NoBranch => "none",
            BranchFate::Deleted => "deleted",
            BranchFate::Kept => "kept",
        }
    }
}

fn pane_name(closing: Closing) -> &'static str {
    match closing {
        Closing::Closed => "closed",
        Closing::NoServer | Closing::NoPane => "none",
        Closing::TimedOut => "timed out",
    }
}

/// removed, pane (closed, none or timed out), worktree, branch (deleted, kept or none), help.
pub(crate) fn removed(
    name: &str,
    pane: Closing,
    worktree: &Retired,
    branch: BranchFate,
) -> Document {
    #[derive(Serialize)]
    struct Removed<'a> {
        removed: &'a str,
        pane: &'a str,
        worktree: String,
        branch: &'a str,
        help: Vec<String>,
    }
    let mut help = Vec::new();
    if let Retired::Kept(path) = worktree {
        help.push(format!(
            "Run `git worktree remove --force {path}` to discard what is left"
        ));
    }
    help.push("Run `saaio` to see what is left".to_string());
    Document::Toon(toon(&Removed {
        removed: name,
        pane: pane_name(pane),
        worktree: worktree.to_string(),
        branch: branch.name(),
        help,
    }))
}

/// cleared: n, names[n], kept[n]{name,path} for worktrees with uncommitted work, help.
pub(crate) fn cleared(names: &[String], kept: &[(String, String)]) -> Document {
    #[derive(Serialize)]
    struct Kept<'a> {
        name: &'a str,
        path: &'a str,
    }
    #[derive(Serialize)]
    struct Cleared<'a> {
        cleared: usize,
        #[serde(skip_serializing_if = "<[String]>::is_empty")]
        names: &'a [String],
        #[serde(skip_serializing_if = "Vec::is_empty")]
        kept: Vec<Kept<'a>>,
        help: Vec<&'static str>,
    }
    let mut help = vec!["Run `saaio` to see what is left"];
    if !kept.is_empty() {
        help.push("Commit or discard the work in a kept worktree, then remove it with `git worktree remove <path>`");
    }
    Document::Toon(toon(&Cleared {
        cleared: names.len(),
        names,
        kept: kept
            .iter()
            .map(|(name, path)| Kept { name, path })
            .collect(),
        help,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_each_refusal_back_from_its_own_message() {
        let uncommitted = uncommitted("shop-230", "/shop/.saaio/worktrees/shop-230");
        let unmerged = unmerged("shop-230", "flow/shop-230", 2);

        assert_eq!(
            Refusal::of(&uncommitted.message),
            Some(Refusal::Uncommitted)
        );
        assert_eq!(Refusal::of(&unmerged.message), Some(Refusal::Unmerged));
        assert_eq!(Refusal::of("no flow named shop-230"), None);
    }

    #[test]
    fn cleared_counts_and_names_the_removed_flows() {
        assert_eq!(
            cleared(&["export-csv".into(), "shop-198".into()], &[]).text(),
            "cleared: 2\nnames[2]: export-csv,shop-198\nhelp[1]: Run `saaio` to see what is left"
        );
        assert_eq!(
            cleared(&[], &[]).text(),
            "cleared: 0\nhelp[1]: Run `saaio` to see what is left"
        );
    }
}
