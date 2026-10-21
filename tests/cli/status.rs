//! Tests of `saaio status`: the list, detail, `--json`, usage errors, help and a closed pipe

use std::path::Path;

use tempfile::TempDir;

use crate::support::*;

fn seed_blocked(home: &Path) {
    seed(
        home,
        "7f3c0000-0000-4000-8000-000000000000",
        &[
            r#"{"v":1,"at":1000,"kind":"started","name":"shop-221","request":"SHOP-221 add CSV export","mode":"auto"}"#,
            r#"{"v":1,"at":2000,"kind":"phase","phase":"implement"}"#,
            r#"{"v":1,"at":3000,"kind":"blocked","reason":"no default"}"#,
        ],
    );
}

#[test]
fn status_lists_flows_and_shows_one_in_detail() {
    let home = TempDir::new().unwrap();
    seed_blocked(home.path());

    let list = stdout(&saaio(home.path(), &["status"]));
    assert!(list.contains("shop-221,blocked,implement,"), "{list}");
    assert!(
        list.contains("blockers[1]{name,blocker}:\n  shop-221,no default"),
        "{list}"
    );

    let detail = stdout(&saaio(home.path(), &["status", "shop-221"]));
    assert!(
        detail.starts_with("name: shop-221\nid: 7f3c0000-0000-4000-8000-000000000000\n"),
        "{detail}"
    );
}

#[test]
fn status_json_is_the_versioned_board() {
    let home = TempDir::new().unwrap();
    seed_blocked(home.path());

    let output = saaio(home.path(), &["status", "--json"]);

    assert!(output.status.success());
    let board: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(board["version"], 3);
    assert_eq!(board["needsYou"], 1);
    assert_eq!(board["flows"][0]["name"], "shop-221");
    assert_eq!(board["flows"][0]["blocker"], "no default");
}

#[test]
fn an_unknown_flag_fails_loudly_on_stdout_with_exit_2() {
    let home = TempDir::new().unwrap();

    let output = saaio(home.path(), &["status", "--nope"]);

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stderr.is_empty());
    let text = stdout(&output);
    assert!(
        text.starts_with("error: unexpected argument '--nope' found"),
        "{text}"
    );
    assert!(
        text.contains("Run `saaio status --help` for the usage"),
        "{text}"
    );
}

#[test]
fn a_flag_without_its_value_names_the_command_to_read_up_on() {
    let home = TempDir::new().unwrap();

    for (args, command) in [
        (vec!["start", "--name"], "saaio start"),
        (vec!["report", "--flow"], "saaio report"),
        (vec!["--nope"], "saaio"),
    ] {
        let output = saaio(home.path(), &args);

        assert_eq!(output.status.code(), Some(2), "{args:?}");
        let text = stdout(&output);
        assert!(
            text.contains(&format!("Run `{command} --help` for the usage")),
            "{args:?}: {text}"
        );
    }
}

#[test]
fn a_missing_flow_fails_with_exit_1_and_next_steps() {
    let home = TempDir::new().unwrap();

    let output = saaio(home.path(), &["status", "shop-999"]);

    assert_eq!(output.status.code(), Some(1));
    let text = stdout(&output);
    assert!(text.starts_with("error: no flow named shop-999"), "{text}");
    assert!(text.contains("Run `saaio` to list the flows"), "{text}");
}

#[test]
fn help_and_version_still_print_normally() {
    let home = TempDir::new().unwrap();

    let help = saaio(home.path(), &["--help"]);
    let version = saaio(home.path(), &["--version"]);

    assert!(help.status.success());
    assert!(stdout(&help).contains("Usage: saaio"));
    assert!(
        stdout(&help).contains("Run alone in a terminal, saaio opens the terminal UI."),
        "{}",
        stdout(&help)
    );
    assert!(stdout(&help).contains("--stop"), "{}", stdout(&help));
    let version = stdout(&version);
    let commit = version
        .trim()
        .strip_prefix("saaio 0.1.0 (")
        .and_then(|rest| rest.strip_suffix(')'));
    assert!(
        commit.is_some_and(|commit| !commit.is_empty() && !commit.contains(' ')),
        "{version}"
    );
}

#[test]
fn saaio_ui_is_an_unknown_command() {
    let home = TempDir::new().unwrap();

    let output = saaio(home.path(), &["ui"]);

    assert_eq!(output.status.code(), Some(2));
    let text = stdout(&output);
    assert!(text.contains("unrecognized subcommand 'ui'"), "{text}");
    assert!(!home.path().join("server.sock").exists());
}

#[test]
fn the_server_command_stays_out_of_the_help() {
    let home = TempDir::new().unwrap();

    let help = stdout(&saaio(home.path(), &["--help"]));

    assert!(help.contains("start"), "{help}");
    assert!(
        !help
            .lines()
            .any(|line| line.trim_start().starts_with("ui ")),
        "{help}"
    );
    assert!(
        !help
            .lines()
            .any(|line| line.trim_start().starts_with("server ")),
        "{help}"
    );
}

#[test]
fn a_reader_that_went_away_ends_saaio_quietly() {
    let home = TempDir::new().unwrap();
    let (reader, writer) = std::io::pipe().unwrap();
    drop(reader);

    let output = saaio_command(home.path())
        .args(["completions", "bash"])
        .stdout(writer)
        .output()
        .unwrap();

    assert!(output.status.success(), "{:?}", output.status);
    assert!(output.stderr.is_empty(), "{}", stderr(&output));
}
