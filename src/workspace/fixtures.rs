use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use tempfile::TempDir;

use crate::fixtures::executable;

pub(super) const LIMIT: Duration = Duration::from_secs(10);
/// Under the 30 s the scripts' sleep takes, which only a command waited out reaches.
pub(super) const SOON: Duration = Duration::from_secs(20);

pub(super) fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?}");
}

pub(super) fn repo() -> (TempDir, PathBuf) {
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

pub(super) fn hook(repo: &Path, name: &str, script: &str) {
    let dir = repo.join(".saaio");
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join(name);
    executable(&file, script);
}
