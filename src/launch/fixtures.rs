use std::path::Path;

use crate::launch::worktree::workspace_for;
use crate::view::Workspace;

pub(super) fn strings(items: &[&str]) -> Vec<String> {
    items.iter().copied().map(String::from).collect()
}

pub(super) fn shop() -> Workspace {
    workspace_for(
        Path::new("/repo"),
        "shop-217",
        "flow/shop-217".into(),
        2,
        "claude",
    )
}

/// A plain session in /Users/jan/scratch: no branch, no slot.
pub(super) fn scratch() -> Workspace {
    Workspace {
        path: "/Users/jan/scratch".into(),
        branch: None,
        slot: None,
        window: None,
        agent: "claude".into(),
    }
}
