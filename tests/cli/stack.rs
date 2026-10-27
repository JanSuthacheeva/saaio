//! Tests of stacked flows: `saaio start --base`, and what a base's status and removal say

use std::path::Path;
use std::process::Command;

use crate::support::*;

fn rev(dir: &Path, rev: &str) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["rev-parse", rev])
        .output()
        .unwrap();
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

fn has_upstream(dir: &Path, branch: &str) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(dir)
        .args([
            "rev-parse",
            "--abbrev-ref",
            &format!("{branch}@{{upstream}}"),
        ])
        .output()
        .unwrap()
        .status
        .success()
}

#[test]
fn start_stacks_a_flow_on_another_flows_branch() {
    let lab = Lab::new();
    assert!(lab.start(&["SHOP-1 add export"]).status.success());
    lab.commit_in("shop-1");

    let stacked = lab.start(&["SHOP-2 export button", "--base", "flow/shop-1"]);

    assert!(stacked.status.success(), "{}", stdout(&stacked));
    assert!(
        stdout(&stacked).contains("branch: flow/shop-2\nbase: flow/shop-1\n"),
        "{}",
        stdout(&stacked)
    );
    let worktree = lab.repo.join(".saaio/worktrees/shop-2");
    assert_eq!(rev(&worktree, "HEAD"), rev(&lab.repo, "flow/shop-1"));
    assert!(!has_upstream(&lab.repo, "flow/shop-2"));
    let detail = stdout(&lab.run(&["status", "shop-2"]));
    assert!(
        detail.contains("branch: flow/shop-2\nbase: flow/shop-1\n"),
        "{detail}"
    );
    assert!(!stdout(&lab.run(&["status", "shop-1"])).contains("base:"));
    let board: serde_json::Value =
        serde_json::from_slice(&lab.run(&["status", "--json"]).stdout).unwrap();
    let base_of = |name: &str| {
        board["flows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|flow| flow["name"] == name)
            .unwrap()["workspace"]["base"]
            .clone()
    };
    assert_eq!(base_of("shop-2"), "flow/shop-1");
    assert_eq!(base_of("shop-1"), serde_json::Value::Null);
}

#[test]
fn start_fetches_a_base_only_a_remote_has() {
    let lab = Lab::new();
    let origin = lab.dir.path().join("origin.git");
    let origin = origin.to_str().unwrap();
    git(&lab.repo, &["branch", "release"]);
    git(&lab.repo, &["clone", "-q", "--bare", ".", origin]);
    git(&lab.repo, &["branch", "-D", "release"]);
    git(&lab.repo, &["remote", "add", "origin", origin]);

    let output = lab.start(&["SHOP-3 hotfix", "--base", "release"]);

    assert!(output.status.success(), "{}", stdout(&output));
    assert!(
        stdout(&output).contains("base: release\n"),
        "{}",
        stdout(&output)
    );
    let worktree = lab.repo.join(".saaio/worktrees/shop-3");
    assert_eq!(
        rev(&worktree, "HEAD"),
        rev(&lab.repo, "refs/remotes/origin/release")
    );
    assert!(!has_upstream(&lab.repo, "flow/shop-3"));
    assert_eq!(lab.branches(), ["flow/shop-3"]);
}

#[test]
fn start_refuses_a_base_nowhere_or_no_branch_before_making_anything() {
    let lab = Lab::new();

    let nowhere = lab.start(&["SHOP-4 x", "--base", "flow/nowhere"]);
    let invalid = lab.start(&["SHOP-4 x", "--base", "a..b"]);
    let plain = lab.run(&["start", "--plain", ".", "--base", "main"]);

    assert_eq!(nowhere.status.code(), Some(1));
    assert!(
        stdout(&nowhere).contains("no branch flow/nowhere here or on a remote"),
        "{}",
        stdout(&nowhere)
    );
    assert!(
        stdout(&nowhere).contains("git branch -a"),
        "{}",
        stdout(&nowhere)
    );
    assert_eq!(invalid.status.code(), Some(1));
    assert!(
        stdout(&invalid).contains("a..b is no branch name"),
        "{}",
        stdout(&invalid)
    );
    assert_eq!(plain.status.code(), Some(2), "{}", stderr(&plain));
    assert!(lab.branches().is_empty());
    assert!(!lab.repo.join(".saaio/worktrees").exists());
    assert!(
        !stdout(&lab.run(&[])).contains("shop-4"),
        "nothing was created"
    );
}
