//! Tests of `saaio remove` and `saaio clear`: teardown, kept work and branches, plain sessions

use std::fs;

use tempfile::TempDir;

use crate::support::*;

#[test]
fn clear_tears_down_and_removes_clean_worktrees_but_keeps_uncommitted_work() {
    let lab = Lab::new();
    lab.hook(
        "teardown",
        "#!/bin/sh\necho \"$SAAIO_FLOW_NAME\" >> \"$SAAIO_REPO/torn-down\"\n",
    );
    assert!(lab.start(&["SHOP-1 clean one"]).status.success());
    assert!(lab.start(&["SHOP-2 dirty one"]).status.success());
    assert!(lab.start(&["SHOP-3 still running"]).status.success());
    let worktrees = lab.repo.join(".saaio/worktrees");
    fs::write(worktrees.join("shop-2/work.txt"), "unsaved").unwrap();
    for name in ["shop-1", "shop-2"] {
        assert!(
            saaio(&lab.home, &["report", "--flow", name, "ended"])
                .status
                .success()
        );
    }

    let output = saaio(&lab.home, &["clear"]);

    let text = stdout(&output);
    assert!(output.status.success(), "{text}");
    assert!(text.starts_with("cleared: 2\n"), "{text}");
    assert!(
        text.contains(&format!(
            "kept[1]{{name,path}}:\n  shop-2,{}",
            worktrees.join("shop-2").display()
        )),
        "{text}"
    );
    assert!(!worktrees.join("shop-1").exists());
    assert!(worktrees.join("shop-2").exists());
    assert!(worktrees.join("shop-3").exists());
    let mut torn: Vec<String> = fs::read_to_string(lab.repo.join("torn-down"))
        .unwrap()
        .lines()
        .map(String::from)
        .collect();
    torn.sort();
    assert_eq!(torn, vec!["shop-1", "shop-2"]);
    assert_eq!(
        lab.branches(),
        vec!["flow/shop-2", "flow/shop-3"],
        "a cleared flow's branch goes when no commit of it is lost"
    );
}

#[test]
fn remove_tears_down_and_forgets_a_running_flow() {
    let lab = Lab::new();
    lab.hook(
        "teardown",
        "#!/bin/sh\necho \"$SAAIO_FLOW_NAME\" >> \"$SAAIO_REPO/torn-down\"\n",
    );
    assert!(lab.start(&["SHOP-1 first try"]).status.success());
    assert!(lab.start(&["SHOP-2 keep me"]).status.success());

    let output = lab.run(&["remove", "shop-1"]);

    let text = stdout(&output);
    assert!(output.status.success(), "{text}");
    assert!(
        text.starts_with("removed: shop-1\npane: none\nworktree: removed\nbranch: deleted\n"),
        "{text}"
    );
    assert!(!lab.repo.join(".saaio/worktrees/shop-1").exists());
    assert_eq!(
        fs::read_to_string(lab.repo.join("torn-down"))
            .unwrap()
            .trim(),
        "shop-1"
    );
    let list = stdout(&saaio(&lab.home, &["status"]));
    assert!(
        list.contains("count: 1") && list.contains("shop-2"),
        "{list}"
    );
}

#[test]
fn remove_forgets_a_plain_session_and_leaves_its_folder_alone() {
    let lab = Lab::new();
    lab.hook("teardown", "#!/bin/sh\ntouch \"$SAAIO_REPO/torn-down\"\n");
    let folder = lab.repo.join(".saaio/worktrees/mine");
    fs::create_dir_all(&folder).unwrap();
    fs::write(folder.join("notes.txt"), "keep").unwrap();
    assert!(
        lab.start(&["--plain", ".saaio/worktrees/mine"])
            .status
            .success()
    );

    let detail = stdout(&lab.run(&["status", "mine"]));
    assert!(
        detail.contains(&format!(
            "mode: plain\nstate: idle\nfolder: {}\n",
            folder.display()
        )),
        "{detail}"
    );
    assert!(
        ["branch:", "worktree:", "request:", "steps"]
            .iter()
            .all(|field| !detail.contains(field)),
        "{detail}"
    );

    let output = lab.run(&["remove", "mine"]);

    let text = stdout(&output);
    assert!(output.status.success(), "{text}");
    assert!(text.contains("worktree: none\nbranch: none"), "{text}");
    assert_eq!(
        fs::read_to_string(folder.join("notes.txt")).unwrap(),
        "keep"
    );
    assert!(!lab.repo.join("torn-down").exists(), "no teardown");
    assert!(stdout(&saaio(&lab.home, &["status"])).contains("count: 0"));
}

#[test]
fn remove_keeps_a_branch_with_its_own_commits_unless_told() {
    let lab = Lab::new();
    assert!(lab.start(&["SHOP-1 first try"]).status.success());
    lab.commit_in("shop-1");

    let refused = lab.run(&["remove", "shop-1"]);

    let text = stdout(&refused);
    assert_eq!(refused.status.code(), Some(1), "{text}");
    assert!(
        text.contains("flow/shop-1 has 1 commit on no other branch"),
        "{text}"
    );
    assert!(
        text.contains("saaio remove shop-1 --delete-unmerged"),
        "{text}"
    );
    assert!(
        lab.repo.join(".saaio/worktrees/shop-1").exists(),
        "nothing touched"
    );
    assert!(stdout(&lab.run(&[])).contains("shop-1"));

    let deleted = lab.run(&["remove", "shop-1", "--delete-unmerged"]);

    let text = stdout(&deleted);
    assert!(deleted.status.success(), "{text}");
    assert!(text.contains("branch: deleted"), "{text}");
    assert!(lab.branches().is_empty());
}

#[test]
fn remove_refuses_uncommitted_work_unless_forced() {
    let lab = Lab::new();
    assert!(lab.start(&["SHOP-1 dirty"]).status.success());
    let worktree = lab.repo.join(".saaio/worktrees/shop-1");
    fs::write(worktree.join("work.txt"), "unsaved").unwrap();

    let refused = lab.run(&["remove", "shop-1"]);
    let text = stdout(&refused);
    assert_eq!(refused.status.code(), Some(1), "{text}");
    assert!(
        text.starts_with("error: shop-1 has uncommitted work"),
        "{text}"
    );
    assert!(text.contains("saaio remove shop-1 --force"), "{text}");
    assert!(worktree.exists());
    assert!(stdout(&lab.run(&[])).contains("shop-1"));

    let forced = lab.run(&["remove", "shop-1", "--force"]);
    assert!(
        stdout(&forced).contains("worktree: removed"),
        "{}",
        stdout(&forced)
    );
    assert!(!worktree.exists());
}

#[test]
fn remove_says_when_git_cannot_check_a_worktree() {
    let lab = Lab::new();
    assert!(lab.start(&["SHOP-1 unlisted"]).status.success());
    let worktree = lab.repo.join(".saaio/worktrees/shop-1");
    fs::remove_dir_all(lab.repo.join(".git/worktrees/shop-1")).unwrap();

    let refused = lab.run(&["remove", "shop-1"]);

    let text = stdout(&refused);
    assert_eq!(refused.status.code(), Some(1), "{text}");
    assert!(
        text.starts_with(&format!(
            "error: git cannot check {} for uncommitted changes (",
            worktree.display()
        )),
        "{text}"
    );
    assert!(text.contains("is no worktree of"), "{text}");
    assert!(text.contains("saaio remove shop-1 --force"), "{text}");
    assert!(worktree.exists());
}

#[test]
fn remove_runs_nothing_the_worktree_configures() {
    let lab = Lab::new();
    assert!(lab.start(&["SHOP-1 planted"]).status.success());
    let worktree = lab.repo.join(".saaio/worktrees/shop-1");
    let scratch = TempDir::new().unwrap();
    let marker = scratch.path().join("marker");
    let monitor = scratch.path().join("monitor");
    executable(
        &monitor,
        &format!("#!/bin/sh\ntouch '{}'\n", marker.display()),
    );
    let planted = scratch.path().join("planted");
    git(scratch.path(), &["init", "-q", "planted"]);
    git(
        &planted,
        &["config", "core.fsmonitor", monitor.to_str().unwrap()],
    );
    fs::write(
        worktree.join(".git"),
        format!("gitdir: {}\n", planted.join(".git").display()),
    )
    .unwrap();

    let output = lab.run(&["remove", "shop-1"]);

    assert!(output.status.success(), "{}", stdout(&output));
    assert!(!marker.exists(), "git ran the worktree's fsmonitor");
    assert!(
        stdout(&output).contains("worktree: \"kept: "),
        "git refuses a worktree whose .git points elsewhere: {}",
        stdout(&output)
    );
}

#[test]
fn remove_says_when_there_is_no_such_flow() {
    let lab = Lab::new();

    let output = lab.run(&["remove", "shop-999"]);

    assert_eq!(output.status.code(), Some(1));
    assert!(
        stdout(&output).starts_with("error: no flow named shop-999"),
        "{}",
        stdout(&output)
    );
}
