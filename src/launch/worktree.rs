use std::path::{Path, PathBuf};

use crate::view::Workspace;

pub const WORKTREES_DIR: &str = ".saaio/worktrees";

/// <repo>/.saaio/worktrees/<name> on `branch`.
pub fn workspace_for(repo: &Path, name: &str, branch: String, slot: u8, agent: &str) -> Workspace {
    Workspace {
        path: repo.join(WORKTREES_DIR).join(name).display().to_string(),
        branch: Some(branch),
        slot: Some(slot),
        window: None,
        agent: agent.to_string(),
    }
}

/// A plain session's workspace: `folder`, no branch, no slot.
pub fn folder_workspace(folder: &Path, agent: &str) -> Workspace {
    Workspace {
        path: folder.display().to_string(),
        branch: None,
        slot: None,
        window: None,
        agent: agent.to_string(),
    }
}

/// git -C <repo> worktree add -b <branch> <path>; workspace_for always sets the branch.
pub fn worktree_add_argv(repo: &Path, workspace: &Workspace) -> Vec<String> {
    let repo = repo.display().to_string();
    let branch = workspace
        .branch
        .as_deref()
        .expect("a worktree flow has a branch");
    [
        "git",
        "-C",
        &repo,
        "worktree",
        "add",
        "-b",
        branch,
        &workspace.path,
    ]
    .map(String::from)
    .to_vec()
}

/// git -C <repo> worktree remove [--force] <path>; without force a dirty worktree is kept.
pub fn worktree_remove_argv(repo: &Path, workspace: &Workspace, force: bool) -> Vec<String> {
    let repo = repo.display().to_string();
    let mut argv: Vec<String> = ["git", "-C", &repo, "worktree", "remove"]
        .map(String::from)
        .to_vec();
    if force {
        argv.push("--force".to_string());
    }
    argv.push(workspace.path.clone());
    argv
}

/// The repository a flow worktree belongs to: the path above .saaio/worktrees/<name>. None for a
/// plain session (no branch), even one opened inside a worktree, so remove never tears down a folder
/// it did not create.
pub fn repo_of(space: &Workspace) -> Option<PathBuf> {
    space.branch.as_ref()?;
    Path::new(&space.path)
        .parent()
        .filter(|worktrees| worktrees.ends_with(WORKTREES_DIR))?
        .parent()?
        .parent()
        .map(Path::to_path_buf)
}

/// What hook_env sets: a worker's identity, which a session saaio ui starts from inside one must
/// not pass on.
pub const HOOK_VARS: [&str; 5] = [
    "SAAIO_FLOW_ID",
    "SAAIO_FLOW_NAME",
    "SAAIO_SLOT",
    "SAAIO_WORKTREE",
    "SAAIO_REPO",
];

/// SAAIO_FLOW_ID, SAAIO_FLOW_NAME, SAAIO_SLOT (when it has one), SAAIO_WORKTREE, SAAIO_REPO for the hooks.
pub fn hook_env(id: &str, name: &str, repo: &Path, workspace: &Workspace) -> Vec<(String, String)> {
    [
        Some(("SAAIO_FLOW_ID", id.to_string())),
        Some(("SAAIO_FLOW_NAME", name.to_string())),
        workspace.slot.map(|slot| ("SAAIO_SLOT", slot.to_string())),
        Some(("SAAIO_WORKTREE", workspace.path.clone())),
        Some(("SAAIO_REPO", repo.display().to_string())),
    ]
    .into_iter()
    .flatten()
    .map(|(key, value)| (key.to_string(), value))
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::launch::fixtures::{scratch, shop, strings};

    #[test]
    fn places_the_worktree_inside_the_repo_on_its_own_branch() {
        let workspace = shop();

        assert_eq!(workspace.path, "/repo/.saaio/worktrees/shop-217");
        assert_eq!(workspace.branch.as_deref(), Some("flow/shop-217"));
        assert_eq!(workspace.slot, Some(2));
        assert_eq!(workspace.window, None);
        assert_eq!(workspace.agent, "claude");
        assert_eq!(
            worktree_add_argv(Path::new("/repo"), &workspace),
            strings(&[
                "git",
                "-C",
                "/repo",
                "worktree",
                "add",
                "-b",
                "flow/shop-217",
                "/repo/.saaio/worktrees/shop-217"
            ])
        );
        assert_eq!(
            worktree_remove_argv(Path::new("/repo"), &workspace, false),
            strings(&[
                "git",
                "-C",
                "/repo",
                "worktree",
                "remove",
                "/repo/.saaio/worktrees/shop-217"
            ])
        );
        assert_eq!(
            worktree_remove_argv(Path::new("/repo"), &workspace, true),
            strings(&[
                "git",
                "-C",
                "/repo",
                "worktree",
                "remove",
                "--force",
                "/repo/.saaio/worktrees/shop-217"
            ])
        );
    }

    #[test]
    fn hands_the_hooks_what_they_need() {
        assert_eq!(
            hook_env("7f3c", "shop-217", Path::new("/repo"), &shop()),
            vec![
                ("SAAIO_FLOW_ID".to_string(), "7f3c".to_string()),
                ("SAAIO_FLOW_NAME".to_string(), "shop-217".to_string()),
                ("SAAIO_SLOT".to_string(), "2".to_string()),
                (
                    "SAAIO_WORKTREE".to_string(),
                    "/repo/.saaio/worktrees/shop-217".to_string()
                ),
                ("SAAIO_REPO".to_string(), "/repo".to_string()),
            ]
        );
    }

    #[test]
    fn finds_the_repo_above_the_worktrees_folder_only() {
        assert_eq!(repo_of(&shop()), Some(PathBuf::from("/repo")));
        let elsewhere = Workspace {
            path: "/somewhere/shop-217".into(),
            ..shop()
        };
        assert_eq!(repo_of(&elsewhere), None);
    }

    #[test]
    fn a_plain_session_has_no_repo_even_inside_the_worktrees_folder() {
        let inside = Workspace {
            path: "/repo/.saaio/worktrees/shop-217".into(),
            ..scratch()
        };

        assert_eq!(repo_of(&scratch()), None);
        assert_eq!(
            repo_of(&inside),
            None,
            "remove must not tear down a folder it did not create"
        );
    }

    #[test]
    fn sets_no_slot_for_the_hooks_without_one() {
        let keys: Vec<String> = hook_env("7f3c", "x", Path::new("/repo"), &scratch())
            .into_iter()
            .map(|(key, _)| key)
            .collect();

        assert_eq!(
            keys,
            [
                "SAAIO_FLOW_ID",
                "SAAIO_FLOW_NAME",
                "SAAIO_WORKTREE",
                "SAAIO_REPO"
            ]
        );
    }
}
