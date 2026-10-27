//! The repo's `.saaio/setup`, `.saaio/teardown`, `.saaio/pause` and `.saaio/resume`

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use anyhow::{Result, bail};

use super::git;
use super::process::finish_within;
use crate::launch::pane::PROOF_ENV;
use crate::trust::Trust;
use crate::view::Workspace;

/// The repo's own hooks: .saaio/setup after the worktree exists, .saaio/teardown before it goes,
/// .saaio/pause and .saaio/resume around a flow's rest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hook {
    Setup,
    Teardown,
    Pause,
    Resume,
}

/// Runs <repo>/.saaio/<hook> in the worktree with env and no stdin when it exists and is
/// executable, killed with what it started and failed past `hook.limit()`; Ok(false) when there is
/// none. A non-zero exit is an Err with the last stderr line; a repo trust does not cover, an Err
/// naming it. Git in a hook that `pins_git` acts on the worktree's registered git dir.
pub fn run(
    trust: &Trust,
    repo: &Path,
    hook: Hook,
    workspace: &Workspace,
    env: &[(String, String)],
) -> Result<bool> {
    run_within(trust, repo, hook, workspace, env, hook.limit())
}

/// `run` with a limit of its own. Err when a pinned hook's worktree has no registered git dir.
pub fn run_within(
    trust: &Trust,
    repo: &Path,
    hook: Hook,
    workspace: &Workspace,
    env: &[(String, String)],
    limit: Duration,
) -> Result<bool> {
    let file = path(repo, hook);
    if !is_executable(&file) {
        return Ok(false);
    }
    if !trust.covers(repo) {
        bail!("{} is not trusted", repo.display());
    }
    let git_dir = if hook.pins_git() {
        Some(git::registered_git_dir(repo, Path::new(&workspace.path))?)
    } else {
        None
    };
    finish_within(
        command(&file, workspace, env, git_dir.as_deref()),
        &file.display().to_string(),
        limit,
    )?;
    Ok(true)
}

/// The hook in the worktree with `env`, without the key of the session that runs it, and with
/// GIT_DIR and GIT_WORK_TREE when `git_dir` is given.
fn command(
    file: &Path,
    workspace: &Workspace,
    env: &[(String, String)],
    git_dir: Option<&Path>,
) -> Command {
    let mut command = Command::new(file);
    command
        .current_dir(&workspace.path)
        .env_remove(PROOF_ENV)
        .envs(env.iter().map(|(key, value)| (key, value)));
    if let Some(git_dir) = git_dir {
        command
            .env("GIT_DIR", git_dir)
            .env("GIT_WORK_TREE", &workspace.path);
    }
    command
}

/// The executable hooks a repo has, in the order they run.
pub fn present(repo: &Path) -> Vec<Hook> {
    [Hook::Setup, Hook::Pause, Hook::Resume, Hook::Teardown]
        .into_iter()
        .filter(|hook| exists(repo, *hook))
        .collect()
}

/// Whether <repo>/.saaio/<hook> exists and is executable.
pub fn exists(repo: &Path, hook: Hook) -> bool {
    is_executable(&path(repo, hook))
}

impl Hook {
    /// How long it may run: setup 10 min, teardown 2 min, pause and resume 5 min.
    pub fn limit(self) -> Duration {
        match self {
            Hook::Setup => Duration::from_mins(10),
            Hook::Teardown => Duration::from_mins(2),
            Hook::Pause | Hook::Resume => Duration::from_mins(5),
        }
    }

    /// Whether git in it is pinned to the worktree's registered git dir: every hook that runs
    /// after the worker had the worktree (teardown, pause, resume), not setup.
    pub fn pins_git(self) -> bool {
        !matches!(self, Hook::Setup)
    }

    /// Its file under .saaio: setup, teardown, pause, resume.
    pub fn name(self) -> &'static str {
        match self {
            Hook::Setup => "setup",
            Hook::Teardown => "teardown",
            Hook::Pause => "pause",
            Hook::Resume => "resume",
        }
    }
}

fn path(repo: &Path, hook: Hook) -> PathBuf {
    repo.join(".saaio").join(hook.name())
}

fn is_executable(file: &Path) -> bool {
    fs::metadata(file).is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use tempfile::TempDir;

    use super::*;
    use crate::launch::worktree::workspace_for;
    use crate::registry::Registry;
    use crate::workspace::create_worktree;
    use crate::workspace::fixtures::{LIMIT, SOON, hook, repo};

    /// A trust that covers repo, kept in a folder of its own.
    fn trusted(repo: &Path) -> (TempDir, Trust) {
        let state = TempDir::new().unwrap();
        let trust = Trust::at(&Registry::at(state.path().to_path_buf()));
        trust.grant(repo).unwrap();
        (state, trust)
    }

    #[test]
    fn runs_no_hook_of_a_repo_trust_does_not_cover() {
        let (_dir, repo) = repo();
        let state = TempDir::new().unwrap();
        let trust = Trust::at(&Registry::at(state.path().to_path_buf()));
        let workspace = workspace_for(&repo, "shop-217", "flow/shop-217".into(), 1, "claude");
        create_worktree(&repo, &workspace, None).unwrap();
        hook(
            &repo,
            "setup",
            "#!/bin/sh
touch ran.txt
",
        );

        let error = run(&trust, &repo, Hook::Setup, &workspace, &[]).unwrap_err();

        assert_eq!(
            format!("{error:#}"),
            format!("{} is not trusted", repo.display())
        );
        assert!(!Path::new(&workspace.path).join("ran.txt").exists());
        assert!(!run(&trust, &repo, Hook::Teardown, &workspace, &[]).unwrap());
        assert_eq!(present(&repo), [Hook::Setup]);
    }

    #[test]
    fn every_hook_has_a_limit() {
        assert_eq!(Hook::Setup.limit(), Duration::from_mins(10));
        assert_eq!(Hook::Teardown.limit(), Duration::from_mins(2));
        assert_eq!(Hook::Pause.limit(), Duration::from_mins(5));
        assert_eq!(Hook::Resume.limit(), Duration::from_mins(5));
    }

    #[test]
    fn runs_a_hook_in_the_worktree_with_the_flow_environment() {
        let (_dir, repo) = repo();
        let (_state, trust) = trusted(&repo);
        let workspace = workspace_for(&repo, "shop-217", "flow/shop-217".into(), 3, "claude");
        create_worktree(&repo, &workspace, None).unwrap();
        hook(
            &repo,
            "setup",
            "#!/bin/sh\necho \"$SAAIO_SLOT $(pwd)\" > ran.txt\n",
        );

        let ran = run(
            &trust,
            &repo,
            Hook::Setup,
            &workspace,
            &[("SAAIO_SLOT".into(), "3".into())],
        )
        .unwrap();

        assert!(ran);
        let written = fs::read_to_string(Path::new(&workspace.path).join("ran.txt")).unwrap();
        assert_eq!(written.trim(), format!("3 {}", workspace.path));
        assert!(!run(&trust, &repo, Hook::Teardown, &workspace, &[]).unwrap());
    }

    #[test]
    fn a_failing_hook_reports_its_last_error_line() {
        let (_dir, repo) = repo();
        let (_state, trust) = trusted(&repo);
        let workspace = workspace_for(&repo, "shop-217", "flow/shop-217".into(), 1, "claude");
        create_worktree(&repo, &workspace, None).unwrap();
        hook(
            &repo,
            "setup",
            "#!/bin/sh\necho starting >&2\necho 'port 8001 is taken' >&2\nexit 3\n",
        );

        let error = run(&trust, &repo, Hook::Setup, &workspace, &[]).unwrap_err();

        assert_eq!(error.to_string(), "port 8001 is taken");
    }

    #[test]
    fn runs_pause_and_resume_within_a_limit_with_the_flow_environment() {
        let (_dir, repo) = repo();
        let (_state, trust) = trusted(&repo);
        let workspace = workspace_for(&repo, "shop-217", "flow/shop-217".into(), 3, "claude");
        create_worktree(&repo, &workspace, None).unwrap();
        let env = [("SAAIO_SLOT".to_string(), "3".to_string())];

        assert!(!exists(&repo, Hook::Pause));
        assert!(!run_within(&trust, &repo, Hook::Pause, &workspace, &env, LIMIT).unwrap());
        hook(
            &repo,
            "pause",
            "#!/bin/sh\necho \"paused $SAAIO_SLOT $(pwd)\" > ran.txt\n",
        );
        hook(&repo, "resume", "#!/bin/sh\nexec sleep 30\n");

        assert!(exists(&repo, Hook::Pause));
        assert!(run_within(&trust, &repo, Hook::Pause, &workspace, &env, LIMIT).unwrap());
        let written = fs::read_to_string(Path::new(&workspace.path).join("ran.txt")).unwrap();
        assert_eq!(written.trim(), format!("paused 3 {}", workspace.path));
        let started = Instant::now();
        let limit = Duration::from_millis(200);
        let error = run_within(&trust, &repo, Hook::Resume, &workspace, &env, limit).unwrap_err();
        assert!(started.elapsed() < SOON);
        assert_eq!(format!("{error:#}"), "no answer within 0s");
        fs::set_permissions(
            repo.join(".saaio/resume"),
            fs::Permissions::from_mode(0o644),
        )
        .unwrap();
        assert!(!exists(&repo, Hook::Resume), "not executable is none");
    }

    #[test]
    fn a_hook_that_leaves_a_process_behind_still_ends() {
        let (_dir, repo) = repo();
        let (_state, trust) = trusted(&repo);
        let workspace = workspace_for(&repo, "shop-217", "flow/shop-217".into(), 3, "claude");
        create_worktree(&repo, &workspace, None).unwrap();
        let pids = repo.join("pids");
        let leave = format!("sleep 30 &\necho $! >> {}", pids.display());
        hook(&repo, "resume", &format!("#!/bin/sh\necho up\n{leave}\n"));
        hook(
            &repo,
            "pause",
            &format!("#!/bin/sh\n{leave}\necho 'docker is not running' >&2\nexit 1\n"),
        );
        let started = Instant::now();

        assert!(run_within(&trust, &repo, Hook::Resume, &workspace, &[], LIMIT).unwrap());
        let error = run_within(&trust, &repo, Hook::Pause, &workspace, &[], LIMIT).unwrap_err();
        // A process left behind can hold the pipes of other tests' commands it inherited.
        for pid in fs::read_to_string(&pids).unwrap().split_whitespace() {
            Command::new("kill").arg(pid).status().unwrap();
        }

        assert!(started.elapsed() < SOON, "{:?}", started.elapsed());
        assert_eq!(format!("{error:#}"), "docker is not running");
    }

    /// Points the worktree's .git at another repository, which leaves `marker` once git uses it;
    /// returns that repository's git dir and the marker.
    fn tamper(workspace: &Workspace) -> (TempDir, PathBuf, PathBuf) {
        let decoy = TempDir::new().unwrap();
        let root = decoy.path().canonicalize().unwrap();
        crate::workspace::fixtures::git(&root, &["init", "-q"]);
        let marker = root.join("marker");
        let monitor = root.join("monitor");
        crate::fixtures::executable(
            &monitor,
            &format!(
                "#!/bin/sh
touch {}
",
                marker.display()
            ),
        );
        crate::workspace::fixtures::git(
            &root,
            &["config", "core.fsmonitor", &monitor.to_string_lossy()],
        );
        let git_dir = root.join(".git");
        fs::write(
            Path::new(&workspace.path).join(".git"),
            format!("gitdir: {}\n", git_dir.display()),
        )
        .unwrap();
        (decoy, git_dir, marker)
    }

    const GIT_SCRIPT: &str = "#!/bin/sh
git status >/dev/null 2>&1
git rev-parse --absolute-git-dir > git-dir.txt
";

    #[test]
    fn pins_git_to_the_registered_worktree_in_teardown_pause_and_resume() {
        let (_dir, repo) = repo();
        let (_state, trust) = trusted(&repo);
        let workspace = workspace_for(&repo, "shop-217", "flow/shop-217".into(), 1, "claude");
        create_worktree(&repo, &workspace, None).unwrap();
        let registered = repo.join(".git/worktrees/shop-217");
        let (_decoy, _git_dir, marker) = tamper(&workspace);

        for pinned in [Hook::Teardown, Hook::Pause, Hook::Resume] {
            hook(&repo, pinned.name(), GIT_SCRIPT);

            assert!(pinned.pins_git());
            assert!(run_within(&trust, &repo, pinned, &workspace, &[], LIMIT).unwrap());
            let git_dir = fs::read_to_string(Path::new(&workspace.path).join("git-dir.txt"));
            assert_eq!(
                git_dir.unwrap().trim(),
                registered.to_string_lossy(),
                "{}",
                pinned.name()
            );
            assert!(!marker.exists(), "{} read the decoy", pinned.name());
        }
    }

    #[test]
    fn leaves_git_in_setup_to_the_worktree() {
        let (_dir, repo) = repo();
        let (_state, trust) = trusted(&repo);
        let workspace = workspace_for(&repo, "shop-217", "flow/shop-217".into(), 1, "claude");
        create_worktree(&repo, &workspace, None).unwrap();
        let (_decoy, git_dir, marker) = tamper(&workspace);
        hook(&repo, "setup", GIT_SCRIPT);

        assert!(!Hook::Setup.pins_git());
        assert!(run(&trust, &repo, Hook::Setup, &workspace, &[]).unwrap());

        let found = fs::read_to_string(Path::new(&workspace.path).join("git-dir.txt")).unwrap();
        assert_eq!(found.trim(), git_dir.to_string_lossy());
        assert!(marker.exists(), "the decoy proves itself");
    }

    #[test]
    fn runs_no_pinned_hook_in_a_folder_that_is_no_worktree() {
        let (_dir, repo) = repo();
        let (_state, trust) = trusted(&repo);
        let workspace = workspace_for(&repo, "shop-217", "flow/shop-217".into(), 1, "claude");
        fs::create_dir_all(&workspace.path).unwrap();
        hook(
            &repo,
            "teardown",
            "#!/bin/sh
touch ran.txt
",
        );

        let error = run(&trust, &repo, Hook::Teardown, &workspace, &[]).unwrap_err();

        assert!(
            format!("{error:#}").ends_with(&format!("is no worktree of {}", repo.display())),
            "{error:#}"
        );
        assert!(!Path::new(&workspace.path).join("ran.txt").exists());
    }
}
