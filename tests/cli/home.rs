//! Tests of bare `saaio`: the overview for agents, the UI in a person's terminal, `--stop`

use std::path::Path;

use tempfile::TempDir;

use crate::support::*;

#[test]
fn the_home_view_shows_the_binary_and_an_empty_state() {
    let home = TempDir::new().unwrap();

    let output = saaio(home.path(), &[]);

    assert!(output.status.success());
    let text = stdout(&output);
    assert!(text.starts_with("bin: "), "{text}");
    assert!(text.contains("description: "), "{text}");
    assert!(text.contains("flows: none tracked"), "{text}");
}

#[test]
fn the_home_view_shortens_the_binary_only_inside_home() {
    let home = TempDir::new().unwrap();
    let exe = Path::new(env!("CARGO_BIN_EXE_saaio"));
    let folder = exe.parent().unwrap();
    let bin_with = |user_home: &Path| {
        let output = saaio_command(home.path())
            .env("HOME", user_home)
            .output()
            .unwrap();
        stdout(&output).lines().next().unwrap().to_string()
    };
    let name = folder.file_name().unwrap().to_str().unwrap();
    let sibling = folder.with_file_name(&name[..name.len() - 1]);

    assert_eq!(
        bin_with(folder.parent().unwrap()),
        format!("bin: ~/{name}/saaio")
    );
    assert_eq!(bin_with(&sibling), format!("bin: {}", exe.display()));
    assert_eq!(bin_with(Path::new("")), format!("bin: {}", exe.display()));
}

/// Stops the saaio server of a store when the test ends, also when it fails.
struct StopsServer<'a>(&'a Path);

impl Drop for StopsServer<'_> {
    fn drop(&mut self) {
        let _ = saaio(self.0, &["--stop"]);
    }
}

#[test]
fn bare_saaio_in_an_agents_or_a_flows_terminal_prints_the_overview() {
    let home = TempDir::new().unwrap();
    let _stops = StopsServer(home.path());

    for marker in [
        "SAAIO_FLOW_ID",
        "SAAIO_MASTER",
        "CLAUDECODE",
        "CODEX_THREAD_ID",
    ] {
        let mut terminal = Terminal::open(home.path(), home.path(), &[], &[(marker, "1")]);

        let status = terminal.child.wait().unwrap();

        assert!(status.success(), "{marker}");
        wait_until(marker, || terminal.shows("flows: none tracked"));
        let text = terminal.text();
        assert!(text.starts_with("bin: "), "{marker}: {text}");
        assert!(!text.contains("\x1b[?1049h"), "{marker}: {text}");
    }
    assert!(!home.path().join("server.sock").exists());
}

#[test]
fn bare_saaio_in_a_persons_terminal_opens_the_ui() {
    let lab = Lab::new();
    lab.agent(
        r#"command = ["cat"]
prompt = ""
resume = ["cat"]"#,
    );

    let _stops = StopsServer(&lab.home);
    let terminal = Terminal::open(&lab.home, &lab.repo, &[], &[]);

    wait_for_socket(&lab.home.join("server.sock"));
    wait_until("the UI never drew", || terminal.shows("\x1b[?1049h"));
    assert!(!terminal.shows("bin: "), "{}", terminal.text());
    assert!(lab.run(&["--stop"]).status.success());
}

#[test]
fn bare_stop_in_a_terminal_never_opens_the_ui() {
    let home = TempDir::new().unwrap();

    for marker in [
        None,
        Some("SAAIO_FLOW_ID"),
        Some("SAAIO_MASTER"),
        Some("CLAUDECODE"),
        Some("CODEX_THREAD_ID"),
    ] {
        let env = marker.map(|name| vec![(name, "1")]).unwrap_or_default();
        let mut terminal = Terminal::open(home.path(), home.path(), &["--stop"], &env);
        assert!(terminal.child.wait().unwrap().success(), "{marker:?}");
        wait_until("stop did not print", || {
            terminal.shows("no saaio server runs")
        });
        assert!(
            !terminal.shows("\x1b[?1049h"),
            "{marker:?}: {}",
            terminal.text()
        );
    }
    assert!(!home.path().join("server.sock").exists());
}
