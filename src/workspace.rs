use std::ffi::OsStr;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};

use crate::launch::worktree::{WORKTREES_DIR, worktree_add_argv, worktree_remove_argv};
use crate::text::short_duration;
use crate::view::Workspace;

/// The repo's own hooks: .saaio/setup after the worktree exists, .saaio/teardown before it goes,
/// .saaio/pause and .saaio/resume around a flow's rest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hook {
    Setup,
    Teardown,
    #[cfg_attr(not(test), expect(dead_code, reason = "PauseWatch runs them"))]
    Pause,
    #[cfg_attr(not(test), expect(dead_code, reason = "PauseWatch runs them"))]
    Resume,
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

/// The git dir shared by every worktree of the repository `dir` is in (git rev-parse
/// --git-common-dir), absolute and canonical; None outside a repository or without git.
pub fn git_common_dir(dir: &Path) -> Option<PathBuf> {
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

/// Creates the worktree and branch; adds .saaio/worktrees/ to .git/info/exclude once.
pub fn create_worktree(repo: &Path, workspace: &Workspace) -> Result<()> {
    run(&worktree_add_argv(repo, workspace))?;
    exclude_worktrees(repo)
}

/// Runs <repo>/.saaio/<hook> in the worktree with env when it exists and is executable;
/// Ok(false) when there is none. A non-zero exit is an Err with the last stderr line.
pub fn run_hook(
    repo: &Path,
    hook: Hook,
    workspace: &Workspace,
    env: &[(String, String)],
) -> Result<bool> {
    let file = hook_file(repo, hook);
    if !is_executable(&file) {
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

/// run_hook with no stdin, killed and failed past `limit` ("no answer within 5m").
#[cfg_attr(not(test), expect(dead_code, reason = "PauseWatch runs them"))]
pub fn run_hook_within(
    repo: &Path,
    hook: Hook,
    workspace: &Workspace,
    env: &[(String, String)],
    limit: Duration,
) -> Result<bool> {
    let file = hook_file(repo, hook);
    if !is_executable(&file) {
        return Ok(false);
    }
    let mut command = Command::new(&file);
    command
        .current_dir(&workspace.path)
        .envs(env.iter().map(|(key, value)| (key, value)));
    finish_within(command, &file.display().to_string(), limit)?;
    Ok(true)
}

/// Whether <repo>/.saaio/<hook> exists and is executable.
#[cfg_attr(not(test), expect(dead_code, reason = "PauseWatch runs them"))]
pub fn has_hook(repo: &Path, hook: Hook) -> bool {
    is_executable(&hook_file(repo, hook))
}

fn hook_file(repo: &Path, hook: Hook) -> PathBuf {
    repo.join(".saaio").join(match hook {
        Hook::Setup => "setup",
        Hook::Teardown => "teardown",
        Hook::Pause => "pause",
        Hook::Resume => "resume",
    })
}

fn is_executable(file: &Path) -> bool {
    fs::metadata(file).is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
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
/// none does.
fn registered_git_dir(repo: &Path, worktree: &Path) -> Result<PathBuf> {
    let common = git_common_dir(repo).context("not a git repository")?;
    let wanted = worktree.canonicalize()?;
    for entry in fs::read_dir(common.join("worktrees"))? {
        let dir = entry?.path();
        let Ok(gitdir) = fs::read_to_string(dir.join("gitdir")) else {
            continue;
        };
        let folder = dir.join(gitdir.trim()).parent().map(Path::canonicalize);
        if matches!(folder, Some(Ok(folder)) if folder == wanted) {
            return Ok(dir);
        }
    }
    bail!(
        "{} is no worktree of {}",
        worktree.display(),
        repo.display()
    )
}

/// Runs an argv in `dir` with GH_PROMPT_DISABLED=1 and no stdin; its stdout, or an error with
/// the last line it printed. Past `limit` it is killed and fails.
pub fn output_in(dir: &Path, argv: &[String], limit: Duration) -> Result<String> {
    let (program, args) = argv.split_first().context("empty command")?;
    let mut command = Command::new(program);
    command
        .args(args)
        .current_dir(dir)
        .env("GH_PROMPT_DISABLED", "1");
    let stdout = finish_within(command, program, limit)?;
    Ok(String::from_utf8_lossy(&stdout).into_owned())
}

/// Runs `command` with no stdin; its stdout, or an error with the last line it printed. Past
/// `limit` it is killed and fails.
fn finish_within(mut command: Command, program: &str, limit: Duration) -> Result<Vec<u8>> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("could not run {program}"))?;
    // Read in the background, so a full pipe never stops the command before it ends.
    let stdout = read_in_background(child.stdout.take());
    let stderr = read_in_background(child.stderr.take());
    let deadline = Instant::now() + limit;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            let millis = u64::try_from(limit.as_millis()).unwrap_or(u64::MAX);
            bail!("no answer within {}", short_duration(millis));
        }
        thread::sleep(WAIT_STEP);
    };
    let stdout = stdout.join().unwrap_or_default();
    let stderr = stderr.join().unwrap_or_default();
    if !status.success() {
        bail!("{}", last_line(&stderr, &stdout));
    }
    Ok(stdout)
}

/// How often finish_within looks whether its command ended.
const WAIT_STEP: Duration = Duration::from_millis(20);

/// All a pipe gives until it closes, read on a thread of its own.
fn read_in_background(pipe: Option<impl Read + Send + 'static>) -> JoinHandle<Vec<u8>> {
    thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(mut pipe) = pipe {
            let _ = pipe.read_to_end(&mut bytes);
        }
        bytes
    })
}

/// Runs an argv; its stdout, or an error with the last line it printed.
fn run(argv: &[String]) -> Result<String> {
    let (program, args) = argv.split_first().context("empty command")?;
    let mut command = Command::new(program);
    command.args(args);
    stdout_of(command, program)
}

fn stdout_of(mut command: Command, program: &str) -> Result<String> {
    let output = command
        .output()
        .with_context(|| format!("could not run {program}"))?;
    if !output.status.success() {
        bail!("{}", last_line(&output.stderr, &output.stdout));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn last_line(stderr: &[u8], stdout: &[u8]) -> String {
    let text = String::from_utf8_lossy(if stderr.is_empty() { stdout } else { stderr });
    text.lines()
        .map(str::trim)
        .rfind(|line| !line.is_empty())
        .unwrap_or("failed without a message")
        .to_string()
}

/// Adds .saaio/worktrees/ to .git/info/exclude once, so worktrees never show in git status.
fn exclude_worktrees(repo: &Path) -> Result<()> {
    let git_dir = git_common_dir(repo)
        .with_context(|| format!("not a git repository: {}", repo.display()))?;
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
        let dir = repo.join(".saaio");
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join(name);
        fs::write(&file, script).unwrap();
        fs::set_permissions(&file, fs::Permissions::from_mode(0o755)).unwrap();
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
    fn runs_a_command_in_a_folder_without_prompts() {
        let dir = TempDir::new().unwrap();
        let script = dir.path().join("ask");
        fs::write(
            &script,
            "#!/bin/sh\nread -r line\necho \"$(basename \"$PWD\") $GH_PROMPT_DISABLED $1 [$line]\"\n",
        )
        .unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        let folder = dir.path().join("csv");
        fs::create_dir(&folder).unwrap();
        let argv = [script.display().to_string(), "list".to_string()];

        assert_eq!(output_in(&folder, &argv, LIMIT).unwrap(), "csv 1 list []\n");

        let missing = ["saaio-test-no-such-program".to_string()];
        let error = output_in(&folder, &missing, LIMIT).unwrap_err();
        assert!(format!("{error:#}").starts_with("could not run saaio-test-no-such-program"));
    }

    const LIMIT: Duration = Duration::from_secs(10);

    #[test]
    fn a_command_that_gives_no_answer_in_time_is_killed() {
        let dir = TempDir::new().unwrap();
        let script = dir.path().join("hang");
        fs::write(&script, "#!/bin/sh\necho started\nexec sleep 30\n").unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        let argv = [script.display().to_string()];
        let started = Instant::now();

        let error = output_in(dir.path(), &argv, Duration::from_millis(200)).unwrap_err();

        assert!(started.elapsed() < Duration::from_secs(5));
        assert!(
            format!("{error:#}").ends_with("no answer within 0s"),
            "{error:#}"
        );
        let failing = dir.path().join("fail");
        fs::write(&failing, "#!/bin/sh\necho 'not logged in' >&2\nexit 4\n").unwrap();
        fs::set_permissions(&failing, fs::Permissions::from_mode(0o755)).unwrap();
        let error = output_in(dir.path(), &[failing.display().to_string()], LIMIT).unwrap_err();
        assert_eq!(format!("{error:#}"), "not logged in");
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
        create_worktree(&repo, &workspace).unwrap();
        let outside = TempDir::new().unwrap();
        let git_dir = repo.join(".git");

        assert_eq!(git_common_dir(&repo), Some(git_dir.clone()));
        assert_eq!(
            git_common_dir(&repo.join("src/deep")),
            Some(git_dir.clone())
        );
        assert_eq!(git_common_dir(Path::new(&workspace.path)), Some(git_dir));
        assert_eq!(git_common_dir(outside.path()), None);
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
        assert_eq!(exclude.matches(".saaio/worktrees/").count(), 1);
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
            "#!/bin/sh\necho \"$SAAIO_SLOT $(pwd)\" > ran.txt\n",
        );

        let ran = run_hook(
            &repo,
            Hook::Setup,
            &workspace,
            &[("SAAIO_SLOT".into(), "3".into())],
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
    fn runs_pause_and_resume_within_a_limit_with_the_flow_environment() {
        let (_dir, repo) = repo();
        let workspace = workspace_for(&repo, "shop-217", "flow/shop-217".into(), 3, "claude");
        create_worktree(&repo, &workspace).unwrap();
        let env = [("SAAIO_SLOT".to_string(), "3".to_string())];

        assert!(!has_hook(&repo, Hook::Pause));
        assert!(!run_hook_within(&repo, Hook::Pause, &workspace, &env, LIMIT).unwrap());
        hook(
            &repo,
            "pause",
            "#!/bin/sh\necho \"paused $SAAIO_SLOT $(pwd)\" > ran.txt\n",
        );
        hook(&repo, "resume", "#!/bin/sh\nexec sleep 30\n");

        assert!(has_hook(&repo, Hook::Pause));
        assert!(run_hook_within(&repo, Hook::Pause, &workspace, &env, LIMIT).unwrap());
        let written = fs::read_to_string(Path::new(&workspace.path).join("ran.txt")).unwrap();
        assert_eq!(written.trim(), format!("paused 3 {}", workspace.path));
        let started = Instant::now();
        let limit = Duration::from_millis(200);
        let error = run_hook_within(&repo, Hook::Resume, &workspace, &env, limit).unwrap_err();
        assert!(started.elapsed() < Duration::from_secs(5));
        assert_eq!(format!("{error:#}"), "no answer within 0s");
        fs::set_permissions(
            repo.join(".saaio/resume"),
            fs::Permissions::from_mode(0o644),
        )
        .unwrap();
        assert!(!has_hook(&repo, Hook::Resume), "not executable is none");
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

    #[test]
    fn reads_uncommitted_work_through_the_git_dir_the_repository_registered() {
        let (dir, repo) = repo();
        let space = workspace_for(&repo, "shop-217", "flow/shop-217".into(), 1, "claude");
        create_worktree(&repo, &space).unwrap();
        let path = Path::new(&space.path);
        let marker = dir.path().join("marker");
        let monitor = dir.path().join("monitor");
        fs::write(
            &monitor,
            format!("#!/bin/sh\ntouch '{}'\n", marker.display()),
        )
        .unwrap();
        fs::set_permissions(&monitor, fs::Permissions::from_mode(0o755)).unwrap();
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
        create_worktree(&repo, &space).unwrap();

        assert!(!has_uncommitted_work(&repo, &space).unwrap());
        fs::write(Path::new(&space.path).join("work.txt"), "unsaved").unwrap();
        assert!(has_uncommitted_work(&repo, &space).unwrap());
    }
}
