//! Git worktrees: created with `.saaio/worktrees/` kept out of git status, and removed

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::launch::worktree::{WORKTREES_DIR, add_argv, remove_argv};
use crate::view::Workspace;
use git::{StartPoint, common_dir};
use process::run;

#[cfg(test)]
pub(crate) mod fixtures;
pub mod git;
pub mod hooks;
pub mod process;

/// Creates the worktree and branch, from `from` or what the repo has checked out; adds
/// .saaio/worktrees/ to .git/info/exclude once. Refuses a repo whose .saaio or .saaio/worktrees is
/// a link, which would place the worktree anywhere.
pub fn create_worktree(
    repo: &Path,
    workspace: &Workspace,
    from: Option<&StartPoint>,
) -> Result<()> {
    refuse_links(repo)?;
    let from = from.map(StartPoint::rev);
    run(&add_argv(repo, workspace, from.as_deref()))?;
    exclude_worktrees(repo)
}

/// Fails when <repo>/.saaio or <repo>/.saaio/worktrees is a symbolic link.
fn refuse_links(repo: &Path) -> Result<()> {
    for folder in [repo.join(".saaio"), repo.join(WORKTREES_DIR)] {
        if fs::symlink_metadata(&folder).is_ok_and(|meta| meta.file_type().is_symlink()) {
            bail!("{} is a symbolic link", folder.display());
        }
    }
    Ok(())
}

/// Removes a worktree; without force Ok(false) when git refuses (uncommitted work).
pub fn remove_worktree(repo: &Path, workspace: &Workspace, force: bool) -> Result<bool> {
    let argv = remove_argv(repo, workspace, force);
    let (program, args) = argv.split_first().context("empty command")?;
    let output = Command::new(program)
        .args(args)
        .output()
        .context("could not run git")?;
    Ok(output.status.success())
}

/// Adds .saaio/worktrees/ to .git/info/exclude once, so worktrees never show in git status.
#[allow(
    clippy::disallowed_methods,
    reason = "info/exclude is git's file, kept with git's modes"
)]
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
    use tempfile::TempDir;

    use super::*;
    use crate::launch::worktree::workspace_for;
    use crate::workspace::fixtures::{git, repo};

    fn git_out(dir: &Path, args: &[&str]) -> std::process::Output {
        Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .output()
            .unwrap()
    }

    #[test]
    fn creates_a_worktree_on_its_branch_and_keeps_it_out_of_git_status() {
        let (_dir, repo) = repo();
        let workspace = workspace_for(&repo, "shop-217", Some("flow/shop-217".into()), 1, "claude");

        create_worktree(&repo, &workspace, None).unwrap();
        create_worktree(
            &repo,
            &workspace_for(&repo, "shop-230", Some("flow/shop-230".into()), 2, "claude"),
            None,
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
        assert!(create_worktree(&repo, &workspace, None).is_err());
    }

    #[test]
    fn creates_a_detached_worktree_without_a_branch() {
        let (_dir, repo) = repo();
        let workspace = workspace_for(&repo, "research", None, 1, "claude");

        create_worktree(&repo, &workspace, None).unwrap();

        let head = |dir: &Path| {
            let out = git_out(dir, &["rev-parse", "HEAD"]);
            String::from_utf8(out.stdout).unwrap()
        };
        assert_eq!(head(Path::new(&workspace.path)), head(&repo));
        let branch = git_out(Path::new(&workspace.path), &["symbolic-ref", "-q", "HEAD"]);
        assert!(!branch.status.success(), "detached");
        let branches = git_out(&repo, &["branch", "--list"]);
        assert_eq!(
            String::from_utf8(branches.stdout).unwrap().lines().count(),
            1
        );
    }

    #[test]
    fn starts_a_worktree_from_a_remote_base_without_an_upstream() {
        let (_dir, repo) = repo();
        let scratch = TempDir::new().unwrap();
        let bare = scratch.path().join("origin.git");
        git(&repo, &["checkout", "-q", "-b", "stack"]);
        git(
            &repo,
            &[
                "-c",
                "user.email=t@t",
                "-c",
                "user.name=t",
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                "stack",
            ],
        );
        let stack = git_out(&repo, &["rev-parse", "stack"]).stdout;
        let bare_path = bare.display().to_string();
        git(&repo, &["clone", "-q", "--bare", ".", &bare_path]);
        git(&repo, &["checkout", "-q", "main"]);
        git(&repo, &["branch", "-D", "stack"]);
        git(&repo, &["remote", "add", "origin", &bare_path]);
        let from = git::start_point(&repo, "stack").unwrap();
        let workspace = Workspace {
            base: Some("stack".into()),
            ..workspace_for(&repo, "shop-217", Some("flow/shop-217".into()), 1, "claude")
        };

        create_worktree(&repo, &workspace, Some(&from)).unwrap();

        let head = git_out(Path::new(&workspace.path), &["rev-parse", "HEAD"]).stdout;
        assert_eq!(head, stack);
        let upstream = git_out(
            &repo,
            &["rev-parse", "--abbrev-ref", "flow/shop-217@{upstream}"],
        );
        assert!(
            !upstream.status.success(),
            "the flow's branch tracks its base"
        );
    }

    #[test]
    fn removes_a_clean_worktree_and_keeps_one_with_uncommitted_work() {
        let (_dir, repo) = repo();
        let clean = workspace_for(&repo, "clean", Some("flow/clean".into()), 1, "claude");
        let dirty = workspace_for(&repo, "dirty", Some("flow/dirty".into()), 2, "claude");
        create_worktree(&repo, &clean, None).unwrap();
        create_worktree(&repo, &dirty, None).unwrap();
        fs::write(Path::new(&dirty.path).join("work.txt"), "unsaved").unwrap();

        assert!(remove_worktree(&repo, &clean, false).unwrap());
        assert!(!Path::new(&clean.path).exists());
        assert!(!remove_worktree(&repo, &dirty, false).unwrap());
        assert!(remove_worktree(&repo, &dirty, true).unwrap());
    }

    #[test]
    fn refuses_a_saaio_folder_that_is_a_link() {
        let (_dir, repo) = repo();
        let elsewhere = TempDir::new().unwrap();
        std::os::unix::fs::symlink(elsewhere.path(), repo.join(".saaio")).unwrap();
        let workspace = workspace_for(&repo, "shop-217", Some("flow/shop-217".into()), 1, "claude");

        let error = create_worktree(&repo, &workspace, None).unwrap_err();

        assert_eq!(
            format!("{error:#}"),
            format!("{} is a symbolic link", repo.join(".saaio").display())
        );
        assert_eq!(fs::read_dir(elsewhere.path()).unwrap().count(), 0);
    }

    #[test]
    fn refuses_a_worktrees_folder_that_is_a_link() {
        let (_dir, repo) = repo();
        let elsewhere = TempDir::new().unwrap();
        fs::create_dir(repo.join(".saaio")).unwrap();
        std::os::unix::fs::symlink(elsewhere.path(), repo.join(WORKTREES_DIR)).unwrap();
        let workspace = workspace_for(&repo, "shop-217", Some("flow/shop-217".into()), 1, "claude");

        let error = create_worktree(&repo, &workspace, None).unwrap_err();

        assert_eq!(
            format!("{error:#}"),
            format!("{} is a symbolic link", repo.join(WORKTREES_DIR).display())
        );
        assert_eq!(fs::read_dir(elsewhere.path()).unwrap().count(), 0);
        assert!(!git::branch_exists(&repo, "flow/shop-217"));
    }
}
