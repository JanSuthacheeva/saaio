//! Tests of `saaio new` and `saaio report`: flows, workflow phases, heartbeats, clear

use std::fs;

use tempfile::TempDir;

use crate::support::*;

#[test]
fn new_creates_a_flow_once_and_returns_it_again_while_unfinished() {
    let home = TempDir::new().unwrap();

    let id = new_flow(home.path(), "shop-221");
    let again = saaio(
        home.path(),
        &["new", "--name", "shop-221", "--request", "other"],
    );

    let text = stdout(&again);
    assert_eq!(id_in(&text), id, "{text}");
    assert!(text.contains("existing: true"), "{text}");
    assert!(stdout(&saaio(home.path(), &["status"])).contains("count: 1"));
}

#[test]
fn report_appends_events_to_the_flow_in_flow_id() {
    let home = TempDir::new().unwrap();
    let id = new_flow(home.path(), "shop-221");

    let phase = saaio_in(home.path(), &id, &["report", "phase", "implement"]);
    saaio_in(home.path(), &id, &["report", "tasks", "4", "9"]);
    saaio_in(
        home.path(),
        &id,
        &["report", "review", "--fixed", "3", "--open", "1"],
    );
    let blocked = saaio_in(home.path(), &id, &["report", "blocked", "no default"]);

    assert_eq!(
        stdout(&phase),
        "flow: shop-221\nstate: working\nphase: implement\nhelp[1]: Run `saaio status shop-221` for its phases and timeline\n"
    );
    assert!(stdout(&blocked).contains("state: blocked"));
    let board: serde_json::Value =
        serde_json::from_slice(&saaio(home.path(), &["status", "--json"]).stdout).unwrap();
    let shown = &board["flows"][0];
    assert_eq!(shown["blocker"], "no default");
    assert_eq!(shown["tasks"]["done"], 4);
    assert_eq!(shown["review"]["open"], 1);
}

#[test]
fn a_flow_walks_the_configured_workflow_and_refuses_another_phase() {
    let home = TempDir::new().unwrap();
    fs::write(
        home.path().join("no-config.toml"),
        "[[phases]]\nname = \"spec\"\n\n[[phases]]\nname = \"build\"\n\n[[phases]]\nname = \"ship\"\n",
    )
    .unwrap();

    let created = saaio(
        home.path(),
        &["new", "--name", "shop-221", "--request", "add CSV export"],
    );
    let id = id_in(&stdout(&created));
    let build = saaio_in(home.path(), &id, &["report", "phase", "build"]);
    let review = saaio_in(home.path(), &id, &["report", "phase", "review"]);
    let capital = saaio_in(home.path(), &id, &["report", "phase", "Build"]);

    assert!(
        stdout(&created).contains("Run `saaio report --flow shop-221 phase spec` when spec starts"),
        "{}",
        stdout(&created)
    );
    assert!(
        stdout(&build).contains("phase: build"),
        "{}",
        stdout(&build)
    );
    assert_eq!(review.status.code(), Some(1));
    assert!(
        stdout(&review).starts_with("error: shop-221 has no phase review"),
        "{}",
        stdout(&review)
    );
    assert!(
        stdout(&review).contains("Phases: spec, build, ship"),
        "{}",
        stdout(&review)
    );
    assert_eq!(capital.status.code(), Some(2));
    let board: serde_json::Value =
        serde_json::from_slice(&saaio(home.path(), &["status", "--json"]).stdout).unwrap();
    let steps: Vec<&str> = board["flows"][0]["steps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|step| step["phase"].as_str().unwrap())
        .collect();
    assert_eq!(steps, ["spec", "build", "ship"]);
    assert_eq!(board["flows"][0]["phase"], "build");
}

#[test]
fn a_plain_session_refuses_a_phase_and_says_it_has_none() {
    let home = TempDir::new().unwrap();
    fs::create_dir_all(home.path().join("flows")).unwrap();
    fs::write(
        home.path().join("flows/scratch-id.jsonl"),
        "{\"v\":1,\"at\":1,\"kind\":\"started\",\"name\":\"scratch\",\"request\":\"\",\"mode\":\"plain\"}\n",
    )
    .unwrap();

    let report = saaio(
        home.path(),
        &["report", "--flow", "scratch", "phase", "plan"],
    );

    assert_eq!(report.status.code(), Some(1));
    assert!(
        stdout(&report).starts_with("error: scratch has no phase plan"),
        "{}",
        stdout(&report)
    );
    assert!(
        stdout(&report).contains("scratch is a plain session, which has no phases"),
        "{}",
        stdout(&report)
    );
}

#[test]
fn report_takes_a_flow_name_from_the_flag() {
    let home = TempDir::new().unwrap();
    new_flow(home.path(), "shop-221");

    let output = saaio(
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

    let output = saaio(home.path(), &["report", "working"]);

    assert!(output.status.success());
    assert!(stdout(&output).starts_with("skipped: no flow id"));
}

#[test]
fn report_to_a_flow_that_does_not_exist_fails_with_exit_1() {
    let home = TempDir::new().unwrap();

    let output = saaio(home.path(), &["report", "--flow", "shop-999", "working"]);

    assert_eq!(output.status.code(), Some(1));
    assert!(stdout(&output).starts_with("error: no flow named shop-999"));
}

#[test]
fn a_heartbeat_marks_the_flow_alive_without_an_event() {
    let home = TempDir::new().unwrap();
    let id = new_flow(home.path(), "shop-221");

    let output = saaio_in(home.path(), &id, &["report", "heartbeat"]);

    assert!(output.status.success());
    assert!(home.path().join(format!("flows/{id}.alive")).exists());
    let log = fs::read_to_string(home.path().join(format!("flows/{id}.jsonl"))).unwrap();
    assert_eq!(log.lines().count(), 1);
}

#[test]
fn report_takes_only_a_merge_request_of_the_flows_repository() {
    let lab = Lab::new();
    git(
        &lab.repo,
        &["remote", "add", "origin", "git@github.com:acme/shop.git"],
    );
    let started = lab.start(&["SHOP-1 export"]);
    assert!(started.status.success(), "{}", stdout(&started));
    let id = id_in(&stdout(&started));
    let report = |url: &str| saaio_in(&lab.home, &id, &["report", "merge-request", "--", url]);

    for url in [
        "--web",
        "https://example.com/acme/shop/pull/7",
        "https://github.com/other/shop/pull/7",
    ] {
        let refused = report(url);
        assert_eq!(refused.status.code(), Some(1), "{url}");
        let said = stdout(&refused);
        assert!(
            said.contains(&format!(
                "error: \"{url} is not a merge request of shop-1's repository\""
            )),
            "{said}"
        );
    }
    let opened = report("https://github.com/acme/shop/pull/7");

    assert!(opened.status.success(), "{}", stdout(&opened));
    let board: serde_json::Value =
        serde_json::from_slice(&lab.run(&["status", "--json"]).stdout).unwrap();
    assert_eq!(
        board["flows"][0]["mergeRequest"],
        "https://github.com/acme/shop/pull/7"
    );
}

#[test]
fn a_flow_without_a_worktree_takes_a_merge_request_of_the_folder_it_reports_from() {
    let lab = Lab::new();
    git(
        &lab.repo,
        &[
            "remote",
            "add",
            "origin",
            "https://gitlab.com/acme/shop.git",
        ],
    );
    let id = new_flow(&lab.home, "shop-221");
    let url = "https://gitlab.com/acme/shop/-/merge_requests/3";

    let elsewhere = saaio_in(&lab.home, &id, &["report", "merge-request", url]);
    let in_repo = lab.run_as(&id, &["report", "merge-request", url]);

    assert_eq!(elsewhere.status.code(), Some(1), "{}", stdout(&elsewhere));
    assert!(in_repo.status.success(), "{}", stdout(&in_repo));
}

#[test]
fn clear_forgets_finished_flows_only() {
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
    let home = &lab.home;
    let done = new_flow(home, "export-csv");
    new_flow(home, "shop-221");
    lab.run_as(
        &done,
        &[
            "report",
            "merge-request",
            "https://github.com/acme/shop/pull/88",
        ],
    );
    let still_live = saaio(home, &["clear"]);
    assert!(stdout(&still_live).starts_with("cleared: 0"));
    saaio_in(home, &done, &["report", "ended"]);

    let cleared = saaio(home, &["clear"]);
    let again = saaio(home, &["clear"]);

    assert_eq!(
        stdout(&cleared),
        "cleared: 1\nnames[1]: export-csv\nhelp[1]: Run `saaio` to see what is left\n"
    );
    assert!(stdout(&again).starts_with("cleared: 0"));
    let list = stdout(&saaio(home, &["status"]));
    assert!(
        list.contains("count: 1") && list.contains("shop-221"),
        "{list}"
    );
}
