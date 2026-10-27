//! What `saaio remove` and `saaio clear` print, and the work `saaio remove` refuses to lose

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

/// The refusal of flow `name`'s removal while its `branch` has `count` commits on no other branch;
/// the `stacked` flows standing on it named in its help.
pub(crate) fn unmerged(name: &str, branch: &str, count: u32, stacked: &[&str]) -> Failure {
    let mut help = vec![
        format!("Merge or push {branch}, then run `saaio remove {name}` again"),
        format!("Run `saaio remove {name} --delete-unmerged` to delete it with its commits"),
    ];
    help.extend(standing(branch, stacked));
    Failure::error(
        format!(
            "{branch} has {count} commit{} {}",
            plural(count),
            Refusal::Unmerged.phrase()
        ),
        help,
    )
}

/// The help line naming the flows that stand on `branch`; none without any.
fn standing(branch: &str, stacked: &[&str]) -> Option<String> {
    let (last, rest) = stacked.split_last()?;
    Some(if rest.is_empty() {
        format!(
            "{last} stands on {branch}: it keeps its commits and rebases onto origin/{branch} or \
where that merged"
        )
    } else {
        format!(
            "{} and {last} stand on {branch}: they keep their commits and rebase onto \
origin/{branch} or where that merged",
            rest.join(", ")
        )
    })
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

/// What the repo's .saaio/teardown did before the worktree went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Teardown {
    /// No worktree, or no teardown script.
    None,
    Ran,
    Failed(String),
    /// The person has not trusted the repository, so its script did not run.
    Untrusted,
}

/// none, ran, failed: reason, not trusted
impl fmt::Display for Teardown {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Teardown::None => f.write_str("none"),
            Teardown::Ran => f.write_str("ran"),
            Teardown::Failed(reason) => write!(f, "failed: {reason}"),
            Teardown::Untrusted => f.write_str("not run, the repository is not trusted"),
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

/// removed, pane (closed, none or timed out), teardown, worktree, branch (deleted, kept or none),
/// stacked[n] (the flows standing on `on`, its branch), help.
pub(crate) fn removed(
    name: &str,
    pane: Closing,
    (worktree, teardown): &(Retired, Teardown),
    branch: BranchFate,
    (on, stacked): (&str, &[&str]),
) -> Document {
    #[derive(Serialize)]
    struct Removed<'a> {
        removed: &'a str,
        pane: &'a str,
        teardown: String,
        worktree: String,
        branch: &'a str,
        #[serde(skip_serializing_if = "<[&str]>::is_empty")]
        stacked: &'a [&'a str],
        help: Vec<String>,
    }
    let mut help: Vec<String> = standing(on, stacked).into_iter().collect();
    if let Retired::Kept(path) = worktree {
        help.push(format!(
            "Run `git worktree remove --force {path}` to discard what is left"
        ));
    }
    if *teardown == Teardown::Untrusted {
        help.push(UNTRUSTED_TEARDOWN.to_string());
    }
    help.push("Run `saaio` to see what is left".to_string());
    Document::Toon(toon(&Removed {
        removed: name,
        pane: pane_name(pane),
        teardown: teardown.to_string(),
        worktree: worktree.to_string(),
        branch: branch.name(),
        stacked,
        help,
    }))
}

/// The help line when a teardown did not run because the repository is not trusted.
const UNTRUSTED_TEARDOWN: &str =
    "Stop what the flow's setup started yourself or run `saaio trust` before the next removal";

/// cleared: n, names[n], kept[n]{name,path} for worktrees with uncommitted work, untrusted[n]
/// for flows whose teardown did not run, help.
pub(crate) fn cleared(
    names: &[String],
    kept: &[(String, String)],
    untrusted: &[String],
) -> Document {
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
        #[serde(skip_serializing_if = "<[String]>::is_empty")]
        untrusted: &'a [String],
        help: Vec<&'static str>,
    }
    let mut help = vec!["Run `saaio` to see what is left"];
    if !kept.is_empty() {
        help.push("Commit or discard the work in a kept worktree, then remove it with `git worktree remove <path>`");
    }
    if !untrusted.is_empty() {
        help.push(UNTRUSTED_TEARDOWN);
    }
    Document::Toon(toon(&Cleared {
        cleared: names.len(),
        names,
        kept: kept
            .iter()
            .map(|(name, path)| Kept { name, path })
            .collect(),
        untrusted,
        help,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_each_refusal_back_from_its_own_message() {
        let uncommitted = uncommitted("shop-230", "/shop/.saaio/worktrees/shop-230");
        let unmerged = unmerged("shop-230", "flow/shop-230", 2, &["shop-231"]);

        assert_eq!(
            Refusal::of(&uncommitted.message),
            Some(Refusal::Uncommitted)
        );
        assert_eq!(Refusal::of(&unmerged.message), Some(Refusal::Unmerged));
        assert_eq!(Refusal::of("no flow named shop-230"), None);
    }

    #[test]
    fn names_the_flows_standing_on_a_removed_or_refused_branch() {
        let refused = unmerged("shop-230", "flow/shop-230", 2, &["shop-231", "shop-232"]);
        let stacked = removed(
            "shop-230",
            Closing::Closed,
            &(Retired::Removed, Teardown::None),
            BranchFate::Deleted,
            ("flow/shop-230", &["shop-231"]),
        );
        let alone = removed(
            "shop-230",
            Closing::Closed,
            &(Retired::Removed, Teardown::None),
            BranchFate::Deleted,
            ("flow/shop-230", &[]),
        );

        assert_eq!(
            refused.help.last().map(String::as_str),
            Some(
                "shop-231 and shop-232 stand on flow/shop-230: they keep their commits and \
rebase onto origin/flow/shop-230 or where that merged"
            )
        );
        assert_eq!(
            stacked.text(),
            "removed: shop-230\npane: closed\nteardown: none\nworktree: removed\nbranch: \
deleted\nstacked[1]: shop-231\nhelp[2]: \"shop-231 stands on flow/shop-230: it keeps its \
commits and rebases onto origin/flow/shop-230 or where that merged\",Run `saaio` to see what is left"
        );
        assert!(!alone.text().contains("stack"), "{}", alone.text());
    }

    #[test]
    fn cleared_counts_and_names_the_removed_flows() {
        assert_eq!(
            cleared(&["export-csv".into(), "shop-198".into()], &[], &[]).text(),
            "cleared: 2\nnames[2]: export-csv,shop-198\nhelp[1]: Run `saaio` to see what is left"
        );
        assert_eq!(
            cleared(&[], &[], &[]).text(),
            "cleared: 0\nhelp[1]: Run `saaio` to see what is left"
        );
        assert_eq!(
            cleared(&["shop-198".into()], &[], &["shop-198".into()]).text(),
            format!(
                "cleared: 1\nnames[1]: shop-198\nuntrusted[1]: shop-198\nhelp[2]: Run `saaio` to see \
                 what is left,{UNTRUSTED_TEARDOWN}"
            )
        );
    }
}
