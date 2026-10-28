//! A flow's worktree path, its git command lines and the hooks' environment

use std::path::{Path, PathBuf};

use crate::view::Workspace;

pub const WORKTREES_DIR: &str = ".saaio/worktrees";

/// <repo>/.saaio/worktrees/<name> on `branch`.
pub fn workspace_for(
    repo: &Path,
    name: &str,
    branch: Option<String>,
    slot: u8,
    agent: &str,
) -> Workspace {
    Workspace {
        path: repo.join(WORKTREES_DIR).join(name).display().to_string(),
        branch,
        slot: Some(slot),
        window: None,
        agent: agent.to_string(),
        base: None,
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
        base: None,
    }
}

/// git -C <repo> worktree add [--no-track] -b <branch> <path> [<from>]: --no-track with `from`,
/// so the flow's branch never pulls from or pushes to its base; without a branch,
/// git -C <repo> worktree add --detach <path> [<from>].
pub fn add_argv(repo: &Path, workspace: &Workspace, from: Option<&str>) -> Vec<String> {
    let repo = repo.display().to_string();
    let mut argv: Vec<String> = ["git", "-C", &repo, "worktree", "add"]
        .map(String::from)
        .to_vec();
    match &workspace.branch {
        Some(branch) => {
            if from.is_some() {
                argv.push("--no-track".to_string());
            }
            argv.extend(["-b", branch].map(String::from));
        }
        None => argv.push("--detach".to_string()),
    }
    argv.push(workspace.path.clone());
    argv.extend(from.map(String::from));
    argv
}

/// git -C <repo> worktree remove [--force] <path>; without force a dirty worktree is kept.
pub fn remove_argv(repo: &Path, workspace: &Workspace, force: bool) -> Vec<String> {
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

/// The repository a flow worktree belongs to: the path above .saaio/worktrees/<name>, with a
/// branch or detached with a slot. None for a plain session (neither), even one opened inside a
/// worktree, so remove never tears down a folder it did not create.
pub fn repo_of(space: &Workspace) -> Option<PathBuf> {
    if space.branch.is_none() && space.slot.is_none() {
        return None;
    }
    Path::new(&space.path)
        .parent()
        .filter(|worktrees| worktrees.ends_with(WORKTREES_DIR))?
        .parent()?
        .parent()
        .map(Path::to_path_buf)
}

/// What hook_env sets: a worker's identity, which a session saaio ui starts from inside one must
/// not pass on.
pub const HOOK_VARS: [&str; 6] = [
    "SAAIO_FLOW_ID",
    "SAAIO_FLOW_NAME",
    "SAAIO_SLOT",
    "SAAIO_WORKTREE",
    "SAAIO_REPO",
    "SAAIO_BASE",
];

/// SAAIO_FLOW_ID, SAAIO_FLOW_NAME, SAAIO_SLOT (when it has one), SAAIO_WORKTREE, SAAIO_REPO and
/// SAAIO_BASE (when it has one) for the hooks.
pub fn hook_env(id: &str, name: &str, repo: &Path, workspace: &Workspace) -> Vec<(String, String)> {
    [
        Some(("SAAIO_FLOW_ID", id.to_string())),
        Some(("SAAIO_FLOW_NAME", name.to_string())),
        workspace.slot.map(|slot| ("SAAIO_SLOT", slot.to_string())),
        Some(("SAAIO_WORKTREE", workspace.path.clone())),
        Some(("SAAIO_REPO", repo.display().to_string())),
        workspace.base.clone().map(|base| ("SAAIO_BASE", base)),
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
            add_argv(Path::new("/repo"), &workspace, None),
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
            remove_argv(Path::new("/repo"), &workspace, false),
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
            remove_argv(Path::new("/repo"), &workspace, true),
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
    fn starts_a_worktree_from_its_base_without_tracking_it() {
        assert_eq!(
            add_argv(
                Path::new("/repo"),
                &shop(),
                Some("refs/remotes/origin/stack")
            ),
            strings(&[
                "git",
                "-C",
                "/repo",
                "worktree",
                "add",
                "--no-track",
                "-b",
                "flow/shop-217",
                "/repo/.saaio/worktrees/shop-217",
                "refs/remotes/origin/stack"
            ])
        );
    }

    #[test]
    fn detaches_a_worktree_without_a_branch() {
        let research = Workspace {
            branch: None,
            ..shop()
        };

        assert_eq!(
            add_argv(Path::new("/repo"), &research, Some("main")),
            strings(&[
                "git",
                "-C",
                "/repo",
                "worktree",
                "add",
                "--detach",
                "/repo/.saaio/worktrees/shop-217",
                "main"
            ])
        );
        assert_eq!(
            add_argv(Path::new("/repo"), &research, None)[5..],
            ["--detach", "/repo/.saaio/worktrees/shop-217"]
        );
    }

    #[test]
    fn names_the_base_to_the_hooks_of_a_stacked_flow() {
        let stacked = Workspace {
            base: Some("flow/shop-216".into()),
            ..shop()
        };

        assert_eq!(
            hook_env("7f3c", "shop-217", Path::new("/repo"), &stacked).last(),
            Some(&("SAAIO_BASE".to_string(), "flow/shop-216".to_string()))
        );
        assert!(
            hook_env("7f3c", "shop-217", Path::new("/repo"), &shop())
                .iter()
                .all(|(key, _)| key != "SAAIO_BASE")
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
        let detached = Workspace {
            branch: None,
            ..shop()
        };
        assert_eq!(repo_of(&detached), Some(PathBuf::from("/repo")));
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
