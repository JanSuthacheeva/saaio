//! Tests of kinds of flows: `saaio start --kind` and `saaio new --kind`, what each records, the
//! hook's phases, `saaio clear`, and how status shows them

use std::fs;
use std::process::Command;

use crate::support::*;

/// The flow's started line as JSON.
fn started_line(lab: &Lab) -> serde_json::Value {
    let log = fs::read_dir(lab.home.join("flows"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.extension().is_some_and(|ext| ext == "jsonl"))
        .unwrap();
    let text = fs::read_to_string(log).unwrap();
    serde_json::from_str(text.lines().next().unwrap()).unwrap()
}

fn types(lab: &Lab) {
    fs::create_dir_all(lab.repo.join(".saaio")).unwrap();
    fs::write(
        lab.repo.join(".saaio/config.toml"),
        "[branch]\nname = \"{type}/{ticket}-{slug}\"\ntypes = [\"feature\", \"bugfix\"]\n",
    )
    .unwrap();
}

#[test]
fn start_records_the_feature_kind_unless_asked_for_another() {
    let lab = Lab::new();

    let output = lab.start(&["SHOP-217 add CSV export"]);

    assert!(output.status.success(), "{}", stdout(&output));
    let started = started_line(&lab);
    assert_eq!(started["flow_kind"], "feature");
    assert_eq!(started["prompt"], "/ship-feature {request}");
    assert_eq!(
        started["workflow"][0],
        serde_json::json!({"name": "plan", "skills": ["design-code"]})
    );
    assert_eq!(started.get("phases"), None, "the workflow names them");
    assert_eq!(started.get("merges"), None, "it merges");
}

#[test]
fn a_fix_takes_the_kinds_branch_type_and_prompt() {
    let lab = Lab::new();
    types(&lab);

    let output = lab.start(&["SHOP-230 fix totals on refunds", "--kind", "fix"]);

    let text = stdout(&output);
    assert!(output.status.success(), "{text}");
    assert!(
        text.contains("branch: bugfix/SHOP-230-fix-totals-on-refunds"),
        "{text}"
    );
    let started = started_line(&lab);
    assert_eq!(started["flow_kind"], "fix");
    assert_eq!(started["prompt"], "/ship-fix {request}");
    assert_eq!(started["workflow"][0]["name"], "diagnose");
    assert_eq!(started["workflow"][0]["skills"][0], "diagnose-bug");
}

#[test]
fn a_research_flow_gets_a_detached_worktree_and_no_branch() {
    let lab = Lab::new();
    types(&lab);

    let output = lab.start(&["why are refunds rounded per line", "--kind", "research"]);

    let text = stdout(&output);
    assert!(output.status.success(), "{text}");
    assert!(!text.contains("\nbranch:"), "{text}");
    let worktree = lab.repo.join(".saaio/worktrees/why-are-refunds-rounded");
    let head = Command::new("git")
        .arg("-C")
        .arg(&worktree)
        .args(["symbolic-ref", "-q", "HEAD"])
        .output()
        .unwrap();
    assert!(!head.status.success(), "detached: {text}");
    let started = started_line(&lab);
    assert_eq!(started["flow_kind"], "research");
    assert_eq!(started["merges"], false);
    assert!(
        started["prompt"]
            .as_str()
            .unwrap()
            .starts_with("Research this and answer here."),
        "{started}"
    );
}

#[test]
fn start_refuses_an_unknown_kind_and_a_type_for_research_before_creating_anything() {
    let lab = Lab::new();
    types(&lab);

    let unknown = lab.start(&["SHOP-1 x", "--kind", "fixx"]);
    let typed = lab.start(&["SHOP-2 why", "--kind", "research", "--type", "bugfix"]);

    assert_eq!(unknown.status.code(), Some(1));
    let text = stdout(&unknown);
    assert!(text.contains("no kind named fixx"), "{text}");
    assert!(text.contains("Kinds: feature, fix, research"), "{text}");
    assert_eq!(typed.status.code(), Some(1));
    let text = stdout(&typed);
    assert!(
        text.contains("a research flow has no branch, so no branch type"),
        "{text}"
    );
    assert!(text.contains("Leave out --type"), "{text}");
    assert!(!lab.home.join("flows").exists() || started_lines(&lab) == 0);
}

fn started_lines(lab: &Lab) -> usize {
    fs::read_dir(lab.home.join("flows"))
        .unwrap()
        .filter(|entry| {
            entry
                .as_ref()
                .unwrap()
                .path()
                .extension()
                .is_some_and(|ext| ext == "jsonl")
        })
        .count()
}

#[test]
fn new_records_the_kind_it_is_given() {
    let lab = Lab::new();

    let output = lab.run(&[
        "new",
        "--name",
        "shop-230",
        "--request",
        "SHOP-230 fix totals",
        "--kind",
        "fix",
    ]);
    let unknown = lab.run(&["new", "--name", "x", "--request", "x", "--kind", "chore"]);

    assert!(output.status.success(), "{}", stdout(&output));
    let started = started_line(&lab);
    assert_eq!(started["flow_kind"], "fix");
    assert_eq!(started["workflow"][0]["name"], "diagnose");
    assert_eq!(started.get("prompt"), None, "saaio new opens no pane");
    assert_eq!(unknown.status.code(), Some(1));
    assert!(stdout(&unknown).contains("no kind named chore"));
}

#[test]
fn the_hook_moves_a_flow_through_the_workflow_it_recorded() {
    let lab = Lab::new();
    let created = lab.run(&[
        "new",
        "--name",
        "shop-230",
        "--request",
        "SHOP-230 fix totals",
        "--kind",
        "fix",
    ]);
    let id = id_in(&stdout(&created));
    let skill = |name: &str| {
        format!(
            r#"{{"hook_event_name":"PreToolUse","tool_name":"Skill","tool_input":{{"skill":"{name}"}}}}"#
        )
    };
    // The config changes after the start: the flow keeps what it recorded.
    fs::write(
        lab.home.join("no-config.toml"),
        "[[kinds.fix.phases]]\nname = \"triage\"\nskills = [\"diagnose-bug\"]\n",
    )
    .unwrap();

    let output = hook(&lab.home, Some(&id), &skill("diagnose-bug"));

    assert!(output.status.success(), "{}", stdout(&output));
    let detail = stdout(&lab.run(&["status", "shop-230"]));
    assert!(detail.contains("phase: diagnose"), "{detail}");
    hook(&lab.home, Some(&id), &skill("implement-plan"));
    let detail = stdout(&lab.run(&["status", "shop-230"]));
    assert!(detail.contains("phase: implement"), "{detail}");
}

#[test]
fn clear_forgets_a_research_flow_once_it_answered_and_keeps_a_done_feature() {
    let lab = Lab::new();
    git(
        &lab.repo,
        &[
            "remote",
            "add",
            "origin",
            "https://github.com/acme/shop.git",
        ],
    );
    assert!(
        lab.start(&["why refunds round", "--kind", "research"])
            .status
            .success()
    );
    assert!(lab.start(&["SHOP-2 export orders"]).status.success());
    assert!(
        lab.start(&["what calls the ledger", "--kind", "research"])
            .status
            .success()
    );
    let report = |name: &str, args: &[&str]| {
        let mut all = vec!["report", "--flow", name];
        all.extend_from_slice(args);
        let output = lab.run(&all);
        assert!(output.status.success(), "{}", stdout(&output));
    };
    report("why-refunds-round", &["idle", "They round per line."]);
    report(
        "shop-2",
        &["merge-request", "https://github.com/acme/shop/pull/2"],
    );

    let output = lab.run(&["clear"]);

    let text = stdout(&output);
    assert!(output.status.success(), "{text}");
    assert!(text.starts_with("cleared: 1\n"), "{text}");
    let worktrees = lab.repo.join(".saaio/worktrees");
    assert!(!worktrees.join("why-refunds-round").exists(), "{text}");
    assert!(worktrees.join("shop-2").exists(), "its session still runs");
    assert!(
        worktrees.join("what-calls-the-ledger").exists(),
        "not answered yet"
    );
    let list = stdout(&lab.run(&["status"]));
    assert!(!list.contains("why-refunds-round"), "{list}");
    let next = lab.start(&["SHOP-4 x"]);
    assert!(
        stdout(&next).contains("slot: 1"),
        "its slot is free again: {}",
        stdout(&next)
    );
}
