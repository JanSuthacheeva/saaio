//! Tests of `saaio hook`: reports, tool calls waiting on services, Codex, the master, MRs

use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Output};
use std::thread;
use std::time::{Duration, Instant};

use tempfile::TempDir;

use crate::support::*;

#[test]
fn a_hook_event_reports_into_the_flow_silently_and_only_once() {
    let home = TempDir::new().unwrap();
    let id = new_flow(home.path(), "shop-221");
    let question = r#"{"hook_event_name":"PreToolUse","tool_name":"AskUserQuestion","tool_input":{"questions":[{"question":"Red or blue?"}]}}"#;
    let skill = r#"{"hook_event_name":"PreToolUse","tool_name":"Skill","tool_input":{"skill":"design-code"}}"#;

    let first = hook(home.path(), Some(&id), question);
    let again = hook(home.path(), Some(&id), question);
    let phase = hook(home.path(), Some(&id), skill);

    for output in [&first, &again, &phase] {
        assert!(output.status.success());
        assert!(output.stdout.is_empty(), "{}", stdout(output));
    }
    let detail = stdout(&saaio(home.path(), &["status", "shop-221"]));
    assert!(detail.contains("phase: plan"), "{detail}");
    let log = fs::read_to_string(home.path().join(format!("flows/{id}.jsonl"))).unwrap();
    assert_eq!(
        log.lines()
            .filter(|line| line.contains("Red or blue?"))
            .count(),
        1,
        "{log}"
    );
}

/// Appends a services line to the flow's log, as saaio server does.
fn services(home: &Path, id: &str, status: &str) {
    let line = format!(r#"{{"v":1,"at":1,"kind":"services","status":"{status}"}}"#);
    let mut log = fs::OpenOptions::new()
        .append(true)
        .open(home.join(format!("flows/{id}.jsonl")))
        .unwrap();
    writeln!(log, "{line}").unwrap();
}

const BASH: &str = r#"{"hook_event_name":"PreToolUse","tool_name":"Bash","tool_input":{"command":"php artisan test"}}"#;

#[test]
fn a_tool_call_waits_while_its_services_resume() {
    let home = TempDir::new().unwrap();
    let id = new_flow(home.path(), "shop-221");
    services(home.path(), &id, "resuming");
    let (path, flow) = (home.path().to_path_buf(), id.clone());
    let resumed = thread::spawn(move || {
        thread::sleep(Duration::from_millis(300));
        services(&path, &flow, "running");
    });
    let started = Instant::now();

    let output = hook(home.path(), Some(&id), BASH);

    resumed.join().unwrap();
    assert!(output.status.success());
    assert_eq!(stdout(&output), "");
    let waited = started.elapsed();
    assert!(waited >= Duration::from_millis(300), "{waited:?}");
    assert!(waited < Duration::from_secs(5), "{waited:?}");
}

#[test]
fn a_tool_call_of_paused_services_without_a_server_waits_only_a_moment() {
    let home = TempDir::new().unwrap();
    let id = new_flow(home.path(), "shop-221");
    services(home.path(), &id, "paused");
    let prompt = r#"{"hook_event_name":"UserPromptSubmit","prompt":"go on"}"#;

    let started = Instant::now();
    assert!(hook(home.path(), Some(&id), prompt).status.success());
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "only tool calls wait"
    );
    let output = hook(home.path(), Some(&id), BASH);

    assert!(output.status.success());
    let waited = started.elapsed();
    assert!(waited >= Duration::from_secs(2), "{waited:?}");
    assert!(waited < Duration::from_secs(5), "{waited:?}");
    let log = fs::read_to_string(home.path().join("server.log")).unwrap();
    assert!(log.contains("services paused and no resume began"), "{log}");
}

#[test]
fn a_tool_call_of_a_resting_flow_with_paused_services_marks_it_working() {
    let home = TempDir::new().unwrap();
    let id = new_flow(home.path(), "shop-221");
    let stop = r#"{"hook_event_name":"Stop","last_assistant_message":"All green."}"#;
    assert!(hook(home.path(), Some(&id), stop).status.success());
    services(home.path(), &id, "paused");
    assert_eq!(only_state(home.path()), "idle");

    assert!(hook(home.path(), Some(&id), BASH).status.success());

    assert_eq!(only_state(home.path()), "working");
}

/// The state `saaio status --json` gives the store's only flow.
fn only_state(home: &Path) -> String {
    let output = saaio(home, &["status", "--json"]);
    let board: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    board["flows"][0]["state"].as_str().unwrap().to_string()
}

#[test]
fn a_tool_call_after_a_question_the_master_answered_marks_the_flow_working() {
    let home = TempDir::new().unwrap();
    let id = new_flow(home.path(), "shop-221");
    let stop = r#"{"hook_event_name":"Stop","last_assistant_message":"Shall I go on?"}"#;
    assert!(hook(home.path(), Some(&id), stop).status.success());
    services(home.path(), &id, "paused");
    assert_eq!(only_state(home.path()), "waiting");

    assert!(hook(home.path(), Some(&id), BASH).status.success());

    assert_eq!(only_state(home.path()), "working");
}

#[test]
fn a_tool_call_that_asks_keeps_a_flow_with_paused_services_waiting() {
    let home = TempDir::new().unwrap();
    let id = new_flow(home.path(), "shop-221");
    services(home.path(), &id, "paused");
    let question = r#"{"hook_event_name":"PreToolUse","tool_name":"AskUserQuestion","tool_input":{"questions":[{"question":"Red or blue?"}]}}"#;

    assert!(hook(home.path(), Some(&id), question).status.success());

    assert_eq!(only_state(home.path()), "waiting");
}

#[test]
fn a_tool_call_of_an_ended_flow_with_paused_services_appends_no_work() {
    let home = TempDir::new().unwrap();
    let id = new_flow(home.path(), "shop-221");
    let end = r#"{"hook_event_name":"SessionEnd","reason":"prompt_input_exit"}"#;
    assert!(hook(home.path(), Some(&id), end).status.success());
    services(home.path(), &id, "paused");
    let log = home.path().join(format!("flows/{id}.jsonl"));
    let before = fs::read_to_string(&log).unwrap().lines().count();

    assert!(hook(home.path(), Some(&id), BASH).status.success());

    assert_eq!(fs::read_to_string(&log).unwrap().lines().count(), before);
}

#[test]
fn a_recorded_codex_skill_read_advances_the_flow_once() {
    let home = TempDir::new().unwrap();
    let id = new_flow(home.path(), "codex-skill-read");
    let reads: serde_json::Value = serde_json::from_str(include_str!(
        "../../src/adapter/fixtures/codex/read_ahead_separate_commands.json"
    ))
    .unwrap();
    let single = reads[0].to_string();
    let mut multi: serde_json::Value = serde_json::from_str(include_str!(
        "../../src/adapter/fixtures/codex/pre_bash_skills_together.json"
    ))
    .unwrap();
    multi["tool_input"]["command"] = multi["tool_input"]["command"]
        .as_str()
        .unwrap()
        .replace("demo-skill", "design-code")
        .into();

    for payload in [&single, &multi.to_string()] {
        let output = hook_with(home.path(), &["codex"], Some(&id), payload);
        assert!(output.status.success());
        assert!(output.stdout.is_empty() && output.stderr.is_empty());
    }
    let board: serde_json::Value =
        serde_json::from_slice(&saaio(home.path(), &["status", "--json"]).stdout).unwrap();
    assert_eq!(board["flows"][0]["phase"], "plan");
    let log = fs::read_to_string(home.path().join(format!("flows/{id}.jsonl"))).unwrap();
    assert_eq!(log.matches("\"kind\":\"phase\"").count(), 1, "{log}");
}

#[test]
fn a_codex_permission_waits_only_when_the_user_answers_it() {
    let home = TempDir::new().unwrap();
    let rollout = home.path().join("rollout.jsonl");
    fs::write(
        &rollout,
        include_str!("../../src/adapter/fixtures/codex/rollout_auto_review.jsonl"),
    )
    .unwrap();
    let pointed = |payload: &str, path: &Path| {
        let mut payload: serde_json::Value = serde_json::from_str(payload).unwrap();
        payload["transcript_path"] = path.to_str().unwrap().into();
        payload.to_string()
    };
    let reviewed = pointed(
        include_str!("../../src/adapter/fixtures/codex/permission_bash_reviewed.json"),
        &rollout,
    );
    let by_user = pointed(
        include_str!("../../src/adapter/fixtures/codex/permission_bash.json"),
        &rollout,
    );
    let unread = pointed(
        include_str!("../../src/adapter/fixtures/codex/permission_bash_reviewed.json"),
        &home.path().join("missing.jsonl"),
    );
    let pending = |id: &str| {
        let log = fs::read_to_string(home.path().join(format!("flows/{id}.jsonl"))).unwrap();
        log.matches("\"kind\":\"permission-pending\"").count()
    };

    for (name, payload, expected) in [
        ("auto-review", &reviewed, 0),
        ("by-user", &by_user, 1),
        ("unread", &unread, 1),
    ] {
        let id = new_flow(home.path(), name);
        let output = hook_with(home.path(), &["codex"], Some(&id), payload);
        assert!(output.status.success());
        assert!(output.stdout.is_empty() && output.stderr.is_empty());
        assert_eq!(pending(&id), expected, "{name}");
    }
    let server_log = fs::read_to_string(home.path().join("server.log")).unwrap();
    assert!(
        server_log.contains("could not read the rollout") && server_log.contains("missing.jsonl"),
        "{server_log}"
    );
}

/// What `saaio send` would append: the master's message to the flow's worker.
fn message_for(home: &Path, id: &str, text: &str) {
    let line = format!("{{\"v\":1,\"at\":5,\"kind\":\"message\",\"text\":\"{text}\"}}\n");
    let mut log = fs::OpenOptions::new()
        .append(true)
        .open(home.join(format!("flows/{id}.jsonl")))
        .unwrap();
    log.write_all(line.as_bytes()).unwrap();
}

#[test]
fn a_codex_stop_hands_the_masters_message_over_and_goes_on_working() {
    let home = TempDir::new().unwrap();
    let id = new_flow(home.path(), "shop-221");
    let prompt = r#"{"session_id":"01a14000-0000-7000-8000-000000000001","hook_event_name":"UserPromptSubmit","prompt":"go"}"#;
    let stop = r#"{"session_id":"01a14000-0000-7000-8000-000000000001","hook_event_name":"Stop","last_assistant_message":"Tabs or spaces?"}"#;
    let busy = home.path().join(format!("waits/{id}.busy"));

    let started = hook_with(home.path(), &["codex"], Some(&id), prompt);
    assert!(started.status.success());
    assert!(busy.exists(), "busy from the prompt on");
    let idle = hook_with(home.path(), &["codex"], Some(&id), stop);
    assert!(idle.status.success(), "nothing to hand over");
    assert!(idle.stdout.is_empty() && idle.stderr.is_empty());
    assert!(!busy.exists(), "idle after its Stop");

    message_for(home.path(), &id, "Tabs.");
    let handed = hook_with(home.path(), &["codex"], Some(&id), stop);

    assert_eq!(handed.status.code(), Some(2), "Codex goes on with stderr");
    assert_eq!(stdout(&handed), "");
    let news = String::from_utf8_lossy(&handed.stderr);
    assert!(news.contains("message from the saaio master:"), "{news}");
    assert!(news.contains("Tabs."), "{news}");
    let detail = stdout(&saaio(home.path(), &["status", "shop-221", "--json"]));
    assert!(detail.contains(r#""state": "working""#), "{detail}");
    assert!(
        detail.contains(r#""id": "01a14000-0000-7000-8000-000000000001""#),
        "the session it picked: {detail}"
    );
    let again = hook_with(home.path(), &["codex"], Some(&id), stop);
    assert!(again.status.success(), "delivered once");
}

#[test]
fn a_claude_code_turn_marks_the_session_busy_and_its_stop_hands_nothing_over() {
    let home = TempDir::new().unwrap();
    let id = new_flow(home.path(), "shop-221");
    let busy = home.path().join(format!("waits/{id}.busy"));
    let prompt = r#"{"session_id":"7f3c0000-0000-4000-8000-000000000000","hook_event_name":"UserPromptSubmit","prompt":"go"}"#;
    let stop = r#"{"session_id":"7f3c0000-0000-4000-8000-000000000000","hook_event_name":"Stop","last_assistant_message":"Tabs or spaces?","background_tasks":[]}"#;
    let compact = |event: &str| {
        format!(
            r#"{{"session_id":"7f3c0000-0000-4000-8000-000000000000","hook_event_name":"{event}","trigger":"manual"}}"#
        )
    };

    assert!(hook(home.path(), Some(&id), prompt).status.success());
    assert_eq!(fs::read_to_string(&busy).unwrap(), "turn");
    message_for(home.path(), &id, "Tabs.");
    let stopped = hook(home.path(), Some(&id), stop);
    assert!(
        stopped.status.success(),
        "saaio wait wakes it, not its Stop"
    );
    assert!(stopped.stdout.is_empty() && stopped.stderr.is_empty());
    assert!(!busy.exists(), "idle after its Stop");

    assert!(
        hook(home.path(), Some(&id), &compact("PreCompact"))
            .status
            .success()
    );
    assert!(busy.exists(), "busy while it compacts");
    assert!(
        hook(home.path(), Some(&id), &compact("PostCompact"))
            .status
            .success()
    );
    assert!(
        !busy.exists(),
        "idle once the compaction ends, with no Stop"
    );
    let log = fs::read_to_string(home.path().join(format!("flows/{id}.jsonl"))).unwrap();
    assert!(!log.contains("Compact"), "{log}");
}

#[test]
fn a_claude_code_turn_that_asks_beside_a_background_task_waits_for_the_person() {
    let home = TempDir::new().unwrap();
    let id = new_flow(home.path(), "shop-221");
    let busy = home.path().join(format!("waits/{id}.busy"));
    let prompt = r#"{"session_id":"7f3c0000-0000-4000-8000-000000000000","hook_event_name":"UserPromptSubmit","prompt":"go"}"#;
    let stop = r#"{"session_id":"7f3c0000-0000-4000-8000-000000000000","hook_event_name":"Stop","last_assistant_message":"The server runs.\n\nWhich color do you prefer, red or blue?","background_tasks":[{"id":"b1","type":"shell","status":"running","description":"Run the dev server"}]}"#;

    hook(home.path(), Some(&id), prompt);
    assert!(hook(home.path(), Some(&id), stop).status.success());

    assert!(!busy.exists(), "idle after its Stop");
    let log = fs::read_to_string(home.path().join(format!("flows/{id}.jsonl"))).unwrap();
    assert!(log.contains(r#""kind":"answer""#), "{log}");
    assert!(!log.contains("delegated"), "{log}");
    let detail = stdout(&saaio(home.path(), &["status", "shop-221", "--json"]));
    assert!(detail.contains(r#""state": "waiting""#), "{detail}");
    assert!(
        detail.contains("Which color do you prefer, red or blue?"),
        "{detail}"
    );
}

#[test]
fn a_codex_master_records_its_session_and_reports_nothing() {
    let home = TempDir::new().unwrap();
    let id = new_flow(home.path(), "shop-221");
    let prompt = r#"{"session_id":"02b24000-0000-7000-8000-000000000002","hook_event_name":"UserPromptSubmit","prompt":"status?"}"#;

    let mut command = saaio_command(home.path());
    command
        .args(["hook", "codex"])
        .env("SAAIO_MASTER", "1")
        .env("SAAIO_PROOF", key(home.path(), "master"))
        .env("SAAIO_FLOW_ID", &id);
    let output = feed(command, prompt);

    assert!(output.status.success());
    assert!(output.stdout.is_empty() && output.stderr.is_empty());
    assert_eq!(
        fs::read_to_string(home.path().join("waits/master.session")).unwrap(),
        "02b24000-0000-7000-8000-000000000002"
    );
    assert!(home.path().join("waits/master.busy").exists());
    let turn = fs::read_to_string(home.path().join("waits/master.turn")).unwrap();
    assert!(turn.contains(r#""state":"working""#), "{turn}");
    let log = fs::read_to_string(home.path().join(format!("flows/{id}.jsonl"))).unwrap();
    assert!(!log.contains("02b2"), "{log}");
}

/// Runs `saaio hook claude-code` as the master's pane does.
fn master_hook(home: &Path, payload: &str) -> Output {
    let mut command = hook_command(home, &["claude-code"], None);
    command
        .env("SAAIO_MASTER", "1")
        .env("SAAIO_PROOF", key(home, "master"));
    feed(command, payload)
}

#[test]
fn a_claude_code_master_keeps_where_its_turn_stands() {
    let home = TempDir::new().unwrap();
    let turn = home.path().join("waits/master.turn");
    let prompt = r#"{"hook_event_name":"UserPromptSubmit","prompt":"status?"}"#;
    let write = r#"{"hook_event_name":"PreToolUse","tool_name":"Write","tool_input":{"file_path":"x.txt"}}"#;
    let stop = r#"{"hook_event_name":"Stop","last_assistant_message":"All green."}"#;

    let started = master_hook(home.path(), prompt);
    assert!(started.status.success());
    assert!(started.stdout.is_empty() && started.stderr.is_empty());
    let working = fs::read_to_string(&turn).unwrap();
    assert!(working.contains(r#""state":"working""#), "{working}");

    master_hook(home.path(), write);
    assert_eq!(
        fs::read_to_string(&turn).unwrap(),
        working,
        "a tool call while working leaves the file as it was"
    );

    master_hook(home.path(), stop);
    let idle = fs::read_to_string(&turn).unwrap();
    assert!(idle.contains(r#""state":"idle""#), "{idle}");
    assert!(
        !home.path().join("flows").exists()
            || fs::read_dir(home.path().join("flows")).unwrap().count() == 0,
        "the master reports into no flow"
    );
}

#[test]
fn a_claude_code_master_turn_ends_beside_a_background_task() {
    let home = TempDir::new().unwrap();
    let turn = home.path().join("waits/master.turn");
    let stop = r#"{"hook_event_name":"Stop","last_assistant_message":"Watching the workers.","background_tasks":[{"id":"b1","type":"subagent","status":"running","description":"background sleeper"}]}"#;

    master_hook(home.path(), stop);

    let idle = fs::read_to_string(&turn).unwrap();
    assert!(idle.contains(r#""state":"idle""#), "{idle}");
}

#[test]
fn a_woken_master_turn_that_ends_idle_again_is_a_new_turn() {
    let home = TempDir::new().unwrap();
    let turn = home.path().join("waits/master.turn");
    fs::create_dir_all(home.path().join("waits")).unwrap();
    fs::write(&turn, r#"{"state":"idle","at":1}"#).unwrap();
    let stop = r#"{"hook_event_name":"Stop","last_assistant_message":"shop-221 is blocked."}"#;

    master_hook(home.path(), stop);

    let kept = fs::read_to_string(&turn).unwrap();
    assert!(kept.contains(r#""state":"idle""#), "{kept}");
    assert!(
        !kept.contains(r#""at":1}"#),
        "a turn woken by saaio wait runs no prompt hook, so its end must still count: {kept}"
    );
}

#[test]
fn a_merge_request_counts_only_for_the_flows_own_repo() {
    let home = TempDir::new().unwrap();
    let id = new_flow(home.path(), "shop-221");
    let repo = home.path().join("shop");
    fs::create_dir_all(&repo).unwrap();
    let git = |args: &[&str]| {
        let status = Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(args)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?}");
    };
    git(&["init", "-q"]);
    git(&["remote", "add", "origin", "git@github.com:shop/app.git"]);
    let workspace = format!(
        "{{\"v\":1,\"at\":2,\"kind\":\"workspace\",\"path\":\"{}\",\"branch\":\"flow/shop-221\",\"slot\":1,\"window\":null,\"agent\":\"claude\"}}\n",
        repo.display()
    );
    let mut log = fs::OpenOptions::new()
        .append(true)
        .open(home.path().join(format!("flows/{id}.jsonl")))
        .unwrap();
    log.write_all(workspace.as_bytes()).unwrap();
    let created = |url: &str| {
        format!(
            r#"{{"hook_event_name":"PostToolUse","tool_name":"Bash","tool_input":{{"command":"gh pr create --fill"}},"tool_response":{{"stdout":"{url}\n"}}}}"#
        )
    };

    let probe = hook(
        home.path(),
        Some(&id),
        &created("https://github.com/probe/repo/pull/7"),
    );
    assert!(probe.status.success());
    let status = stdout(&saaio(home.path(), &["status", "shop-221"]));
    assert!(!status.contains("probe/repo"), "today's case: {status}");
    assert!(!status.contains("done"), "{status}");

    hook(
        home.path(),
        Some(&id),
        &created("https://github.com/shop/app/pull/12"),
    );
    let status = stdout(&saaio(home.path(), &["status", "shop-221"]));
    assert!(
        status.contains("https://github.com/shop/app/pull/12"),
        "{status}"
    );
}

#[test]
fn a_merge_request_of_a_forks_upstream_counts() {
    let home = TempDir::new().unwrap();
    let id = new_flow(home.path(), "shop-221");
    let repo = home.path().join("fork");
    fs::create_dir_all(&repo).unwrap();
    for args in [
        &["init", "-q"][..],
        &["remote", "add", "origin", "git@github.com:jan/app.git"],
        &[
            "remote",
            "add",
            "upstream",
            "https://github.com/shop/app.git",
        ],
    ] {
        let status = Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(args)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?}");
    }
    let workspace = format!(
        "{{\"v\":1,\"at\":2,\"kind\":\"workspace\",\"path\":\"{}\",\"branch\":\"flow/shop-221\",\"slot\":1,\"window\":null,\"agent\":\"claude\"}}\n",
        repo.display()
    );
    let mut log = fs::OpenOptions::new()
        .append(true)
        .open(home.path().join(format!("flows/{id}.jsonl")))
        .unwrap();
    log.write_all(workspace.as_bytes()).unwrap();

    hook(
        home.path(),
        Some(&id),
        r#"{"hook_event_name":"PostToolUse","tool_name":"Bash","tool_input":{"command":"gh pr create --fill"},"tool_response":{"stdout":"https://github.com/shop/app/pull/12\n"}}"#,
    );

    let status = stdout(&saaio(home.path(), &["status", "shop-221"]));
    assert!(
        status.contains("https://github.com/shop/app/pull/12"),
        "gh pr create in a fork opens it upstream: {status}"
    );
}

#[test]
fn a_merge_request_counts_through_an_ssh_host_alias() {
    let home = TempDir::new().unwrap();
    let id = new_flow(home.path(), "shop-221");
    let repo = home.path().join("work");
    fs::create_dir_all(&repo).unwrap();
    for args in [
        &["init", "-q"][..],
        &["remote", "add", "origin", "git@github-work:shop/app.git"],
    ] {
        let status = Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(args)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?}");
    }
    let workspace = format!(
        "{{\"v\":1,\"at\":2,\"kind\":\"workspace\",\"path\":\"{}\",\"branch\":\"flow/shop-221\",\"slot\":1,\"window\":null,\"agent\":\"claude\"}}\n",
        repo.display()
    );
    let mut log = fs::OpenOptions::new()
        .append(true)
        .open(home.path().join(format!("flows/{id}.jsonl")))
        .unwrap();
    log.write_all(workspace.as_bytes()).unwrap();
    let bin = home.path().join("bin");
    fs::create_dir_all(&bin).unwrap();
    executable(
        &bin.join("ssh"),
        "#!/bin/sh\nfor host; do :; done\nif [ \"$host\" = github-work ]; then echo 'hostname github.com'; else echo \"hostname $host\"; fi\n",
    );
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap());

    let mut command = hook_command(home.path(), &["claude-code"], Some(&id));
    command.env("PATH", path);
    let output = feed(
        command,
        r#"{"hook_event_name":"PostToolUse","tool_name":"Bash","tool_input":{"command":"gh pr create --fill"},"tool_response":{"stdout":"https://github.com/shop/app/pull/12\n"}}"#,
    );
    assert!(output.status.success());

    let status = stdout(&saaio(home.path(), &["status", "shop-221"]));
    assert!(
        status.contains("https://github.com/shop/app/pull/12"),
        "origin git@github-work: is github.com: {status}"
    );
}

#[test]
fn a_hook_keeps_reporting_when_the_config_does_not_load() {
    let home = TempDir::new().unwrap();
    let id = new_flow(home.path(), "shop-221");
    fs::write(
        home.path().join("no-config.toml"),
        "[agents.aider]\ncommand = [\"aider\"]\nprompt = \"\"\nadapter = \"claud-code\"\n",
    )
    .unwrap();
    let skill = r#"{"hook_event_name":"PreToolUse","tool_name":"Skill","tool_input":{"skill":"design-code"}}"#;

    let output = hook(home.path(), Some(&id), skill);

    assert!(output.status.success());
    assert!(output.stdout.is_empty(), "{}", stdout(&output));
    assert!(output.stderr.is_empty(), "{}", stderr(&output));
    let detail = stdout(&saaio(home.path(), &["status", "shop-221"]));
    assert!(detail.contains("phase: plan"), "{detail}");
}

#[test]
fn a_hook_never_fails_or_prints_whatever_it_gets() {
    let home = TempDir::new().unwrap();
    let id = new_flow(home.path(), "shop-221");
    let stop = r#"{"hook_event_name":"Stop","last_assistant_message":"Done?"}"#;

    let cases = [
        hook(home.path(), None, stop),
        hook(home.path(), Some(&id), "not json"),
        hook(home.path(), Some(MISSING), stop),
        hook(
            home.path(),
            Some(&id),
            r#"{"hook_event_name":"Notification"}"#,
        ),
    ];

    for output in &cases {
        assert!(output.status.success());
        assert!(output.stdout.is_empty(), "{}", stdout(output));
        assert!(output.stderr.is_empty(), "{}", stderr(output));
    }
    let log = fs::read_to_string(home.path().join("server.log")).unwrap();
    assert!(
        log.contains(&format!(
            "] hook claude-code flow {id}: the event is not JSON: "
        )),
        "{log}"
    );
    assert!(
        log.contains(&format!(
            "] hook claude-code flow {MISSING}: no flow {MISSING}\n"
        )),
        "{log}"
    );
    let list = stdout(&saaio(home.path(), &["status"]));
    assert!(!list.contains("waiting"), "{list}");
}

#[test]
fn a_hook_skips_an_event_too_large_binary_or_too_deep_and_says_why() {
    let home = TempDir::new().unwrap();
    let id = new_flow(home.path(), "shop-221");
    let before = stdout(&saaio(home.path(), &["status"]));
    let huge = format!(
        r#"{{"hook_event_name":"Stop","last_assistant_message":"{}"}}"#,
        "x".repeat(17 << 20)
    );
    let deep = format!("{}{}", "[".repeat(10_000), "]".repeat(10_000));
    let binary: Vec<u8> = (0..=255).cycle().take(4096).collect();

    let cases = [
        feed(hook_command(home.path(), &["claude-code"], Some(&id)), huge),
        feed(
            hook_command(home.path(), &["claude-code"], Some(&id)),
            binary,
        ),
        hook(home.path(), Some(&id), &deep),
        hook(home.path(), Some(&id), r#"{"hook_event_name":"#),
    ];

    for output in &cases {
        assert!(output.status.success(), "{:?}", output.status);
        assert!(output.stdout.is_empty(), "{}", stdout(output));
        assert!(output.stderr.is_empty(), "{}", stderr(output));
    }
    let log = fs::read_to_string(home.path().join("server.log")).unwrap();
    let flow = format!("] hook claude-code flow {id}: ");
    for why in [
        "the event is over 16 MiB",
        "the event is not UTF-8",
        "the event is not JSON: recursion limit exceeded",
        "the event is not JSON: EOF",
    ] {
        assert!(log.contains(&format!("{flow}{why}")), "{why} in\n{log}");
    }
    assert_eq!(stdout(&saaio(home.path(), &["status"])), before);
}

const MISSING: &str = "0b5e4000-0000-4000-8000-000000000000";

#[test]
fn a_flow_id_that_is_no_uuid_never_becomes_a_path() {
    let outer = TempDir::new().unwrap();
    let home = outer.path().join("store");
    new_flow(&home, "shop-221");
    let forged = "../../escaped";
    let prompt = r#"{"hook_event_name":"UserPromptSubmit","prompt":"go"}"#;
    let stop = r#"{"hook_event_name":"Stop","last_assistant_message":"Done?"}"#;

    for adapter in ["claude-code", "codex"] {
        for payload in [prompt, stop] {
            let mut command = hook_command(&home, &[adapter], None);
            command.env("SAAIO_FLOW_ID", forged);
            let output = feed(command, payload);
            assert!(output.status.success());
            assert!(output.stdout.is_empty() && output.stderr.is_empty());
        }
    }
    let wait = saaio_command(&home)
        .arg("wait")
        .env("SAAIO_FLOW_ID", forged)
        .output()
        .unwrap();

    assert!(wait.status.success());
    assert!(wait.stdout.is_empty() && wait.stderr.is_empty());
    let outside: Vec<_> = fs::read_dir(outer.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(outside, ["store"]);
    let log = fs::read_to_string(home.join("server.log")).unwrap();
    assert!(
        log.contains(r#"] hook codex: SAAIO_FLOW_ID "../../escaped" is no flow id"#),
        "{log}"
    );
}
