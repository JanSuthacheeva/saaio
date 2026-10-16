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
