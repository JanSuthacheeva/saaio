use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result};

use crate::launch::worktree::{WORKTREES_DIR, worktree_add_argv, worktree_remove_argv};
use crate::view::Workspace;
use git::common_dir;
use process::run;

#[cfg(test)]
mod fixtures;
pub mod git;
pub mod hooks;
pub mod process;

/// Creates the worktree and branch; adds .saaio/worktrees/ to .git/info/exclude once.
pub fn create_worktree(repo: &Path, workspace: &Workspace) -> Result<()> {
    run(&worktree_add_argv(repo, workspace))?;
    exclude_worktrees(repo)
}

/// Removes a worktree; without force Ok(false) when git refuses (uncommitted work).
pub fn remove_worktree(repo: &Path, workspace: &Workspace, force: bool) -> Result<bool> {
    let argv = worktree_remove_argv(repo, workspace, force);
    let (program, args) = argv.split_first().context("empty command")?;
    let output = Command::new(program)
        .args(args)
        .output()
        .context("could not run git")?;
    Ok(output.status.success())
}

/// Adds .saaio/worktrees/ to .git/info/exclude once, so worktrees never show in git status.
fn exclude_worktrees(repo: &Path) -> Result<()> {
    let git_dir =
        common_dir(repo).with_context(|| format!("not a git repository: {}", repo.display()))?;
    let exclude = git_dir.join("info/exclude");
    let line = format!("{WORKTREES_DIR}/");
    let current = fs::read_to_string(&exclude).unwrap_or_default();
    if current.lines().any(|existing| existing.trim() == line) {
        return Ok(());
    }
    fs::create_dir_all(exclude.parent().expect("exclude has a parent"))?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&exclude)
        .with_context(|| format!("could not write {}", exclude.display()))?;
    let separator = if current.is_empty() || current.ends_with('\n') {
        ""
    } else {
        "\n"
    };
    writeln!(file, "{separator}{line}")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::launch::worktree::workspace_for;
    use crate::workspace::fixtures::repo;

    #[test]
    fn creates_a_worktree_on_its_branch_and_keeps_it_out_of_git_status() {
        let (_dir, repo) = repo();
        let workspace = workspace_for(&repo, "shop-217", "flow/shop-217".into(), 1, "claude");

        create_worktree(&repo, &workspace).unwrap();
        create_worktree(
            &repo,
            &workspace_for(&repo, "shop-230", "flow/shop-230".into(), 2, "claude"),
        )
        .unwrap();

        assert!(Path::new(&workspace.path).join(".git").exists());
        let status = Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["status", "--porcelain"])
            .output()
            .unwrap();
        assert_eq!(String::from_utf8(status.stdout).unwrap(), "");
        let exclude = fs::read_to_string(repo.join(".git/info/exclude")).unwrap();
        assert_eq!(exclude.matches(".saaio/worktrees/").count(), 1);
        assert!(create_worktree(&repo, &workspace).is_err());
    }

    #[test]
    fn removes_a_clean_worktree_and_keeps_one_with_uncommitted_work() {
        let (_dir, repo) = repo();
        let clean = workspace_for(&repo, "clean", "flow/clean".into(), 1, "claude");
        let dirty = workspace_for(&repo, "dirty", "flow/dirty".into(), 2, "claude");
        create_worktree(&repo, &clean).unwrap();
        create_worktree(&repo, &dirty).unwrap();
        fs::write(Path::new(&dirty.path).join("work.txt"), "unsaved").unwrap();

        assert!(remove_worktree(&repo, &clean, false).unwrap());
        assert!(!Path::new(&clean.path).exists());
        assert!(!remove_worktree(&repo, &dirty, false).unwrap());
        assert!(remove_worktree(&repo, &dirty, true).unwrap());
    }
}
