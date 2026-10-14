use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use tempfile::TempDir;

fn flow(home: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_flow"))
        .args(args)
        .env("FLOW_HOME", home)
        .env_remove("FLOW_ID")
        .output()
        .expect("the flow binary runs")
}

fn flow_in(home: &Path, flow_id: &str, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_flow"))
        .args(args)
        .env("FLOW_HOME", home)
        .env("FLOW_ID", flow_id)
        .output()
        .expect("the flow binary runs")
}

fn new_flow(home: &Path, name: &str) -> String {
    let output = flow(
        home,
        &[
            "new",
            "--name",
            name,
            "--request",
            "SHOP-221 add CSV export",
            "--mode",
            "auto",
        ],
    );
    assert!(output.status.success(), "{}", stdout(&output));
    stdout(&output)
        .lines()
        .find_map(|line| line.strip_prefix("id: "))
        .expect("new prints the id")
        .to_string()
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).unwrap()
}

fn seed(home: &Path, id: &str, lines: &[&str]) {
    let flows = home.join("flows");
    fs::create_dir_all(&flows).unwrap();
    fs::write(flows.join(format!("{id}.jsonl")), lines.join("\n") + "\n").unwrap();
}

fn seed_blocked(home: &Path) {
    seed(
        home,
        "7f3c",
        &[
            r#"{"v":1,"at":1000,"kind":"started","name":"shop-221","request":"SHOP-221 add CSV export","mode":"auto"}"#,
            r#"{"v":1,"at":2000,"kind":"phase","phase":"implement"}"#,
            r#"{"v":1,"at":3000,"kind":"blocked","reason":"no default"}"#,
        ],
    );
}

#[test]
fn the_home_view_shows_the_binary_and_an_empty_state() {
    let home = TempDir::new().unwrap();

    let output = flow(home.path(), &[]);

    assert!(output.status.success());
    let text = stdout(&output);
    assert!(text.starts_with("bin: "), "{text}");
    assert!(text.contains("description: "), "{text}");
    assert!(text.contains("flows: none tracked"), "{text}");
}

#[test]
fn status_lists_flows_and_shows_one_in_detail() {
    let home = TempDir::new().unwrap();
    seed_blocked(home.path());

    let list = stdout(&flow(home.path(), &["status"]));
    assert!(list.contains("shop-221,blocked,implement,"), "{list}");
    assert!(
        list.contains("blockers[1]{name,blocker}:\n  shop-221,no default"),
        "{list}"
    );

    let detail = stdout(&flow(home.path(), &["status", "shop-221"]));
    assert!(detail.starts_with("name: shop-221\nid: 7f3c\n"), "{detail}");
}

#[test]
fn status_json_is_the_versioned_board() {
    let home = TempDir::new().unwrap();
    seed_blocked(home.path());

    let output = flow(home.path(), &["status", "--json"]);

    assert!(output.status.success());
    let board: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(board["version"], 1);
    assert_eq!(board["needsYou"], 1);
    assert_eq!(board["flows"][0]["name"], "shop-221");
    assert_eq!(board["flows"][0]["blocker"], "no default");
}

#[test]
fn an_unknown_flag_fails_loudly_on_stdout_with_exit_2() {
    let home = TempDir::new().unwrap();

    let output = flow(home.path(), &["status", "--nope"]);

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stderr.is_empty());
    let text = stdout(&output);
    assert!(
        text.starts_with("error: unexpected argument '--nope' found"),
        "{text}"
    );
    assert!(
        text.contains("Run `flow status --help` for the usage"),
        "{text}"
    );
}

#[test]
fn a_missing_flow_fails_with_exit_1_and_next_steps() {
    let home = TempDir::new().unwrap();

    let output = flow(home.path(), &["status", "shop-999"]);

    assert_eq!(output.status.code(), Some(1));
    let text = stdout(&output);
    assert!(text.starts_with("error: no flow named shop-999"), "{text}");
    assert!(text.contains("Run `flow` to list the flows"), "{text}");
}

#[test]
fn help_and_version_still_print_normally() {
    let home = TempDir::new().unwrap();

    let help = flow(home.path(), &["--help"]);
    let version = flow(home.path(), &["--version"]);

    assert!(help.status.success());
    assert!(stdout(&help).contains("Usage: flow"));
    assert_eq!(stdout(&version).trim(), "flow 0.1.0");
}

#[test]
fn new_creates_a_flow_once_and_returns_it_again_while_unfinished() {
    let home = TempDir::new().unwrap();

    let id = new_flow(home.path(), "shop-221");
    let again = flow(
        home.path(),
        &["new", "--name", "shop-221", "--request", "other"],
    );

    let text = stdout(&again);
    assert!(text.contains(&format!("id: {id}")), "{text}");
    assert!(text.contains("existing: true"), "{text}");
    assert!(stdout(&flow(home.path(), &["status"])).contains("count: 1"));
}

#[test]
fn report_appends_events_to_the_flow_in_flow_id() {
    let home = TempDir::new().unwrap();
    let id = new_flow(home.path(), "shop-221");

    let phase = flow_in(home.path(), &id, &["report", "phase", "implement"]);
    flow_in(home.path(), &id, &["report", "tasks", "4", "9"]);
    flow_in(
        home.path(),
        &id,
        &["report", "review", "--fixed", "3", "--open", "1"],
    );
    let blocked = flow_in(home.path(), &id, &["report", "blocked", "no default"]);

    assert_eq!(
        stdout(&phase),
        "flow: shop-221\nstate: working\nphase: implement\nhelp[1]: Run `flow status shop-221` for its phases and timeline\n"
    );
    assert!(stdout(&blocked).contains("state: blocked"));
    let board: serde_json::Value =
        serde_json::from_slice(&flow(home.path(), &["status", "--json"]).stdout).unwrap();
    let shown = &board["flows"][0];
    assert_eq!(shown["blocker"], "no default");
    assert_eq!(shown["tasks"]["done"], 4);
    assert_eq!(shown["review"]["open"], 1);
}

#[test]
fn report_takes_a_flow_name_from_the_flag() {
    let home = TempDir::new().unwrap();
    new_flow(home.path(), "shop-221");

    let output = flow(
        home.path(),
        &[
            "report",
            "--flow",
            "shop-221",
            "waiting",
            "Approve the plan?",
        ],
    );

    assert!(output.status.success());
    assert!(stdout(&output).contains("state: waiting"));
}

#[test]
fn report_without_a_flow_id_is_skipped_not_failed() {
    let home = TempDir::new().unwrap();

    let output = flow(home.path(), &["report", "working"]);

    assert!(output.status.success());
    assert!(stdout(&output).starts_with("skipped: no flow id"));
}

#[test]
fn report_to_a_flow_that_does_not_exist_fails_with_exit_1() {
    let home = TempDir::new().unwrap();

    let output = flow(home.path(), &["report", "--flow", "shop-999", "working"]);

    assert_eq!(output.status.code(), Some(1));
    assert!(stdout(&output).starts_with("error: no flow named shop-999"));
}

#[test]
fn a_heartbeat_marks_the_flow_alive_without_an_event() {
    let home = TempDir::new().unwrap();
    let id = new_flow(home.path(), "shop-221");

    let output = flow_in(home.path(), &id, &["report", "heartbeat"]);

    assert!(output.status.success());
    assert!(home.path().join(format!("flows/{id}.alive")).exists());
    let log = fs::read_to_string(home.path().join(format!("flows/{id}.jsonl"))).unwrap();
    assert_eq!(log.lines().count(), 1);
}

#[test]
fn clear_forgets_finished_flows_only() {
    let home = TempDir::new().unwrap();
    let done = new_flow(home.path(), "export-csv");
    new_flow(home.path(), "shop-221");
    flow_in(
        home.path(),
        &done,
        &["report", "merge-request", "https://x/88"],
    );

    let cleared = flow(home.path(), &["clear"]);
    let again = flow(home.path(), &["clear"]);

    assert_eq!(
        stdout(&cleared),
        "cleared: 1\nnames[1]: export-csv\nhelp[1]: Run `flow` to see what is left\n"
    );
    assert!(stdout(&again).starts_with("cleared: 0"));
    let list = stdout(&flow(home.path(), &["status"]));
    assert!(
        list.contains("count: 1") && list.contains("shop-221"),
        "{list}"
    );
}
