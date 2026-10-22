//! The repo's `.saaio/setup`, `.saaio/teardown`, `.saaio/pause` and `.saaio/resume`

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use anyhow::{Result, bail};

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
/// naming it.
pub fn run(
    trust: &Trust,
    repo: &Path,
    hook: Hook,
    workspace: &Workspace,
    env: &[(String, String)],
) -> Result<bool> {
    run_within(trust, repo, hook, workspace, env, hook.limit())
}

/// `run` with a limit of its own.
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
    finish_within(
        command(&file, workspace, env),
        &file.display().to_string(),
        limit,
    )?;
    Ok(true)
}

/// The hook in the worktree with `env`, without the key of the session that runs it.
fn command(file: &Path, workspace: &Workspace, env: &[(String, String)]) -> Command {
    let mut command = Command::new(file);
    command
        .current_dir(&workspace.path)
        .env_remove(PROOF_ENV)
        .envs(env.iter().map(|(key, value)| (key, value)));
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
        create_worktree(&repo, &workspace).unwrap();
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
        create_worktree(&repo, &workspace).unwrap();
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
        create_worktree(&repo, &workspace).unwrap();
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
        create_worktree(&repo, &workspace).unwrap();
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
        create_worktree(&repo, &workspace).unwrap();
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
}
