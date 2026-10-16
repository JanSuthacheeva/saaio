//! FLOW_COMMIT: the commit this binary is built from, so server.log can say which flow ran.

use std::path::Path;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let commit = git(&["rev-parse", "--short", "HEAD"]).unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=FLOW_COMMIT={commit}");
    if let Some(dir) = git(&["rev-parse", "--git-dir"]) {
        watch(&Path::new(&dir).join("HEAD"));
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
