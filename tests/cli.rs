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
        .trim_matches('"')
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

/// A temp git repo with one commit, a fake tmux on PATH that records its argv and prints @14,
/// and a state folder.
struct Lab {
    _dir: TempDir,
    repo: std::path::PathBuf,
    home: std::path::PathBuf,
    bin: std::path::PathBuf,
}

impl Lab {
    fn new() -> Lab {
        let dir = TempDir::new().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let repo = root.join("repo");
        let home = root.join("state");
        let bin = root.join("bin");
        fs::create_dir_all(&repo).unwrap();
        fs::create_dir_all(&bin).unwrap();
        let git = |args: &[&str]| {
            assert!(
                Command::new("git")
                    .arg("-C")
                    .arg(&repo)
                    .args(args)
                    .status()
                    .unwrap()
                    .success()
            )
        };
        git(&["init", "-q", "-b", "main"]);
        git(&[
            "-c",
            "user.email=t@t",
            "-c",
            "user.name=t",
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "init",
        ]);
        let tmux = bin.join("tmux");
        fs::write(
            &tmux,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$@\" > {root}/tmux-args\nprintf '%s\\n' \"$*\" >> {root}/tmux-log\necho @14\n",
                root = root.display()
            ),
        )
        .unwrap();
        set_executable(&tmux);
        Lab {
            _dir: dir,
            repo,
            home,
            bin,
        }
    }

    fn start(&self, args: &[&str], in_tmux: bool) -> Output {
        let path = format!(
            "{}:{}",
            self.bin.display(),
            std::env::var("PATH").unwrap_or_default()
        );
        let mut command = Command::new(env!("CARGO_BIN_EXE_flow"));
        command
            .arg("start")
            .args(args)
            .current_dir(&self.repo)
            .env("FLOW_HOME", &self.home)
            .env("FLOW_CONFIG", self.home.join("no-config.toml"))
            .env("PATH", path)
            .env_remove("FLOW_ID");
        if in_tmux {
            command.env("TMUX", "/tmp/tmux-test,1,0");
        } else {
            command.env_remove("TMUX");
        }
        command.output().unwrap()
    }

    /// Runs flow with the fake tmux on PATH and inside "tmux".
    fn run(&self, args: &[&str]) -> Output {
        let path = format!(
            "{}:{}",
            self.bin.display(),
            std::env::var("PATH").unwrap_or_default()
        );
        Command::new(env!("CARGO_BIN_EXE_flow"))
            .args(args)
            .current_dir(&self.repo)
            .env("FLOW_HOME", &self.home)
            .env("FLOW_CONFIG", self.home.join("no-config.toml"))
            .env("PATH", path)
            .env("TMUX", "/tmp/tmux-test,1,0")
            .env_remove("FLOW_ID")
            .output()
            .unwrap()
    }

    fn tmux_log(&self) -> Vec<String> {
        fs::read_to_string(self.bin.parent().unwrap().join("tmux-log"))
            .unwrap_or_default()
            .lines()
            .map(String::from)
            .collect()
    }

    fn tmux_args(&self) -> Vec<String> {
        fs::read_to_string(self.bin.parent().unwrap().join("tmux-args"))
            .unwrap_or_default()
            .lines()
            .map(String::from)
            .collect()
    }

    fn hook(&self, name: &str, script: &str) {
        let dir = self.repo.join(".flow");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(name), script).unwrap();
        set_executable(&dir.join(name));
    }
}

fn set_executable(file: &Path) {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(file, fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn start_makes_a_worktree_runs_setup_and_opens_the_worker_window() {
    let lab = Lab::new();
    lab.hook("setup", "#!/bin/sh\necho \"slot $FLOW_SLOT\" > setup-ran\n");

    let output = lab.start(&["SHOP-217 add CSV export"], true);

    let text = stdout(&output);
    assert!(output.status.success(), "{text}");
    let worktree = lab.repo.join(".flow/worktrees/shop-217");
    assert!(text.starts_with("flow: shop-217\n"), "{text}");
    assert!(
        text.contains(&format!("worktree: {}", worktree.display())),
        "{text}"
    );
    assert!(
        text.contains("branch: flow/shop-217\nslot: 1\nsetup: ran\nagent: claude\nwindow: @14"),
        "{text}"
    );
    assert_eq!(
        fs::read_to_string(worktree.join("setup-ran"))
            .unwrap()
            .trim(),
        "slot 1"
    );

    let args = lab.tmux_args();
    assert_eq!(
        &args[..8],
        [
            "new-window",
            "-d",
            "-P",
            "-F",
            "#{window_id}",
            "-n",
            "shop-217",
            "-c"
        ]
    );
    assert!(
        args.iter().any(|arg| arg.starts_with("FLOW_ID=")),
        "{args:?}"
    );
    assert!(
        args.iter()
            .any(|arg| *arg == format!("FLOW_HOME={}", lab.home.display())),
        "{args:?}"
    );
    let agent = &args[args.iter().position(|arg| arg == "--").unwrap() + 1..];
    assert_eq!(agent[0], "claude");
    assert_eq!(
        agent.last().unwrap(),
        "/ship-feature SHOP-217 add CSV export"
    );

    let board: serde_json::Value =
        serde_json::from_slice(&flow(&lab.home, &["status", "--json"]).stdout).unwrap();
    assert_eq!(board["flows"][0]["workspace"]["window"], "@14");
    assert_eq!(board["flows"][0]["workspace"]["slot"], 1);
}

#[test]
fn start_refuses_outside_tmux_and_outside_a_repository() {
    let lab = Lab::new();

    let outside_tmux = lab.start(&["x"], false);
    assert_eq!(outside_tmux.status.code(), Some(1));
    assert!(
        stdout(&outside_tmux).contains("run it inside tmux"),
        "{}",
        stdout(&outside_tmux)
    );

    let elsewhere = TempDir::new().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_flow"))
        .args(["start", "x"])
        .current_dir(elsewhere.path())
        .env("FLOW_HOME", &lab.home)
        .env("FLOW_CONFIG", lab.home.join("no-config.toml"))
        .env("TMUX", "/tmp/tmux-test,1,0")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(
        stdout(&output).contains("needs a git repository"),
        "{}",
        stdout(&output)
    );
}

#[test]
fn a_failing_setup_marks_the_flow_blocked_but_still_opens_the_window() {
    let lab = Lab::new();
    lab.hook(
        "setup",
        "#!/bin/sh\necho 'port 8001 is taken' >&2\nexit 1\n",
    );

    let output = lab.start(&["SHOP-218 fix totals"], true);

    let text = stdout(&output);
    assert!(output.status.success(), "{text}");
    assert!(
        text.contains("setup: \"failed: port 8001 is taken\""),
        "{text}"
    );
    assert!(!lab.tmux_args().is_empty());
    let board: serde_json::Value =
        serde_json::from_slice(&flow(&lab.home, &["status", "--json"]).stdout).unwrap();
    assert_eq!(board["flows"][0]["state"], "blocked");
    assert_eq!(
        board["flows"][0]["blocker"],
        "setup failed: port 8001 is taken"
    );
}

#[test]
fn start_says_when_every_slot_is_taken() {
    let lab = Lab::new();
    for slot in 1..=9 {
        seed(
            &lab.home,
            &format!("f{slot}"),
            &[
                &format!(
                    r#"{{"v":1,"at":1,"kind":"started","name":"f{slot}","request":"x","mode":"guided"}}"#
                ),
                &format!(
                    r#"{{"v":1,"at":2,"kind":"workspace","path":"/p{slot}","branch":"b","slot":{slot},"window":null,"agent":"claude"}}"#
                ),
            ],
        );
    }

    let output = lab.start(&["one more"], true);

    assert_eq!(output.status.code(), Some(1));
    assert!(
        stdout(&output).contains("all 9 slots are in use"),
        "{}",
        stdout(&output)
    );
}

#[test]
fn clear_tears_down_and_removes_clean_worktrees_but_keeps_uncommitted_work() {
    let lab = Lab::new();
    lab.hook(
        "teardown",
        "#!/bin/sh\necho \"$FLOW_NAME\" >> \"$FLOW_REPO/torn-down\"\n",
    );
    assert!(lab.start(&["SHOP-1 clean one"], true).status.success());
    assert!(lab.start(&["SHOP-2 dirty one"], true).status.success());
    assert!(lab.start(&["SHOP-3 still running"], true).status.success());
    let worktrees = lab.repo.join(".flow/worktrees");
    fs::write(worktrees.join("shop-2/work.txt"), "unsaved").unwrap();
    for name in ["shop-1", "shop-2"] {
        assert!(
            flow(&lab.home, &["report", "--flow", name, "ended"])
                .status
                .success()
        );
    }

    let output = flow(&lab.home, &["clear"]);

    let text = stdout(&output);
    assert!(output.status.success(), "{text}");
    assert!(text.starts_with("cleared: 2\n"), "{text}");
    assert!(
        text.contains(&format!(
            "kept[1]{{name,path}}:\n  shop-2,{}",
            worktrees.join("shop-2").display()
        )),
        "{text}"
    );
    assert!(!worktrees.join("shop-1").exists());
    assert!(worktrees.join("shop-2").exists());
    assert!(worktrees.join("shop-3").exists());
    let torn: Vec<String> = fs::read_to_string(lab.repo.join("torn-down"))
        .unwrap()
        .lines()
        .map(String::from)
        .collect();
    let mut torn = torn;
    torn.sort();
    assert_eq!(torn, vec!["shop-1", "shop-2"]);
    let branches = Command::new("git")
        .arg("-C")
        .arg(&lab.repo)
        .args(["branch", "--list", "flow/*"])
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8(branches.stdout).unwrap().lines().count(),
        3
    );
}

#[test]
fn remove_closes_the_window_tears_down_and_forgets_a_running_flow() {
    let lab = Lab::new();
    lab.hook(
        "teardown",
        "#!/bin/sh\necho \"$FLOW_NAME\" >> \"$FLOW_REPO/torn-down\"\n",
    );
    assert!(lab.start(&["SHOP-1 first try"], true).status.success());
    assert!(lab.start(&["SHOP-2 keep me"], true).status.success());

    let output = lab.run(&["remove", "shop-1"]);

    let text = stdout(&output);
    assert!(output.status.success(), "{text}");
    assert!(
        text.starts_with("removed: shop-1\nwindow: closed\nworktree: removed\n"),
        "{text}"
    );
    assert!(
        lab.tmux_log().contains(&"kill-window -t @14".to_string()),
        "{:?}",
        lab.tmux_log()
    );
    assert!(!lab.repo.join(".flow/worktrees/shop-1").exists());
    assert_eq!(
        fs::read_to_string(lab.repo.join("torn-down"))
            .unwrap()
            .trim(),
        "shop-1"
    );
    let list = stdout(&flow(&lab.home, &["status"]));
    assert!(
        list.contains("count: 1") && list.contains("shop-2"),
        "{list}"
    );
}

#[test]
fn remove_keeps_uncommitted_work_unless_forced() {
    let lab = Lab::new();
    assert!(lab.start(&["SHOP-1 dirty"], true).status.success());
    let worktree = lab.repo.join(".flow/worktrees/shop-1");
    fs::write(worktree.join("work.txt"), "unsaved").unwrap();

    let kept = lab.run(&["remove", "shop-1"]);
    let text = stdout(&kept);
    assert!(
        text.contains(&format!("worktree: \"kept: {}\"", worktree.display())),
        "{text}"
    );
    assert!(text.contains("flow remove shop-1 --force"), "{text}");
    assert!(worktree.exists());

    assert!(lab.start(&["SHOP-2 dirty too"], true).status.success());
    let other = lab.repo.join(".flow/worktrees/shop-2");
    fs::write(other.join("work.txt"), "unsaved").unwrap();
    let forced = lab.run(&["remove", "shop-2", "--force"]);
    assert!(
        stdout(&forced).contains("worktree: removed"),
        "{}",
        stdout(&forced)
    );
    assert!(!other.exists());
}

#[test]
fn remove_says_when_there_is_no_such_flow() {
    let lab = Lab::new();

    let output = lab.run(&["remove", "shop-999"]);

    assert_eq!(output.status.code(), Some(1));
    assert!(
        stdout(&output).starts_with("error: no flow named shop-999"),
        "{}",
        stdout(&output)
    );
}

#[test]
fn ui_stop_ends_a_running_server_and_says_when_none_runs() {
    let home = TempDir::new().unwrap();
    let config = home.path().join("config.toml");
    fs::write(
        &config,
        "[agents.claude]\ncommand = [\"cat\"]\nprompt = \"\"\nresume = [\"cat\"]\n",
    )
    .unwrap();
    let socket = home.path().join("server.sock");

    let none = flow(home.path(), &["ui", "--stop"]);
    assert!(none.status.success());
    assert_eq!(stdout(&none).trim(), "no flow server runs");

    let mut server = Command::new(env!("CARGO_BIN_EXE_flow"))
        .arg("server")
        .env("FLOW_HOME", home.path())
        .env("FLOW_CONFIG", &config)
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !socket.exists() {
        assert!(
            std::time::Instant::now() < deadline,
            "the server never listened"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }

    let stopped = flow(home.path(), &["ui", "--stop"]);

    assert_eq!(
        stdout(&stopped).trim(),
        "stopped the flow server and every session in it"
    );
    assert!(server.wait().unwrap().success());
    assert!(!socket.exists());
    assert!(home.path().join("master.json").exists());
}

#[test]
fn the_server_command_stays_out_of_the_help() {
    let home = TempDir::new().unwrap();

    let help = stdout(&flow(home.path(), &["--help"]));

    assert!(help.contains("ui"), "{help}");
    assert!(!help.contains("server"), "{help}");
}
