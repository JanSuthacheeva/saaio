use std::path::{Path, PathBuf};

use crate::view::Workspace;

pub const WORKTREES_DIR: &str = ".flow/worktrees";

/// <repo>/.flow/worktrees/<name> on `branch`.
pub fn workspace_for(repo: &Path, name: &str, branch: String, slot: u8, agent: &str) -> Workspace {
    Workspace {
        path: repo.join(WORKTREES_DIR).join(name).display().to_string(),
        branch,
        slot,
        window: None,
        agent: agent.to_string(),
    }
}

/// git -C <repo> worktree add -b <branch> <path>
pub fn worktree_add_argv(repo: &Path, workspace: &Workspace) -> Vec<String> {
    let repo = repo.display().to_string();
    [
        "git",
        "-C",
        &repo,
        "worktree",
        "add",
        "-b",
        &workspace.branch,
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

/// The repository a flow worktree belongs to: the path above .flow/worktrees/<name>.
pub fn repo_of(space: &Workspace) -> Option<PathBuf> {
    Path::new(&space.path)
        .parent()
        .filter(|worktrees| worktrees.ends_with(WORKTREES_DIR))?
        .parent()?
        .parent()
        .map(Path::to_path_buf)
}

/// FLOW_ID, FLOW_NAME, FLOW_SLOT, FLOW_WORKTREE, FLOW_REPO for the hooks.
pub fn hook_env(id: &str, name: &str, repo: &Path, workspace: &Workspace) -> Vec<(String, String)> {
    [
        ("FLOW_ID", id.to_string()),
        ("FLOW_NAME", name.to_string()),
        ("FLOW_SLOT", workspace.slot.to_string()),
        ("FLOW_WORKTREE", workspace.path.clone()),
        ("FLOW_REPO", repo.display().to_string()),
    ]
    .into_iter()
    .map(|(key, value)| (key.to_string(), value))
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::launch::fixtures::{shop, strings};

    #[test]
    fn places_the_worktree_inside_the_repo_on_its_own_branch() {
        let workspace = shop();

        assert_eq!(workspace.path, "/repo/.flow/worktrees/shop-217");
        assert_eq!(workspace.branch, "flow/shop-217");
        assert_eq!(workspace.slot, 2);
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
                "/repo/.flow/worktrees/shop-217"
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
                "/repo/.flow/worktrees/shop-217"
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
                "/repo/.flow/worktrees/shop-217"
            ])
        );
    }

    #[test]
    fn hands_the_hooks_what_they_need() {
        assert_eq!(
            hook_env("7f3c", "shop-217", Path::new("/repo"), &shop()),
            vec![
                ("FLOW_ID".to_string(), "7f3c".to_string()),
                ("FLOW_NAME".to_string(), "shop-217".to_string()),
                ("FLOW_SLOT".to_string(), "2".to_string()),
                (
                    "FLOW_WORKTREE".to_string(),
                    "/repo/.flow/worktrees/shop-217".to_string()
                ),
                ("FLOW_REPO".to_string(), "/repo".to_string()),
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
}
