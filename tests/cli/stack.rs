//! Tests of stacked flows: `saaio start --base`, and what a base's status and removal say

use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::Command;

use tempfile::TempDir;

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

/// Flow `name`'s workspace in `repo` on flow/<name>, standing on `base`, appended to its log.
fn workspace_in(home: &Path, id: &str, repo: &Path, name: &str, base: Option<&str>) {
    let worktree = repo.join(".saaio/worktrees").join(name);
    fs::create_dir_all(&worktree).unwrap();
    let base = base.map_or(String::new(), |base| format!(r#","base":"{base}""#));
    let line = format!(
        r#"{{"v":1,"at":2,"kind":"workspace","path":"{}","branch":"flow/{name}","slot":null,"window":null,"agent":"claude"{base}}}"#,
        worktree.display()
    );
    fs::OpenOptions::new()
        .append(true)
        .open(home.join(format!("flows/{id}.jsonl")))
        .unwrap()
        .write_all(format!("{line}\n").as_bytes())
        .unwrap();
}

#[test]
fn a_merged_base_moves_the_flows_on_it_onto_where_it_merged() {
    let home = TempDir::new().unwrap();
    let repo = home.path().join("shop");
    fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q"]);
    git(
        &repo,
        &[
            "remote",
            "add",
            "origin",
            "https://github.com/acme/shop.git",
        ],
    );
    let base = new_flow(home.path(), "export");
    let stacked = new_flow(home.path(), "button");
    let other = new_flow(home.path(), "pdf");
    workspace_in(home.path(), &base, &repo, "export", Some("release"));
    workspace_in(home.path(), &stacked, &repo, "button", Some("flow/export"));
    workspace_in(home.path(), &other, &repo, "pdf", None);
    let _server = server_with_gh(
        home.path(),
        r#"#!/bin/sh
case "$*" in
  *'--head flow/export '*) echo '[{"baseRefName":"release","createdAt":"2099-01-01T00:00:00Z","state":"MERGED","url":"https://github.com/acme/shop/pull/7"}]' ;;
  *) echo '[]' ;;
esac
"#,
    );
    let log = home.path().join(format!("flows/{stacked}.jsonl"));

    let export = home.path().join(format!("flows/{base}.jsonl"));
    wait_until("no merged event", || {
        fs::read_to_string(&export)
            .unwrap()
            .contains(r#""kind":"merged""#)
    });
    wait_until("no base-merged event", || {
        fs::read_to_string(&log).unwrap().contains("base-merged")
    });

    let detail = stdout(&saaio(home.path(), &["status", "button"]));
    assert!(detail.contains("base: release\n"), "{detail}");
    assert!(
        detail.contains("base flow/export merged, now on release"),
        "{detail}"
    );
    let pdf = fs::read_to_string(home.path().join(format!("flows/{other}.jsonl"))).unwrap();
    assert!(!pdf.contains("base-merged"), "{pdf}");
    let merged = fs::read_to_string(home.path().join(format!("flows/{base}.jsonl"))).unwrap();
    assert!(!merged.contains("base-merged"), "{merged}");
    assert!(saaio(home.path(), &["--stop"]).status.success());
    let mut wait = waiter(home.path(), Some(&stacked), &[]);
    let (code, _, news) = ended(&mut wait, PATIENCE).expect("woken");
    assert_eq!(code, 2);
    assert!(
        news.contains(
            "news from saaio: flow/export, the branch this flow stands on, was merged into release."
        ),
        "{news}"
    );
    assert!(
        news.contains("git rebase --onto origin/release flow/export"),
        "{news}"
    );
}

/// A lab with flow shop-1 and shop-2 standing on its branch.
fn stack_of_two() -> Lab {
    let lab = Lab::new();
    assert!(lab.start(&["SHOP-1 add export"]).status.success());
    lab.commit_in("shop-1");
    let stacked = lab.start(&["SHOP-2 export button", "--base", "flow/shop-1"]);
    assert!(stacked.status.success(), "{}", stdout(&stacked));
    lab
}

const STANDS_ON_SHOP_1: &str = "shop-2 stands on flow/shop-1: it keeps its commits and rebases \
onto origin/flow/shop-1 or where that merged";

#[test]
fn removing_a_base_names_the_flows_that_stand_on_it() {
    let lab = stack_of_two();

    let removed = lab.run(&["remove", "shop-1"]);

    assert!(removed.status.success(), "{}", stdout(&removed));
    assert!(
        stdout(&removed).contains("branch: deleted\nstacked[1]: shop-2\n"),
        "its commits live on in flow/shop-2: {}",
        stdout(&removed)
    );
    assert!(
        stdout(&removed).contains(STANDS_ON_SHOP_1),
        "{}",
        stdout(&removed)
    );
    let alone = lab.run(&["remove", "shop-2", "--delete-unmerged"]);
    assert!(alone.status.success(), "{}", stdout(&alone));
    assert!(!stdout(&alone).contains("stand"), "{}", stdout(&alone));
}

#[test]
fn a_refused_removal_of_a_base_names_the_flows_that_stand_on_it() {
    let lab = stack_of_two();
    let worktree = lab.repo.join(".saaio/worktrees/shop-1");
    fs::write(worktree.join("more.txt"), "more").unwrap();
    git(&worktree, &["add", "more.txt"]);
    git(
        &worktree,
        &[
            "-c",
            "user.email=t@t",
            "-c",
            "user.name=t",
            "commit",
            "-q",
            "-m",
            "more",
        ],
    );

    let refused = lab.run(&["remove", "shop-1"]);

    assert_eq!(refused.status.code(), Some(1));
    assert!(
        stdout(&refused).contains("flow/shop-1 has 1 commit"),
        "{}",
        stdout(&refused)
    );
    assert!(
        stdout(&refused).contains(STANDS_ON_SHOP_1),
        "{}",
        stdout(&refused)
    );
}
