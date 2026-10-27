//! What git says of a repository and its worktrees, and ssh of a host; a branch deleted

use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use anyhow::{Context, Result, bail};

use super::process::{finish_within, stdout_of};
use crate::view::Workspace;

/// git rev-parse --show-toplevel in cwd.
pub fn repo_root(cwd: &Path) -> Result<PathBuf> {
    let output = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .context("could not run git")?;
    if !output.status.success() {
        bail!("not a git repository: {}", cwd.display());
    }
    Ok(PathBuf::from(
        String::from_utf8_lossy(&output.stdout).trim(),
    ))
}

/// The root of the repository `folder` is in, else the folder itself.
pub fn root_or_folder(folder: &Path) -> PathBuf {
    repo_root(folder).unwrap_or_else(|_| folder.to_path_buf())
}

/// The git dir shared by every worktree of the repository `dir` is in (git rev-parse
/// --git-common-dir), absolute and canonical; None outside a repository or without git.
pub fn common_dir(dir: &Path) -> Option<PathBuf> {
    let found = PathBuf::from(git(dir, &["rev-parse", "--git-common-dir"]).ok()?.trim());
    dir.join(found).canonicalize().ok()
}

/// The branch checked out in `dir` or the repository above it, read from git's HEAD file so it is
/// cheap to ask often; None when HEAD is detached or there is no repository.
pub fn current_branch(dir: &Path) -> Option<String> {
    for at in dir.ancestors() {
        let dot_git = at.join(".git");
        let head = if dot_git.is_dir() {
            dot_git.join("HEAD")
        } else if dot_git.is_file() {
            let link = fs::read_to_string(&dot_git).ok()?;
            at.join(link.trim().strip_prefix("gitdir:")?.trim())
                .join("HEAD")
        } else {
            continue;
        };
        let text = fs::read_to_string(head).ok()?;
        return text
            .trim()
            .strip_prefix("ref: refs/heads/")
            .map(String::from);
    }
    None
}

/// Whether the worktree has changes or untracked files git would refuse to remove it over.
pub fn has_uncommitted_work(repo: &Path, workspace: &Workspace) -> Result<bool> {
    let path = Path::new(&workspace.path);
    let status = worktree_git(repo, path, &["status", "--porcelain"])?;
    Ok(!status.trim().is_empty())
}

/// How many commits of `branch` are on no other branch, local or remote; None when there is no such
/// branch.
pub fn commits_only_on(repo: &Path, branch: &str) -> Result<Option<u32>> {
    let full = format!("refs/heads/{branch}");
    if git(repo, &["show-ref", "--verify", "--quiet", &full]).is_err() {
        return Ok(None);
    }
    // With --branches, --exclude names the branch without refs/heads/.
    let exclude = format!("--exclude={branch}");
    let count = git(
        repo,
        &[
            "rev-list",
            "--count",
            &full,
            "--not",
            &exclude,
            "--branches",
            "--remotes",
        ],
    )?;
    count
        .trim()
        .parse()
        .map(Some)
        .with_context(|| format!("git counted {count}"))
}

/// Whether the repo has a local branch of this name.
pub fn branch_exists(repo: &Path, branch: &str) -> bool {
    git(
        repo,
        &[
            "show-ref",
            "--verify",
            "--quiet",
            &format!("refs/heads/{branch}"),
        ],
    )
    .is_ok()
}

/// The URLs of every remote of the repository `dir` is in (a fork's upstream too); none without one.
pub fn remote_urls(dir: &Path) -> Vec<String> {
    let Ok(text) = git(dir, &["remote", "-v"]) else {
        return Vec::new();
    };
    let mut urls: Vec<String> = text
        .lines()
        .filter_map(|line| line.split_whitespace().nth(1))
        .map(String::from)
        .collect();
    urls.dedup();
    urls
}

/// ssh -G -- <host>: the hostname its ssh config gives it (the host itself when no rule names it);
/// None when ssh does not run or prints none.
pub fn ssh_hostname(host: &str) -> Option<String> {
    let mut command = Command::new("ssh");
    command.args(["-G", "--", host]);
    let config = stdout_of(command, "ssh").ok()?;
    config
        .lines()
        .find_map(|line| line.strip_prefix("hostname "))
        .map(|name| name.trim().to_string())
}

/// Where git creates a flow's branch: a local branch, or a remote's copy of one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartPoint {
    Local(String),
    Remote { remote: String, branch: String },
}

impl StartPoint {
    /// What git worktree add starts from: `x` or `refs/remotes/origin/x`.
    pub fn rev(&self) -> String {
        match self {
            Self::Local(branch) => branch.clone(),
            Self::Remote { remote, branch } => format!("refs/remotes/{remote}/{branch}"),
        }
    }
}

/// How long one remote may take to fetch a base.
const FETCH_LIMIT: Duration = Duration::from_secs(30);

/// The start point for `base`: the local branch, else the first remote (origin first) that has
/// it, fetched with a deadline. Err for a name git refuses as a branch, or one no remote has.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "saaio start --base uses it next")
)]
pub fn start_point(repo: &Path, base: &str) -> Result<StartPoint> {
    if base.starts_with('-') || git(repo, &["check-ref-format", "--branch", base]).is_err() {
        bail!("{base} is no branch name");
    }
    if branch_exists(repo, base) {
        return Ok(StartPoint::Local(base.to_string()));
    }
    let listed = git(repo, &["remote"]).unwrap_or_default();
    let mut remotes: Vec<&str> = listed.lines().map(str::trim).collect();
    remotes.sort_by_key(|remote| *remote != "origin");
    for remote in remotes {
        let refspec = format!("refs/heads/{base}:refs/remotes/{remote}/{base}");
        let mut command = Command::new("git");
        command
            .arg("-C")
            .arg(repo)
            .args(["fetch", "--quiet", "--no-tags", "--", remote, &refspec])
            .env("GIT_TERMINAL_PROMPT", "0");
        if finish_within(command, "git", FETCH_LIMIT).is_ok() {
            return Ok(StartPoint::Remote {
                remote: remote.to_string(),
                branch: base.to_string(),
            });
        }
    }
    bail!("no branch {base} here or on a remote")
}

/// git branch -D <branch>: the flow's branch once its worktree is gone.
pub fn delete_branch(repo: &Path, branch: &str) -> Result<()> {
    git(repo, &["branch", "-D", branch]).map(|_| ())
}

/// git -C <dir> <args>; its stdout, or an error with the last line it printed.
fn git(dir: impl AsRef<OsStr>, args: &[&str]) -> Result<String> {
    let mut command = Command::new("git");
    command.arg("-C").arg(dir).args(args);
    stdout_of(command, "git")
}

/// git on a worker's worktree through the git dir the repository registered for it, with no
/// fsmonitor and no hooks: nothing the worker wrote into its worktree configures it.
fn worktree_git(repo: &Path, worktree: &Path, args: &[&str]) -> Result<String> {
    let git_dir = registered_git_dir(repo, worktree)?;
    let mut command = Command::new("git");
    command
        .arg("--git-dir")
        .arg(git_dir)
        .arg("--work-tree")
        .arg(worktree)
        .args([
            "-c",
            "core.fsmonitor=false",
            "-c",
            "core.hooksPath=/dev/null",
        ])
        .args(args)
        .current_dir(worktree);
    stdout_of(command, "git")
}

/// The git dir the repository registered for `worktree`: the <common>/worktrees/<n> whose gitdir
/// file names a .git in that folder (relative to <n> under worktree.useRelativePaths); Err when
/// none does, the folder is gone or the repository has no worktrees.
pub fn registered_git_dir(repo: &Path, worktree: &Path) -> Result<PathBuf> {
    let common = common_dir(repo).context("not a git repository")?;
    let (Ok(wanted), Ok(entries)) = (
        worktree.canonicalize(),
        fs::read_dir(common.join("worktrees")),
    ) else {
        bail!(no_worktree(repo, worktree));
    };
    for entry in entries {
        let dir = entry?.path();
        let Ok(gitdir) = fs::read_to_string(dir.join("gitdir")) else {
            continue;
        };
        let folder = dir.join(gitdir.trim()).parent().map(Path::canonicalize);
        if matches!(folder, Some(Ok(folder)) if folder == wanted) {
            return Ok(dir);
        }
    }
    bail!(no_worktree(repo, worktree))
}

fn no_worktree(repo: &Path, worktree: &Path) -> String {
    format!(
        "{} is no worktree of {}",
        worktree.display(),
        repo.display()
    )
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;
    use crate::fixtures::executable;
    use crate::launch::worktree::workspace_for;
    use crate::workspace::create_worktree;
    use crate::workspace::fixtures::{git, repo};

    /// A bare clone of `repo` with a branch `stack` the repo itself lacks, added as `remote`.
    fn remote_with_stack(dir: &Path, repo: &Path, remote: &str) {
        let bare = dir.join(format!("{remote}.git"));
        git(repo, &["branch", "stack"]);
        git(
            dir,
            &[
                "clone",
                "-q",
                "--bare",
                &repo.display().to_string(),
                &bare.display().to_string(),
            ],
        );
        git(repo, &["branch", "-D", "stack"]);
        git(
            repo,
            &["remote", "add", remote, &bare.display().to_string()],
        );
    }

    #[test]
    fn a_local_base_starts_from_the_branch_itself() {
        let (_dir, repo) = repo();
        git(&repo, &["branch", "flow/csv"]);

        let point = start_point(&repo, "flow/csv").unwrap();

        assert_eq!(point, StartPoint::Local("flow/csv".into()));
        assert_eq!(point.rev(), "flow/csv");
    }

    #[test]
    fn a_remote_only_base_is_fetched_from_origin_first() {
        let (_dir, repo) = repo();
        let scratch = TempDir::new().unwrap();
        remote_with_stack(scratch.path(), &repo, "aaa");
        remote_with_stack(scratch.path(), &repo, "origin");

        let point = start_point(&repo, "stack").unwrap();

        assert_eq!(
            point,
            StartPoint::Remote {
                remote: "origin".into(),
                branch: "stack".into()
            }
        );
        assert_eq!(point.rev(), "refs/remotes/origin/stack");
        git(&repo, &["show-ref", "--verify", "--quiet", &point.rev()]);
        assert!(!branch_exists(&repo, "stack"));
    }

    #[test]
    fn a_base_on_another_remote_is_fetched_from_it() {
        let (_dir, repo) = repo();
        let scratch = TempDir::new().unwrap();
        remote_with_stack(scratch.path(), &repo, "upstream");
        let empty = scratch.path().join("empty.git");
        git(
            scratch.path(),
            &["init", "-q", "--bare", &empty.display().to_string()],
        );
        git(
            &repo,
            &["remote", "add", "origin", &empty.display().to_string()],
        );

        assert_eq!(
            start_point(&repo, "stack").unwrap().rev(),
            "refs/remotes/upstream/stack"
        );
    }

    #[test]
    fn refuses_an_invalid_name_and_a_branch_nowhere() {
        let (_dir, repo) = repo();
        let scratch = TempDir::new().unwrap();
        remote_with_stack(scratch.path(), &repo, "origin");

        for bad in ["a..b", "-x", "HEAD", "with space"] {
            let error = start_point(&repo, bad).unwrap_err();
            assert_eq!(error.to_string(), format!("{bad} is no branch name"));
        }
        let error = start_point(&repo, "flow/nowhere").unwrap_err();
        assert_eq!(
            error.to_string(),
            "no branch flow/nowhere here or on a remote"
        );
    }

    #[test]
    fn an_ssh_host_no_rule_names_resolves_to_itself() {
        if Command::new("ssh").arg("-V").output().is_err() {
            eprintln!("skipped: no ssh");
            return;
        }

        assert_eq!(
            ssh_hostname("saaio-test-unknown-alias").as_deref(),
            Some("saaio-test-unknown-alias")
        );
    }

    #[test]
    fn finds_the_repository_root_from_a_subfolder() {
        let (_dir, repo) = repo();
        fs::create_dir_all(repo.join("src/deep")).unwrap();

        assert_eq!(repo_root(&repo.join("src/deep")).unwrap(), repo);
        assert!(repo_root(Path::new("/")).is_err());
    }

    #[test]
    fn finds_the_main_git_dir_from_a_subfolder_and_a_worktree() {
        let (_dir, repo) = repo();
        fs::create_dir_all(repo.join("src/deep")).unwrap();
        let workspace = workspace_for(&repo, "shop-217", "flow/shop-217".into(), 1, "claude");
        create_worktree(&repo, &workspace, None).unwrap();
        let outside = TempDir::new().unwrap();
        let git_dir = repo.join(".git");

        assert_eq!(common_dir(&repo), Some(git_dir.clone()));
        assert_eq!(common_dir(&repo.join("src/deep")), Some(git_dir.clone()));
        assert_eq!(common_dir(Path::new(&workspace.path)), Some(git_dir));
        assert_eq!(common_dir(outside.path()), None);
    }

    #[test]
    fn reads_the_branch_of_a_repo_a_subfolder_and_a_worktree() {
        let (_dir, repo) = repo();
        let sub = repo.join("src/deep");
        fs::create_dir_all(&sub).unwrap();
        let worker = workspace_for(&repo, "csv", "flow/csv".into(), 1, "claude");
        create_worktree(&repo, &worker, None).unwrap();

        assert_eq!(current_branch(&repo).as_deref(), Some("main"));
        assert_eq!(current_branch(&sub).as_deref(), Some("main"));
        assert_eq!(
            current_branch(Path::new(&worker.path)).as_deref(),
            Some("flow/csv")
        );
        assert_eq!(current_branch(Path::new("/")), None);
    }

    #[test]
    fn reads_uncommitted_work_through_the_git_dir_the_repository_registered() {
        let (dir, repo) = repo();
        let space = workspace_for(&repo, "shop-217", "flow/shop-217".into(), 1, "claude");
        create_worktree(&repo, &space, None).unwrap();
        let path = Path::new(&space.path);
        let marker = dir.path().join("marker");
        let monitor = dir.path().join("monitor");
        executable(
            &monitor,
            &format!("#!/bin/sh\ntouch '{}'\n", marker.display()),
        );
        let planted = dir.path().join("planted");
        git(dir.path(), &["init", "-q", "planted"]);
        git(
            &planted,
            &["config", "core.fsmonitor", &monitor.display().to_string()],
        );
        let gitdir = format!("gitdir: {}\n", planted.join(".git").display());

        assert!(!has_uncommitted_work(&repo, &space).unwrap());
        fs::write(path.join("work.txt"), "unsaved").unwrap();
        assert!(has_uncommitted_work(&repo, &space).unwrap());
        fs::write(path.join(".git"), gitdir).unwrap();
        assert!(has_uncommitted_work(&repo, &space).unwrap());
        assert!(!marker.exists(), "the worktree's own .git configured git");
        let elsewhere = workspace_for(&repo, "other", "flow/other".into(), 2, "claude");
        fs::create_dir_all(&elsewhere.path).unwrap();
        assert!(
            has_uncommitted_work(&repo, &elsewhere).is_err(),
            "no registered worktree"
        );
    }

    #[test]
    fn reads_uncommitted_work_in_a_repository_that_records_relative_paths() {
        let (_dir, repo) = repo();
        git(&repo, &["config", "worktree.useRelativePaths", "true"]);
        let space = workspace_for(&repo, "shop-217", "flow/shop-217".into(), 1, "claude");
        create_worktree(&repo, &space, None).unwrap();

        assert!(!has_uncommitted_work(&repo, &space).unwrap());
        fs::write(Path::new(&space.path).join("work.txt"), "unsaved").unwrap();
        assert!(has_uncommitted_work(&repo, &space).unwrap());
    }
}
