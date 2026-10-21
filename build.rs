//! `SAAIO_COMMIT`, the commit `proto::BUILD` names, set again once HEAD moves; `SAAIO_RELEASE`,
//! the tag release.yml builds

#[path = "build/commit.rs"]
mod commit;

use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

#[allow(
    clippy::disallowed_methods,
    reason = "cargo hands a build script its input as variables"
)]
fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=build/commit.rs");
    println!("cargo:rerun-if-env-changed=SAAIO_COMMIT");
    println!("cargo:rerun-if-env-changed=SAAIO_RELEASE");
    let release = env::var("SAAIO_RELEASE").unwrap_or_default();
    println!("cargo:rustc-env=SAAIO_RELEASE={}", release.trim());
    let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("cargo sets it"));
    let own = git(&["rev-parse", "--show-toplevel"])
        .is_some_and(|toplevel| commit::is_own(Path::new(&toplevel), &manifest));
    let head = own
        .then(|| git(&["rev-parse", "--short", "HEAD"]))
        .flatten();
    let stamp = commit::stamp(env::var("SAAIO_COMMIT").ok(), head);
    println!("cargo:rustc-env=SAAIO_COMMIT={stamp}");
    if own {
        watch_head();
    }
}

/// Reruns the build script once HEAD moves to a new commit.
fn watch_head() {
    if let Some(dir) = git(&["rev-parse", "--git-dir"]) {
        let dir = Path::new(&dir);
        watch(&dir.join("HEAD"));
        // Every commit appends here, also one that creates a branch's loose ref after a pack.
        watch(&dir.join("logs").join("HEAD"));
    }
    if let Some(common) = git(&["rev-parse", "--git-common-dir"]) {
        let common = Path::new(&common);
        watch(&common.join("packed-refs"));
        if let Some(branch) = git(&["symbolic-ref", "-q", "HEAD"]) {
            watch(&common.join(branch));
        }
    }
}

/// A file whose change means a new commit; only one that exists, as cargo reruns for a missing one
/// on every build.
fn watch(file: &Path) {
    if file.exists() {
        println!("cargo:rerun-if-changed={}", file.display());
    }
}

fn git(args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    Some(text.trim().to_string()).filter(|text| !text.is_empty())
}
