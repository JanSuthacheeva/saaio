use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::launch::{WORKTREES_DIR, worktree_add_argv, worktree_remove_argv};
use crate::view::Workspace;

/// The repo's own hooks: .flow/setup after the worktree exists, .flow/teardown before it goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hook {
    Setup,
    Teardown,
}

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

/// The branch checked out in `dir` or the repository above it, read from git's HEAD file so it is
/// cheap to ask often; None when HEAD is detached or there is no repository.
pub fn current_branch(dir: &Path) -> Option<String> {
    let mut here = Some(dir);
    while let Some(at) = here {
        let dot_git = at.join(".git");
        let head = if dot_git.is_dir() {
            dot_git.join("HEAD")
        } else if dot_git.is_file() {
            let link = fs::read_to_string(&dot_git).ok()?;
            at.join(link.trim().strip_prefix("gitdir:")?.trim())
                .join("HEAD")
        } else {
            here = at.parent();
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

/// Creates the worktree and branch; adds .flow/worktrees/ to .git/info/exclude once.
pub fn create_worktree(repo: &Path, workspace: &Workspace) -> Result<()> {
    run(&worktree_add_argv(repo, workspace))?;
    exclude_worktrees(repo)
}

/// Runs <repo>/.flow/<hook> in the worktree with env when it exists and is executable;
/// Ok(false) when there is none. A non-zero exit is an Err with the last stderr line.
pub fn run_hook(
    repo: &Path,
    hook: Hook,
    workspace: &Workspace,
    env: &[(String, String)],
) -> Result<bool> {
    let file = repo.join(".flow").join(match hook {
        Hook::Setup => "setup",
        Hook::Teardown => "teardown",
    });
    let is_executable = fs::metadata(&file)
        .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0);
    if !is_executable {
        return Ok(false);
    }
    let output = Command::new(&file)
        .current_dir(&workspace.path)
        .envs(env.iter().map(|(key, value)| (key, value)))
        .output()
        .with_context(|| format!("could not run {}", file.display()))?;
    if !output.status.success() {
        bail!("{}", last_line(&output.stderr, &output.stdout));
    }
    Ok(true)
}

/// Removes a worktree; without force Ok(false) when git refuses (uncommitted work).
pub fn remove_worktree(repo: &Path, workspace: &Workspace, force: bool) -> Result<bool> {
    let argv = worktree_remove_argv(repo, workspace, force);
    let output = Command::new(&argv[0])
        .args(&argv[1..])
        .output()
        .context("could not run git")?;
    Ok(output.status.success())
}

/// Whether the worktree has changes or untracked files git would refuse to remove it over.
pub fn has_uncommitted_work(workspace: &Workspace) -> Result<bool> {
    let status = run(&[
        "git".to_string(),
        "-C".to_string(),
        workspace.path.clone(),
        "status".to_string(),
        "--porcelain".to_string(),
    ])?;
    Ok(!status.trim().is_empty())
}

/// Runs a command; its stdout, or an error with the last line it printed.
/// How many commits of `branch` are on no other branch, local or remote; None when there is no such
/// branch.
pub fn commits_only_on(repo: &Path, branch: &str) -> Result<Option<u32>> {
    let git = |args: &[&str]| -> Vec<String> {
        ["git", "-C", &repo.display().to_string()]
            .iter()
            .chain(args)
            .map(|part| part.to_string())
            .collect()
    };
    let full = format!("refs/heads/{branch}");
    if run(&git(&["show-ref", "--verify", "--quiet", &full])).is_err() {
        return Ok(None);
    }
    // With --branches, --exclude names the branch without refs/heads/.
    let exclude = format!("--exclude={branch}");
    let count = run(&git(&[
        "rev-list",
        "--count",
        &full,
        "--not",
        &exclude,
        "--branches",
        "--remotes",
    ]))?;
    count
        .trim()
        .parse()
        .map(Some)
        .with_context(|| format!("git counted {count}"))
}

/// Whether the repo has a local branch of this name.
pub fn branch_exists(repo: &Path, branch: &str) -> bool {
    run(&[
        "git".to_string(),
        "-C".to_string(),
        repo.display().to_string(),
        "show-ref".to_string(),
        "--verify".to_string(),
        "--quiet".to_string(),
        format!("refs/heads/{branch}"),
    ])
    .is_ok()
}

/// git branch -D <branch>: the flow's branch once its worktree is gone.
pub fn delete_branch(repo: &Path, branch: &str) -> Result<()> {
    run(&[
        "git".to_string(),
        "-C".to_string(),
        repo.display().to_string(),
        "branch".to_string(),
        "-D".to_string(),
        branch.to_string(),
    ])
    .map(|_| ())
}

fn run(argv: &[String]) -> Result<String> {
    let (program, args) = argv.split_first().context("empty command")?;
    let output = Command::new(program)
        .args(args)
        .output()
        .with_context(|| format!("could not run {program}"))?;
    if !output.status.success() {
        bail!("{}", last_line(&output.stderr, &output.stdout));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn last_line(stderr: &[u8], stdout: &[u8]) -> String {
    let text =
        String::from_utf8_lossy(if stderr.is_empty() { stdout } else { stderr }).into_owned();
    text.lines()
        .map(str::trim)
        .rfind(|line| !line.is_empty())
        .unwrap_or("failed without a message")
        .to_string()
}

/// Adds .flow/worktrees/ to .git/info/exclude once, so worktrees never show in git status.
fn exclude_worktrees(repo: &Path) -> Result<()> {
    let git_dir = PathBuf::from(
        run(&[
            "git".to_string(),
            "-C".to_string(),
            repo.display().to_string(),
            "rev-parse".to_string(),
            "--git-common-dir".to_string(),
        ])?
        .trim(),
    );
    let git_dir = if git_dir.is_absolute() {
        git_dir
    } else {
        repo.join(git_dir)
    };
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
    use crate::launch::workspace_for;

    fn git(dir: &Path, args: &[&str]) {
        let status = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?}");
    }

    fn repo() -> (TempDir, PathBuf) {
        let dir = TempDir::new().unwrap();
        let repo = dir.path().canonicalize().unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
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
                "init",
            ],
        );
        (dir, repo)
    }

    fn hook(repo: &Path, name: &str, script: &str) {
        let dir = repo.join(".flow");
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join(name);
        fs::write(&file, script).unwrap();
        fs::set_permissions(&file, fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[test]
    fn finds_the_repository_root_from_a_subfolder() {
        let (_dir, repo) = repo();
        fs::create_dir_all(repo.join("src/deep")).unwrap();

        assert_eq!(repo_root(&repo.join("src/deep")).unwrap(), repo);
        assert!(repo_root(Path::new("/")).is_err());
    }

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
        assert_eq!(exclude.matches(".flow/worktrees/").count(), 1);
        assert!(create_worktree(&repo, &workspace).is_err());
    }

    #[test]
    fn runs_a_hook_in_the_worktree_with_the_flow_environment() {
        let (_dir, repo) = repo();
        let workspace = workspace_for(&repo, "shop-217", "flow/shop-217".into(), 3, "claude");
        create_worktree(&repo, &workspace).unwrap();
        hook(
            &repo,
            "setup",
            "#!/bin/sh\necho \"$FLOW_SLOT $(pwd)\" > ran.txt\n",
        );

        let ran = run_hook(
            &repo,
            Hook::Setup,
            &workspace,
            &[("FLOW_SLOT".into(), "3".into())],
        )
        .unwrap();

        assert!(ran);
        let written = fs::read_to_string(Path::new(&workspace.path).join("ran.txt")).unwrap();
        assert_eq!(written.trim(), format!("3 {}", workspace.path));
        assert!(!run_hook(&repo, Hook::Teardown, &workspace, &[]).unwrap());
    }

    #[test]
    fn a_failing_hook_reports_its_last_error_line() {
        let (_dir, repo) = repo();
        let workspace = workspace_for(&repo, "shop-217", "flow/shop-217".into(), 1, "claude");
        create_worktree(&repo, &workspace).unwrap();
        hook(
            &repo,
            "setup",
            "#!/bin/sh\necho starting >&2\necho 'port 8001 is taken' >&2\nexit 3\n",
        );

        let error = run_hook(&repo, Hook::Setup, &workspace, &[]).unwrap_err();

        assert_eq!(error.to_string(), "port 8001 is taken");
    }

    #[test]
    fn reads_the_branch_of_a_repo_a_subfolder_and_a_worktree() {
        let (_dir, repo) = repo();
        let sub = repo.join("src/deep");
        fs::create_dir_all(&sub).unwrap();
        let worker = workspace_for(&repo, "csv", "flow/csv".into(), 1, "claude");
        create_worktree(&repo, &worker).unwrap();

        assert_eq!(current_branch(&repo).as_deref(), Some("main"));
        assert_eq!(current_branch(&sub).as_deref(), Some("main"));
        assert_eq!(
            current_branch(Path::new(&worker.path)).as_deref(),
            Some("flow/csv")
        );
        assert_eq!(current_branch(Path::new("/")), None);
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
