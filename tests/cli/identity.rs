//! Tests of who saaio hook, send and wait act for: only the session whose pane key proves it

use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};

use tempfile::TempDir;

use crate::support::*;

const WORKER: &str = "7f3c0000-0000-4000-8000-000000000000";

/// `saaio <args>` with exactly these identity variables, as a session that set them itself.
/// What a hook logs in a pane an earlier server opened, which handed it no key.
const NO_KEY: &str = "no SAAIO_PROOF: an earlier saaio server opened this pane; saaio --stop";

fn claiming(home: &Path, args: &[&str], vars: &[(&str, &str)]) -> Command {
    let mut command = saaio_command(home);
    command.args(args).envs(vars.iter().copied());
    command
}

fn server_log(home: &Path) -> String {
    fs::read_to_string(home.join("server.log")).unwrap_or_default()
}

fn flow_log(home: &Path, id: &str) -> String {
    fs::read_to_string(home.join(format!("flows/{id}.jsonl"))).unwrap()
}

#[test]
fn a_hook_reports_into_a_flow_only_with_that_flows_key() {
    let home = TempDir::new().unwrap();
    let victim = new_flow(home.path(), "victim");
    let other = new_flow(home.path(), "other");
    let other_key = key(home.path(), &other);
    let stop = r#"{"hook_event_name":"Stop","last_assistant_message":"forged"}"#;
    let before = flow_log(home.path(), &victim);

    for vars in [
        vec![("SAAIO_FLOW_ID", victim.as_str())],
        vec![("SAAIO_FLOW_ID", victim.as_str()), ("SAAIO_PROOF", "guess")],
        vec![
            ("SAAIO_FLOW_ID", victim.as_str()),
            ("SAAIO_PROOF", other_key.as_str()),
        ],
    ] {
        let output = feed(claiming(home.path(), &["hook", "claude-code"], &vars), stop);
        assert!(output.status.success(), "{vars:?}");
        assert!(output.stdout.is_empty() && output.stderr.is_empty());
    }

    assert_eq!(flow_log(home.path(), &victim), before);
    let log = server_log(home.path());
    assert_eq!(
        log.matches(&format!("SAAIO_PROOF does not prove flow {victim}"))
            .count(),
        2,
        "{log}"
    );
    assert_eq!(log.matches(NO_KEY).count(), 1, "{log}");
    let own_key = key(home.path(), &victim);
    let vars = [
        ("SAAIO_FLOW_ID", victim.as_str()),
        ("SAAIO_PROOF", own_key.as_str()),
    ];
    feed(claiming(home.path(), &["hook", "claude-code"], &vars), stop);
    assert!(flow_log(home.path(), &victim).contains("forged"));
}

#[test]
fn a_hook_is_the_masters_only_with_the_masters_key() {
    let home = TempDir::new().unwrap();
    let worker = new_flow(home.path(), "shop-221");
    let worker_key = key(home.path(), &worker);
    let prompt = r#"{"hook_event_name":"UserPromptSubmit","prompt":"status?"}"#;

    for vars in [
        vec![("SAAIO_MASTER", "1")],
        vec![("SAAIO_MASTER", "1"), ("SAAIO_PROOF", worker_key.as_str())],
    ] {
        let output = feed(
            claiming(home.path(), &["hook", "claude-code"], &vars),
            prompt,
        );
        assert!(output.status.success(), "{vars:?}");
    }

    assert!(!home.path().join("waits/master.turn").exists());
    assert!(!home.path().join("waits/master.busy").exists());
    let log = server_log(home.path());
    assert_eq!(
        log.matches("SAAIO_PROOF does not prove the master").count(),
        1,
        "{log}"
    );
    assert_eq!(log.matches(NO_KEY).count(), 1, "{log}");
}

#[test]
fn only_the_masters_key_sends() {
    let home = TempDir::new().unwrap();
    seed_worker(
        home.path(),
        WORKER,
        "shop-217",
        r#"{"v":1,"at":2000,"kind":"working"}"#,
    );
    let worker_key = key(home.path(), WORKER);

    for vars in [vec![], vec![("SAAIO_PROOF", worker_key.as_str())]] {
        let output = claiming(home.path(), &["send", "shop-217", "forged"], &vars)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1), "{vars:?}");
        assert!(
            stdout(&output).contains("only the master sends"),
            "{}",
            stdout(&output)
        );
    }
    assert!(!flow_log(home.path(), WORKER).contains("forged"));

    let keyless = claiming(
        home.path(),
        &["send", "shop-217", "forged"],
        &[("SAAIO_MASTER", "1")],
    )
    .output()
    .unwrap();
    assert_eq!(keyless.status.code(), Some(1));
    let said = stdout(&keyless);
    assert!(
        said.contains("this pane has no key: an earlier saaio server opened it")
            && said.contains("Ask the user to run `saaio --stop` and start saaio again"),
        "{said}"
    );

    let sent = as_master(home.path(), &["send", "shop-217", "Tabs."]);
    assert!(sent.status.success(), "{}", stdout(&sent));
    assert!(flow_log(home.path(), WORKER).contains("Tabs."));
}

/// Runs `saaio wait <args>` with `vars` to its end: its exit code and stderr.
fn wait(home: &Path, args: &[&str], vars: &[(&str, &str)]) -> (i32, String) {
    let mut command = claiming(home, &["wait"], vars);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = Running(command.spawn().unwrap());
    let (code, _, err) = ended(&mut child, PATIENCE).expect("the wait ends");
    (code, err)
}

#[test]
fn a_wait_without_the_sessions_key_takes_no_news() {
    let home = TempDir::new().unwrap();
    seed_worker(
        home.path(),
        WORKER,
        "shop-217",
        r#"{"v":1,"at":2000,"kind":"message","text":"Semicolons."}"#,
    );

    let worker = wait(home.path(), &[], &[("SAAIO_FLOW_ID", WORKER)]);
    let master = wait(home.path(), &["--master"], &[]);

    assert_eq!(worker, (0, String::new()));
    assert_eq!(master, (0, String::new()));
    assert!(!home.path().join(format!("waits/{WORKER}.pid")).exists());
    assert!(!home.path().join("waits/master.pid").exists());
    let log = server_log(home.path());
    assert!(
        log.contains(&format!(
            "wait flow {WORKER}: SAAIO_PROOF does not prove flow {WORKER}"
        )),
        "{log}"
    );
    assert!(
        log.contains("wait master: SAAIO_PROOF does not prove the master"),
        "{log}"
    );
}
