//! Tests of how a worker's hooks move its flow through the phases: forward on a skill, back to
//! implement on a change asked for after the review

use std::fs;
use std::path::Path;

use tempfile::TempDir;

use crate::support::*;

const ID: &str = "7f3c0000-0000-4000-8000-000000000000";

/// A feature flow saaio start began in `worktree`, as its log has it.
fn started_in(home: &Path, worktree: &Path) {
    let workspace = format!(
        r#"{{"v":1,"at":1500,"kind":"workspace","path":"{}","branch":"flow/shop-221","slot":1,"window":"saaio-ui","agent":"claude"}}"#,
        worktree.display()
    );
    seed(
        home,
        ID,
        &[
            r#"{"v":1,"at":1000,"kind":"started","name":"shop-221","request":"SHOP-221 add CSV export","mode":"auto"}"#,
            &workspace,
        ],
    );
}

fn send(home: &Path, payload: &str) {
    let output = hook(home, Some(ID), payload);
    assert!(output.status.success(), "{}", stderr(&output));
}

fn prompt(home: &Path, text: &str) {
    send(
        home,
        &format!(r#"{{"hook_event_name":"UserPromptSubmit","prompt":"{text}"}}"#),
    );
}

/// A Skill call; `agent` names the subagent that makes it, if one does.
fn skill(home: &Path, name: &str, agent: Option<&str>) {
    let agent = agent.map_or(String::new(), |id| format!(r#","agent_id":"{id}""#));
    send(
        home,
        &format!(
            r#"{{"hook_event_name":"PreToolUse","tool_name":"Skill","tool_input":{{"skill":"{name}"}}{agent}}}"#
        ),
    );
}

/// An Edit of `path` finished; `agent` names the subagent that made it, if one did.
fn edit(home: &Path, path: &Path, agent: Option<&str>) {
    let agent = agent.map_or(String::new(), |id| format!(r#","agent_id":"{id}""#));
    send(
        home,
        &format!(
            r#"{{"hook_event_name":"PostToolUse","tool_name":"Edit","tool_input":{{"file_path":"{}","old_string":"a","new_string":"b"}}{agent}}}"#,
            path.display()
        ),
    );
}

fn stop(home: &Path, answer: &str) {
    send(
        home,
        &format!(r#"{{"hook_event_name":"Stop","last_assistant_message":"{answer}"}}"#),
    );
}

/// The flow's phase and its steps' statuses, as `saaio status --json` has them.
fn where_it_stands(home: &Path) -> (String, Vec<String>) {
    let output = saaio(home, &["status", "--json"]);
    let board: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let flow = &board["flows"][0];
    let steps = flow["steps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|step| step["status"].as_str().unwrap().to_string())
        .collect();
    (flow["phase"].as_str().unwrap().to_string(), steps)
}

fn stands(phase: &str, steps: &[&str]) -> (String, Vec<String>) {
    let steps = steps.iter().copied().map(String::from).collect();
    (phase.to_string(), steps)
}

#[test]
fn a_change_asked_for_after_the_review_takes_the_flow_back_to_implement() {
    let home = TempDir::new().unwrap();
    let worktree = home.path().join("worktree");
    let code = worktree.join("src/csv.rs");
    started_in(home.path(), &worktree);

    prompt(home.path(), "/ship-feature add CSV export");
    skill(home.path(), "design-code", None);
    skill(home.path(), "implement-plan", None);
    edit(home.path(), &code, None);
    stop(home.path(), "Built.");
    prompt(home.path(), "continue shipping");
    skill(home.path(), "review-changes", Some("a1"));
    edit(home.path(), &code, Some("a1"));
    stop(home.path(), "Reviewed: nothing open.");
    assert_eq!(
        where_it_stands(home.path()),
        stands("review", &["done", "done", "current", "open"]),
        "the review fixing its own findings stays the review"
    );

    prompt(home.path(), "Also make the CSV header bold, please");
    edit(home.path(), &code, None);
    edit(home.path(), &code, None);
    let expected = stands("implement", &["done", "current", "stale", "open"]);
    assert_eq!(where_it_stands(home.path()), expected.clone());
    stop(home.path(), "Done: header bold.");
    assert_eq!(where_it_stands(home.path()), expected);
    let detail = stdout(&saaio(home.path(), &["status", "shop-221"]));
    assert!(detail.contains("review,stale"), "{detail}");

    let log = fs::read_to_string(home.path().join(format!("flows/{ID}.jsonl"))).unwrap();
    let edits = log
        .lines()
        .filter(|line| line.contains(r#""kind":"edited""#));
    assert_eq!(edits.count(), 1, "only the edit that stepped back: {log}");
}
