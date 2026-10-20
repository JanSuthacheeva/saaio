use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use anyhow::{Context, Result, bail};

use super::process::{finish_within, last_line};
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

/// Runs <repo>/.saaio/<hook> in the worktree with env when it exists and is executable;
/// Ok(false) when there is none. A non-zero exit is an Err with the last stderr line.
pub fn run(
    repo: &Path,
    hook: Hook,
    workspace: &Workspace,
    env: &[(String, String)],
) -> Result<bool> {
    let file = path(repo, hook);
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

/// `run` with no stdin, killed and failed past `limit` ("no answer within 5m").
pub fn run_within(
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
    let mut command = Command::new(&file);
    command
        .current_dir(&workspace.path)
        .envs(env.iter().map(|(key, value)| (key, value)));
    finish_within(command, &file.display().to_string(), limit)?;
    Ok(true)
}

/// Whether <repo>/.saaio/<hook> exists and is executable.
pub fn exists(repo: &Path, hook: Hook) -> bool {
    is_executable(&path(repo, hook))
}

fn path(repo: &Path, hook: Hook) -> PathBuf {
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

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use super::*;
    use crate::launch::worktree::workspace_for;
    use crate::workspace::create_worktree;
    use crate::workspace::fixtures::{LIMIT, hook, repo};

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

        let ran = run(
            &repo,
            Hook::Setup,
            &workspace,
            &[("SAAIO_SLOT".into(), "3".into())],
        )
        .unwrap();

        assert!(ran);
        let written = fs::read_to_string(Path::new(&workspace.path).join("ran.txt")).unwrap();
        assert_eq!(written.trim(), format!("3 {}", workspace.path));
        assert!(!run(&repo, Hook::Teardown, &workspace, &[]).unwrap());
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

        let error = run(&repo, Hook::Setup, &workspace, &[]).unwrap_err();

        assert_eq!(error.to_string(), "port 8001 is taken");
    }

    #[test]
    fn runs_pause_and_resume_within_a_limit_with_the_flow_environment() {
        let (_dir, repo) = repo();
        let workspace = workspace_for(&repo, "shop-217", "flow/shop-217".into(), 3, "claude");
        create_worktree(&repo, &workspace).unwrap();
        let env = [("SAAIO_SLOT".to_string(), "3".to_string())];

        assert!(!exists(&repo, Hook::Pause));
        assert!(!run_within(&repo, Hook::Pause, &workspace, &env, LIMIT).unwrap());
        hook(
            &repo,
            "pause",
            "#!/bin/sh\necho \"paused $SAAIO_SLOT $(pwd)\" > ran.txt\n",
        );
        hook(&repo, "resume", "#!/bin/sh\nexec sleep 30\n");

        assert!(exists(&repo, Hook::Pause));
        assert!(run_within(&repo, Hook::Pause, &workspace, &env, LIMIT).unwrap());
        let written = fs::read_to_string(Path::new(&workspace.path).join("ran.txt")).unwrap();
        assert_eq!(written.trim(), format!("paused 3 {}", workspace.path));
        let started = Instant::now();
        let limit = Duration::from_millis(200);
        let error = run_within(&repo, Hook::Resume, &workspace, &env, limit).unwrap_err();
        assert!(started.elapsed() < Duration::from_secs(5));
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

        assert!(run_within(&repo, Hook::Resume, &workspace, &[], LIMIT).unwrap());
        let error = run_within(&repo, Hook::Pause, &workspace, &[], LIMIT).unwrap_err();
        // A process left behind can hold the pipes of other tests' commands it inherited.
        for pid in fs::read_to_string(&pids).unwrap().split_whitespace() {
            Command::new("kill").arg(pid).status().unwrap();
        }

        // Short of the 30 s the processes left behind run; a new script's first exec can take seconds.
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "{:?}",
            started.elapsed()
        );
        assert_eq!(format!("{error:#}"), "docker is not running");
    }
}
