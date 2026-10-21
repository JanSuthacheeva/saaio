use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Stdio;
use std::thread;
use std::time::Duration;

use tempfile::TempDir;

use crate::support::*;

#[test]
fn every_repository_has_its_own_store_without_saaio_home() {
    let shop = Lab::new();
    let twin = Lab::new();
    let state = shop.dir.path().canonicalize().unwrap().join("xdg");
    let outside = shop.dir.path().join("notes");
    fs::create_dir_all(&outside).unwrap();
    let run_in = |dir: &Path, args: &[&str]| {
        let output = saaio_command(&shop.home)
            .env_remove("SAAIO_HOME")
            .env("XDG_STATE_HOME", &state)
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap();
        stdout(&output)
    };

    let started = run_in(&shop.repo, &["start", "SHOP-1 one"]);
    let worktree = shop.repo.join(".saaio/worktrees/shop-1");
    run_in(&twin.repo, &["new", "--name", "twin", "--request", "two"]);

    assert!(started.contains("slot: 1"), "{started}");
    assert!(state.join("saaio/projects/repo/repo").is_file());
    assert!(run_in(&worktree, &["status"]).contains("shop-1"));
    assert!(run_in(&shop.repo.join(".saaio"), &["status"]).contains("shop-1"));
    assert!(state.join("saaio/projects/repo-2/flows").is_dir());
    assert!(!run_in(&twin.repo, &["status"]).contains("shop-1"));
    assert!(run_in(&outside, &["status"]).contains("count: 0"));
    let all = run_in(&outside, &["status", "--all"]);
    assert!(all.contains("  repo,shop-1,"), "{all}");
    assert!(all.contains("  repo-2,twin,"), "{all}");
}

/// A git repository at `home/<name>` whose origin is `origin`, and its folder
/// .saaio/worktrees/<name> as a flow's worktree on flow/<name>, appended to that flow's log.
fn worktree_of(home: &Path, id: &str, name: &str, origin: &str) {
    let repo = home.join(name);
    let worktree = repo.join(".saaio/worktrees").join(name);
    fs::create_dir_all(&worktree).unwrap();
    git(&repo, &["init", "-q"]);
    git(&repo, &["remote", "add", "origin", origin]);
    let workspace = format!(
        "{{\"v\":1,\"at\":2,\"kind\":\"workspace\",\"path\":\"{}\",\"branch\":\"flow/{name}\",\"slot\":1,\"window\":null,\"agent\":\"claude\"}}\n",
        worktree.display()
    );
    fs::OpenOptions::new()
        .append(true)
        .open(home.join(format!("flows/{id}.jsonl")))
        .unwrap()
        .write_all(workspace.as_bytes())
        .unwrap();
}

/// `saaio server` with cat for the worker and the master and `gh` from `script` first on PATH;
/// its stderr in home/server.log.
fn server_with_gh(home: &Path, script: &str) -> Running {
    let config = home.join("config.toml");
    fs::write(
        &config,
        "[agents.claude]\ncommand = [\"cat\"]\nprompt = \"\"\nresume = [\"cat\"]\n\n[agents.master]\ncommand = [\"cat\"]\nprompt = \"\"\nresume = [\"cat\"]\n",
    )
    .unwrap();
    let bin = home.join("bin");
    fs::create_dir_all(&bin).unwrap();
    executable(&bin.join("gh"), script);
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap());
    let log = fs::File::create(home.join("server.log")).unwrap();
    let server = saaio_command(home)
        .arg("server")
        .stdout(Stdio::null())
        .stderr(log)
        .env("SAAIO_CONFIG", &config)
        .env("PATH", path)
        .spawn()
        .unwrap();
    let server = Running(server);
    wait_for_socket(&home.join("server.sock"));
    server
}

#[test]
fn a_store_is_for_its_user_alone() {
    let outer = TempDir::new().unwrap();
    let home = outer.path().join("store");
    let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;

    let id = new_flow(&home, "csv");

    assert_eq!(mode(&home), 0o700);
    assert_eq!(mode(&home.join("flows")), 0o700);
    assert_eq!(mode(&home.join(format!("flows/{id}.jsonl"))), 0o600);
    fs::set_permissions(&home, fs::Permissions::from_mode(0o755)).unwrap();
    let _server = server_with_gh(&home, "#!/bin/sh\nexit 1\n");
    assert_eq!(mode(&home), 0o700, "the server closes a store left open");
    // The server writes master.json after it starts listening.
    let master = home.join("master.json");
    wait_until("the server never wrote master.json", || master.exists());
    assert_eq!(mode(&master), 0o600);
    assert!(saaio(&home, &["--stop"]).status.success());
}

#[test]
fn a_pull_request_merged_elsewhere_finishes_its_flow() {
    let home = TempDir::new().unwrap();
    let id = new_flow(home.path(), "csv");
    worktree_of(home.path(), &id, "csv", "https://github.com/acme/shop.git");
    let _server = server_with_gh(
        home.path(),
        r#"#!/bin/sh
[ "$*" = 'pr list --head flow/csv --state all --json url,state,createdAt --limit 10' ] || exit 1
echo '[{"createdAt":"2020-05-04T10:00:00Z","state":"MERGED","url":"https://github.com/acme/shop/pull/7"},{"createdAt":"2099-01-01T00:00:00Z","state":"MERGED","url":"https://github.com/acme/shop/pull/42"}]'
"#,
    );
    let log = home.path().join(format!("flows/{id}.jsonl"));

    wait_until("no merged event", || {
        fs::read_to_string(&log)
            .unwrap()
            .contains(r#""kind":"merged""#)
    });

    let json: serde_json::Value =
        serde_json::from_str(&stdout(&saaio(home.path(), &["status", "--json"]))).unwrap();
    let flow = &json["flows"][0];
    assert_eq!(flow["state"], "done");
    assert_eq!(flow["merged"], true);
    assert_eq!(flow["mergeRequest"], "https://github.com/acme/shop/pull/42");
    assert_eq!(flow["phase"], "merge-request");
}

#[test]
fn a_forge_that_cannot_answer_records_nothing_and_is_logged_once() {
    let home = TempDir::new().unwrap();
    for name in ["csv", "pdf"] {
        let id = new_flow(home.path(), name);
        worktree_of(home.path(), &id, name, "git@github.com:acme/shop.git");
    }
    let _server = server_with_gh(
        home.path(),
        "#!/bin/sh\necho 'To get started with GitHub CLI, please run:  gh auth login' >&2\nexit 4\n",
    );
    let server_log = home.path().join("server.log");

    wait_until("the failure was not logged", || {
        fs::read_to_string(&server_log)
            .unwrap()
            .contains("could not ask about merge requests")
    });
    thread::sleep(Duration::from_millis(500));

    let lines = fs::read_to_string(&server_log).unwrap();
    let failures: Vec<&str> = lines
        .lines()
        .filter(|line| line.contains("could not ask about merge requests"))
        .collect();
    assert_eq!(failures.len(), 1, "{lines}");
    assert!(
        failures[0]
            .ends_with("gh failed: To get started with GitHub CLI, please run:  gh auth login"),
        "{lines}"
    );
    let status = stdout(&saaio(home.path(), &["status", "--json"]));
    assert!(!status.contains("\"done\""), "{status}");
}
