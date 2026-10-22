//! Tests of `saaio send`: messages into a flow's log, `--type` lines, refusals, dialogs

use std::fs;

use tempfile::TempDir;

use crate::support::*;

#[test]
fn sends_after_pr_until_worker_ends() {
    let home = TempDir::new().unwrap();
    let repo = home.path().join("repo");
    fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-q"]);
    git(
        &repo,
        &[
            "remote",
            "add",
            "origin",
            "https://github.com/probe/repo.git",
        ],
    );
    let workspace = format!(
        r#"{{"v":1,"at":1500,"kind":"workspace","path":"{}","branch":"flow/slot-reclaim","slot":1,"window":"saaio-ui","agent":"codex"}}"#,
        repo.display()
    );
    seed(
        home.path(),
        "7f3c0000-0000-4000-8000-000000000000",
        &[
            r#"{"v":1,"at":1000,"kind":"started","name":"slot-reclaim","request":"fix a race","mode":"auto"}"#,
            &workspace,
        ],
    );
    for payload in [
        include_str!("../../src/adapter/fixtures/codex/post_bash_merge_request.json"),
        include_str!("../../src/adapter/fixtures/codex/stop_question.json"),
    ] {
        let output = hook_with(
            home.path(),
            &["codex"],
            Some("7f3c0000-0000-4000-8000-000000000000"),
            payload,
        );
        assert!(output.status.success(), "{}", stderr(&output));
    }
    let status: serde_json::Value =
        serde_json::from_slice(&saaio(home.path(), &["status", "--json"]).stdout).unwrap();
    let flow = &status["flows"][0];
    assert_eq!(flow["state"], "waiting");
    assert_eq!(flow["mergeRequest"], "https://github.com/probe/repo/pull/7");

    let sent = as_master(
        home.path(),
        &["send", "slot-reclaim", "Fix the race first."],
    );
    let typed = as_master(home.path(), &["send", "--type", "slot-reclaim", "/review"]);
    assert!(sent.status.success(), "{}", stdout(&sent));
    assert!(typed.status.success(), "{}", stdout(&typed));
    let log = fs::read_to_string(
        home.path()
            .join("flows/7f3c0000-0000-4000-8000-000000000000.jsonl"),
    )
    .unwrap();
    assert!(log.contains(r#""kind":"message","text":"Fix the race first.""#));
    assert!(log.contains(r#""kind":"typed","text":"/review""#));

    let ended = saaio_in(
        home.path(),
        "7f3c0000-0000-4000-8000-000000000000",
        &["report", "ended"],
    );
    assert!(ended.status.success(), "{}", stderr(&ended));
    for args in [
        vec!["send", "slot-reclaim", "late"],
        vec!["send", "--type", "slot-reclaim", "/review"],
    ] {
        let output = as_master(home.path(), &args);
        assert_eq!(output.status.code(), Some(1));
        assert!(stdout(&output).contains("slot-reclaim is finished"));
    }
    fs::remove_file(
        home.path()
            .join("flows/7f3c0000-0000-4000-8000-000000000000.jsonl"),
    )
    .unwrap();
    let removed = as_master(
        home.path(),
        &["send", "7f3c0000-0000-4000-8000-000000000000", "late"],
    );
    assert_eq!(removed.status.code(), Some(1));
    assert!(stdout(&removed).contains("no flow named 7f3c0000-0000-4000-8000-000000000000"));
}

#[test]
fn send_puts_the_masters_message_into_the_flows_log() {
    let home = TempDir::new().unwrap();
    seed_worker(
        home.path(),
        "7f3c0000-0000-4000-8000-000000000000",
        "shop-217",
        r#"{"v":1,"at":2000,"kind":"waiting","question":"Which delimiter?"}"#,
    );

    let output = as_master(home.path(), &["send", "shop-217", "Semicolons."]);

    assert!(output.status.success(), "{}", stdout(&output));
    assert!(stdout(&output).starts_with("sent: shop-217\nstate: waiting\n"));
    let log = fs::read_to_string(
        home.path()
            .join("flows/7f3c0000-0000-4000-8000-000000000000.jsonl"),
    )
    .unwrap();
    assert!(
        log.lines()
            .last()
            .unwrap()
            .contains(r#""kind":"message","text":"Semicolons.""#),
        "{log}"
    );
    let detail = stdout(&saaio(home.path(), &["status", "shop-217"]));
    assert!(detail.contains("master: Semicolons."), "{detail}");
}

const DIALOG: &str = r#"{"v":1,"at":2000,"kind":"waiting","question":"Which color?","questions":[{"header":"Color","question":"Which color?","options":[{"label":"Red","description":"A warm, bold color"},{"label":"Blue","description":"A cool, steady color"}],"multiSelect":false},{"header":"Toppings","question":"Which toppings?","options":[{"label":"Cheese","description":"Melted cheese"},{"label":"Olives","description":"Black olives"}],"multiSelect":true}]}"#;

#[test]
fn status_lists_the_questions_and_options_of_a_dialog() {
    let home = TempDir::new().unwrap();
    seed_worker(
        home.path(),
        "7f3c0000-0000-4000-8000-000000000000",
        "ask",
        DIALOG,
    );

    let detail = stdout(&saaio(home.path(), &["status", "ask", "--full"]));

    assert!(
        detail.contains(
            "questions[2]{n,header,question,pick}:\n  1,Color,Which color?,one\n  2,Toppings,Which toppings?,several\noptions[4]{question,n,label,description}:\n  1,1,Red,\"A warm, bold color\"\n  1,2,Blue,\"A cool, steady color\"\n  2,1,Cheese,Melted cheese\n  2,2,Olives,Black olives\n"
        ),
        "{detail}"
    );
    let json = stdout(&saaio(home.path(), &["status", "ask", "--json"]));
    assert!(json.contains(r#""multiSelect": true"#), "{json}");
}

#[test]
fn send_to_a_flow_on_a_dialog_says_saaio_closes_it() {
    let home = TempDir::new().unwrap();
    seed_worker(
        home.path(),
        "7f3c0000-0000-4000-8000-000000000000",
        "ask",
        DIALOG,
    );

    let output = as_master(
        home.path(),
        &["send", "ask", "The user says: Red; Cheese and Olives"],
    );

    assert!(output.status.success(), "{}", stdout(&output));
    assert!(
        stdout(&output).contains("saaio closes ask's question dialog"),
        "{}",
        stdout(&output)
    );
}

#[test]
fn send_refuses_what_has_no_worker_to_read_it() {
    let home = TempDir::new().unwrap();
    seed_worker(
        home.path(),
        "aaaa",
        "shipped",
        r#"{"v":1,"at":2000,"kind":"ended"}"#,
    );
    seed(
        home.path(),
        "bbbb",
        &[r#"{"v":1,"at":1000,"kind":"started","name":"tracked","request":"x","mode":"auto"}"#],
    );
    seed_worker(
        home.path(),
        "cccc",
        "live",
        r#"{"v":1,"at":2000,"kind":"working"}"#,
    );

    let refused = |args: &[&str], from: Option<&str>| {
        let output = match from {
            Some(id) => saaio_in(home.path(), id, args),
            None => as_master(home.path(), args),
        };
        assert_eq!(
            output.status.code(),
            Some(1),
            "{args:?}: {}",
            stdout(&output)
        );
        stdout(&output)
    };

    assert!(refused(&["send", "nobody", "hi"], None).contains("no flow named nobody"));
    assert!(refused(&["send", "shipped", "hi"], None).contains("shipped is finished"));
    assert!(refused(&["send", "tracked", "hi"], None).contains("tracked has no worker"));
    assert!(refused(&["send", "live", "  "], None).contains("nothing to send"));
    let long = "x".repeat(4001);
    assert!(refused(&["send", "live", &long], None).contains("over 4000"));
    assert!(refused(&["send", "live", "hi"], Some("cccc")).contains("only the master sends"));
    seed(
        home.path(),
        "dddd",
        &[
            r#"{"v":1,"at":1000,"kind":"started","name":"scratch","request":"","mode":"plain"}"#,
            r#"{"v":1,"at":1500,"kind":"workspace","path":"/tmp","branch":null,"slot":null,"window":"saaio-ui","agent":"claude"}"#,
        ],
    );
    let plain = refused(&["send", "scratch", "hi"], None);
    assert!(
        plain.contains("scratch is a plain session; it is not the master's")
            && plain.contains("Plain sessions are the user's own"),
        "{plain}"
    );
    let log = fs::read_to_string(home.path().join("flows/cccc.jsonl")).unwrap();
    assert!(!log.contains("message"), "{log}");
}

#[test]
fn send_takes_a_message_while_config_toml_is_broken() {
    let home = TempDir::new().unwrap();
    seed_worker(
        home.path(),
        "7f3c0000-0000-4000-8000-000000000000",
        "shop-217",
        r#"{"v":1,"at":2000,"kind":"idle","note":"plan done"}"#,
    );
    fs::write(home.path().join("no-config.toml"), "agent = [\n").unwrap();

    let sent = as_master(home.path(), &["send", "shop-217", "Tabs."]);
    let typed = as_master(home.path(), &["send", "--type", "shop-217", "/compact"]);

    assert!(sent.status.success(), "{}", stdout(&sent));
    assert_eq!(typed.status.code(), Some(1), "{}", stdout(&typed));
    assert!(
        stdout(&typed).contains("no-config.toml:1: "),
        "{}",
        stdout(&typed)
    );
}

#[test]
fn send_type_queues_lines_in_order_in_the_flows_log() {
    let home = TempDir::new().unwrap();
    seed_worker(
        home.path(),
        "7f3c0000-0000-4000-8000-000000000000",
        "shop-217",
        r#"{"v":1,"at":2000,"kind":"idle","note":"plan done"}"#,
    );

    let first = as_master(home.path(), &["send", "--type", "shop-217", "/compact"]);
    let second = as_master(
        home.path(),
        &["send", "--type", "shop-217", "continue shipping"],
    );

    assert!(first.status.success(), "{}", stdout(&first));
    assert!(
        stdout(&first).starts_with("queued: shop-217\nline: /compact\nahead: 0\n"),
        "{}",
        stdout(&first)
    );
    assert!(stdout(&second).contains("ahead: 1"), "{}", stdout(&second));
    let log = fs::read_to_string(
        home.path()
            .join("flows/7f3c0000-0000-4000-8000-000000000000.jsonl"),
    )
    .unwrap();
    let typed: Vec<&str> = log
        .lines()
        .filter(|line| line.contains(r#""kind":"typed""#))
        .collect();
    assert_eq!(typed.len(), 2, "{log}");
    assert!(typed[0].contains(r#""text":"/compact""#), "{log}");
    assert!(!log.contains(r#""kind":"message""#), "{log}");
    let detail = stdout(&saaio(home.path(), &["status", "shop-217"]));
    assert!(detail.contains("master typed: /compact"), "{detail}");

    fs::create_dir_all(home.path().join("waits")).unwrap();
    fs::write(
        home.path()
            .join("waits/7f3c0000-0000-4000-8000-000000000000.typed"),
        "2",
    )
    .unwrap();
    let after = as_master(home.path(), &["send", "--type", "shop-217", "/review"]);
    assert!(stdout(&after).contains("ahead: 0"), "{}", stdout(&after));
}

#[test]
fn send_type_refuses_more_than_one_line_and_an_agent_without_turns() {
    let home = TempDir::new().unwrap();
    seed_worker(
        home.path(),
        "7f3c0000-0000-4000-8000-000000000000",
        "shop-217",
        r#"{"v":1,"at":2000,"kind":"working"}"#,
    );
    seed(
        home.path(),
        "aaaa",
        &[
            r#"{"v":1,"at":1000,"kind":"started","name":"old-cli","request":"x","mode":"auto"}"#,
            r#"{"v":1,"at":1500,"kind":"workspace","path":"/repo","branch":"flow/old-cli","slot":2,"window":"saaio-ui","agent":"aider"}"#,
        ],
    );
    let config = home.path().join("config.toml");
    fs::write(
        &config,
        "[agents.aider]\ncommand = [\"aider\"]\nprompt = \"{request}\"\n",
    )
    .unwrap();
    let refused = |args: &[&str]| {
        let output = saaio_command(home.path())
            .args(args)
            .env("SAAIO_PROOF", key(home.path(), "master"))
            .env("SAAIO_CONFIG", &config)
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(1),
            "{args:?}: {}",
            stdout(&output)
        );
        stdout(&output)
    };

    let lines = refused(&["send", "--type", "shop-217", "/compact\ncontinue"]);
    assert!(
        lines.contains("a typed line is one line")
            && lines.contains("Queue one `saaio send --type` per line"),
        "{lines}"
    );
    let aider = refused(&["send", "--type", "old-cli", "/compact"]);
    assert!(
        aider.contains("old-cli runs aider, which reports no turns to saaio")
            && aider.contains("Use `saaio send` for a message"),
        "{aider}"
    );
    assert!(refused(&["send", "--type", "shop-217", "  "]).contains("nothing to send"));
    let log = fs::read_to_string(
        home.path()
            .join("flows/7f3c0000-0000-4000-8000-000000000000.jsonl"),
    )
    .unwrap();
    assert!(!log.contains("typed"), "{log}");
}
