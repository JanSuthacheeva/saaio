use std::fs;
use std::io::{Read, Write};
use std::ops::{Deref, DerefMut};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use portable_pty::{CommandBuilder, MasterPty, PtySize, native_pty_system};
use tempfile::TempDir;

/// The flow binary with a test's own state: `home` as SAAIO_HOME, the shared folder inside it, a
/// config file that does not exist (so the built-in agents), no Claude Code home of the user's
/// and no FLOW_* variable of the shell it runs in.
fn saaio_command(home: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_saaio"));
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("FLOW_") {
            command.env_remove(key);
        }
    }
    command
        .env("SAAIO_HOME", home)
        .env("XDG_STATE_HOME", shared(home))
        .env("SAAIO_CONFIG", home.join("no-config.toml"))
        .env("XDG_CONFIG_HOME", home.join("no-config-home"))
        .env("CLAUDE_CONFIG_DIR", home.join("no-claude-home"));
    command
}

fn saaio(home: &Path, args: &[&str]) -> Output {
    saaio_command(home)
        .args(args)
        .output()
        .expect("the flow binary runs")
}

fn saaio_in(home: &Path, flow_id: &str, args: &[&str]) -> Output {
    saaio_command(home)
        .args(args)
        .env("SAAIO_FLOW_ID", flow_id)
        .output()
        .expect("the flow binary runs")
}

/// git in `dir`, which must succeed.
fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?}");
}

/// Polls `done` until it holds; fails the test with `what` after `limit`.
fn wait_until(limit: Duration, what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + limit;
    while !done() {
        assert!(Instant::now() < deadline, "{what}");
        thread::sleep(Duration::from_millis(20));
    }
}

/// Waits until something accepts connections on `socket`; a socket file can exist before that.
fn wait_for_socket(socket: &Path) {
    wait_until(Duration::from_secs(5), "the server never listened", || {
        UnixStream::connect(socket).is_ok()
    });
}

/// A flow process the test started, killed when the test ends, also when it fails.
struct Running(Child);

impl Deref for Running {
    type Target = Child;

    fn deref(&self) -> &Child {
        &self.0
    }
}

impl DerefMut for Running {
    fn deref_mut(&mut self) -> &mut Child {
        &mut self.0
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn new_flow(home: &Path, name: &str) -> String {
    let output = saaio(
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
    id_in(&stdout(&output))
}

/// The id a document prints; TOON quotes one that starts like a number (02e1...).
fn id_in(text: &str) -> String {
    text.lines()
        .find_map(|line| line.strip_prefix("id: "))
        .expect("the document prints an id")
        .trim_matches('"')
        .to_string()
}

/// The shared folder every project registers in: inside the test's home, never the real one.
fn shared(home: &Path) -> PathBuf {
    home.join("shared")
}

fn stdout(output: &Output) -> String {
    std::str::from_utf8(&output.stdout).unwrap().to_string()
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

/// Bare `flow` in a real terminal, as a person runs it: the test's own state, in `dir`, with
/// no agent's shell around it and `env` added.
struct Terminal {
    child: Box<dyn portable_pty::Child + Send + Sync>,
    screen: Arc<Mutex<Vec<u8>>>,
    _master: Box<dyn MasterPty + Send>,
}

impl Terminal {
    fn open(home: &Path, dir: &Path, env: &[(&str, &str)]) -> Terminal {
        let template = saaio_command(home);
        let mut command = CommandBuilder::new(template.get_program());
        for (key, value) in template.get_envs() {
            match value {
                Some(value) => command.env(key, value),
                None => command.env_remove(key),
            }
        }
        command.env_remove("CLAUDECODE");
        command.env_remove("CODEX_THREAD_ID");
        command.env("TERM", "xterm-256color");
        for (key, value) in env {
            command.env(key, value);
        }
        command.cwd(dir);
        let pair = native_pty_system()
            .openpty(PtySize {
                rows: 24,
                cols: 100,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();
        let child = pair.slave.spawn_command(command).unwrap();
        drop(pair.slave);
        let screen = Arc::new(Mutex::new(Vec::new()));
        let mut reader = pair.master.try_clone_reader().unwrap();
        let shown = Arc::clone(&screen);
        thread::spawn(move || {
            let mut buf = [0; 4096];
            while let Ok(n @ 1..) = reader.read(&mut buf) {
                shown.lock().unwrap().extend_from_slice(&buf[..n]);
            }
        });
        Terminal {
            child,
            screen,
            _master: pair.master,
        }
    }

    fn shows(&self, text: &str) -> bool {
        String::from_utf8_lossy(&self.screen.lock().unwrap()).contains(text)
    }

    fn text(&self) -> String {
        String::from_utf8_lossy(&self.screen.lock().unwrap()).to_string()
    }
}

/// Stops the flow server of a store when the test ends, also when it fails.
struct StopsServer<'a>(&'a Path);

impl Drop for StopsServer<'_> {
    fn drop(&mut self) {
        let _ = saaio(self.0, &["ui", "--stop"]);
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
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
        let mut terminal = Terminal::open(home.path(), home.path(), &[(marker, "1")]);

        let status = terminal.child.wait().unwrap();

        assert!(status.success(), "{marker}");
        wait_until(Duration::from_secs(5), marker, || {
            terminal.shows("flows: none tracked")
        });
        let text = terminal.text();
        assert!(text.starts_with("bin: "), "{marker}: {text}");
        assert!(!text.contains("\x1b[?1049h"), "{marker}: {text}");
    }
    assert!(!home.path().join("server.sock").exists());
}

#[test]
fn bare_saaio_in_a_persons_terminal_opens_saaio_ui() {
    let lab = Lab::new();
    lab.agent(
        r#"command = ["cat"]
prompt = ""
resume = ["cat"]"#,
    );

    let _stops = StopsServer(&lab.home);
    let terminal = Terminal::open(&lab.home, &lab.repo, &[]);

    wait_for_socket(&lab.home.join("server.sock"));
    wait_until(Duration::from_secs(5), "flow ui never drew", || {
        terminal.shows("\x1b[?1049h")
    });
    assert!(!terminal.shows("bin: "), "{}", terminal.text());
    assert!(lab.run(&["ui", "--stop"]).status.success());
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
    assert!(detail.starts_with("name: shop-221\nid: 7f3c\n"), "{detail}");
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
    assert!(text.contains("Run `flow` to list the flows"), "{text}");
}

#[test]
fn help_and_version_still_print_normally() {
    let home = TempDir::new().unwrap();

    let help = saaio(home.path(), &["--help"]);
    let version = saaio(home.path(), &["--version"]);

    assert!(help.status.success());
    assert!(stdout(&help).contains("Usage: saaio"));
    assert_eq!(stdout(&version).trim(), "saaio 0.1.0");
}

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
        "flow: shop-221\nstate: working\nphase: implement\nhelp[1]: Run `flow status shop-221` for its phases and timeline\n"
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
        stdout(&created).contains("Run `flow report --flow shop-221 phase spec` when spec starts"),
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
fn clear_forgets_finished_flows_only() {
    let home = TempDir::new().unwrap();
    let done = new_flow(home.path(), "export-csv");
    new_flow(home.path(), "shop-221");
    saaio_in(
        home.path(),
        &done,
        &["report", "merge-request", "https://x/88"],
    );

    let cleared = saaio(home.path(), &["clear"]);
    let again = saaio(home.path(), &["clear"]);

    assert_eq!(
        stdout(&cleared),
        "cleared: 1\nnames[1]: export-csv\nhelp[1]: Run `flow` to see what is left\n"
    );
    assert!(stdout(&again).starts_with("cleared: 0"));
    let list = stdout(&saaio(home.path(), &["status"]));
    assert!(
        list.contains("count: 1") && list.contains("shop-221"),
        "{list}"
    );
}

/// A temp git repo with one commit and a state folder.
struct Lab {
    dir: TempDir,
    repo: PathBuf,
    home: PathBuf,
}

impl Lab {
    fn new() -> Lab {
        let dir = TempDir::new().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let repo = root.join("repo");
        let home = root.join("state");
        fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        git(
            &repo,
            &[
                "-c",
                "user.email=t@t",
                "-c",
                "user.name=t",
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                "init",
            ],
        );
        Lab { dir, repo, home }
    }

    fn start(&self, args: &[&str]) -> Output {
        let mut all = vec!["start"];
        all.extend_from_slice(args);
        self.run(&all)
    }

    /// Runs flow in the repo with the lab's state.
    fn run(&self, args: &[&str]) -> Output {
        saaio_command(&self.home)
            .args(args)
            .current_dir(&self.repo)
            .output()
            .unwrap()
    }

    /// The claude agent flow runs, as config.toml lines.
    fn agent(&self, lines: &str) {
        fs::create_dir_all(&self.home).unwrap();
        fs::write(
            self.home.join("no-config.toml"),
            format!("[agents.claude]\n{lines}\n[agents.master]\n{lines}\n"),
        )
        .unwrap();
    }

    /// `flow server` in the repo, once it listens.
    fn server(&self) -> Running {
        let server = saaio_command(&self.home)
            .arg("server")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .current_dir(&self.repo)
            .spawn()
            .unwrap();
        let server = Running(server);
        wait_for_socket(&self.home.join("server.sock"));
        server
    }

    /// The flow branches in the repo, sorted.
    fn branches(&self) -> Vec<String> {
        let output = Command::new("git")
            .arg("-C")
            .arg(&self.repo)
            .args(["branch", "--list", "flow/*", "--format=%(refname:short)"])
            .output()
            .unwrap();
        String::from_utf8(output.stdout)
            .unwrap()
            .lines()
            .map(String::from)
            .collect()
    }

    /// A commit in the flow's worktree, on its branch only.
    fn commit_in(&self, name: &str) {
        let worktree = self.repo.join(".flow/worktrees").join(name);
        fs::write(worktree.join("work.txt"), "done").unwrap();
        git(&worktree, &["add", "work.txt"]);
        git(
            &worktree,
            &[
                "-c",
                "user.email=t@t",
                "-c",
                "user.name=t",
                "commit",
                "-q",
                "-m",
                "work",
            ],
        );
    }

    fn hook(&self, name: &str, script: &str) {
        let dir = self.repo.join(".flow");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(name), script).unwrap();
        set_executable(&dir.join(name));
    }
}

fn set_executable(file: &Path) {
    fs::set_permissions(file, fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn start_makes_a_worktree_runs_setup_and_leaves_the_worker_to_flow_ui() {
    let lab = Lab::new();
    lab.hook(
        "setup",
        "#!/bin/sh\necho \"slot $SAAIO_SLOT\" > setup-ran\n",
    );

    let output = lab.start(&["SHOP-217 add CSV export"]);

    let text = stdout(&output);
    assert!(output.status.success(), "{text}");
    let worktree = lab.repo.join(".flow/worktrees/shop-217");
    assert!(text.starts_with("flow: shop-217\n"), "{text}");
    assert!(
        text.contains(&format!("worktree: {}", worktree.display())),
        "{text}"
    );
    assert!(
        text.contains(
            "branch: flow/shop-217\nslot: 1\nsetup: ran\nagent: claude\npane: when flow ui runs"
        ),
        "{text}"
    );
    assert!(text.contains("Run `flow ui` to open the worker"), "{text}");
    assert_eq!(
        fs::read_to_string(worktree.join("setup-ran"))
            .unwrap()
            .trim(),
        "slot 1"
    );

    let board: serde_json::Value =
        serde_json::from_slice(&saaio(&lab.home, &["status", "--json"]).stdout).unwrap();
    assert_eq!(
        board["flows"][0]["workspace"]["window"],
        serde_json::Value::Null
    );
    assert_eq!(board["flows"][0]["workspace"]["slot"], 1);
    let log = fs::read_dir(lab.home.join("flows"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.extension().is_some_and(|ext| ext == "jsonl"))
        .unwrap();
    assert!(
        fs::read_to_string(log)
            .unwrap()
            .contains(r#""phases":["plan","implement","review","merge-request"]"#),
        "start records the workflow"
    );
}

#[test]
fn start_hands_the_worker_to_a_running_flow_server() {
    let lab = Lab::new();
    lab.agent(
        r#"command = ["cat"]
prompt = ""
resume = ["cat"]"#,
    );
    let mut server = lab.server();

    let output = lab.start(&["SHOP-217 add CSV export"]);

    let text = stdout(&output);
    assert!(text.contains("pane: opening"), "{text}");
    assert!(
        text.contains("flow ui opens shop-217 in its sidebar"),
        "{text}"
    );
    let hosted = || {
        let board: serde_json::Value =
            serde_json::from_slice(&saaio(&lab.home, &["status", "--json"]).stdout).unwrap();
        board["flows"][0]["workspace"]["window"] == "flow-ui"
    };
    wait_until(
        Duration::from_secs(5),
        "the server never opened the worker",
        hosted,
    );
    assert!(lab.run(&["ui", "--stop"]).status.success());
    assert!(server.wait().unwrap().success());
}

#[test]
fn status_says_where_a_flows_code_is_and_whether_its_worktree_is_left() {
    let lab = Lab::new();
    assert!(lab.start(&["SHOP-217 add CSV export"]).status.success());
    let worktree = lab.repo.join(".flow/worktrees/shop-217");
    assert!(
        lab.run(&["report", "--flow", "shop-217", "ended"])
            .status
            .success()
    );

    let ended = stdout(&lab.run(&["status", "shop-217"]));

    assert!(
        ended.contains(&format!(
            "branch: flow/shop-217\nworktree: {}\nworktree_exists: true\n",
            worktree.display()
        )),
        "{ended}"
    );
    assert!(ended.contains("C-space r in flow ui resumes it"), "{ended}");
    let config = lab.home.join("no-config.toml");
    fs::write(&config, "[ui]\nprefix = \"C-b\"\n").unwrap();
    let ended = stdout(&lab.run(&["status", "shop-217"]));
    assert!(ended.contains("C-b r in flow ui resumes it"), "{ended}");
    fs::write(&config, "[ui]\nprefix = \"C-c\"\n").unwrap();
    let broken = lab.run(&["status", "shop-217"]);
    assert!(
        broken.status.success(),
        "a config flow ui refuses leaves status working"
    );
    assert!(stdout(&broken).contains("C-space r in flow ui resumes it"));
    fs::remove_file(&config).unwrap();
    git(
        &lab.repo,
        &["worktree", "remove", worktree.to_str().unwrap()],
    );
    let gone = stdout(&lab.run(&["status", "shop-217"]));
    assert!(gone.contains("worktree_exists: false"), "{gone}");
    assert!(gone.contains("its commits stay on flow/shop-217"), "{gone}");
}

#[test]
fn after_a_crash_the_server_resumes_the_master_and_every_worker() {
    let lab = Lab::new();
    let log = lab.home.join("agents.log");
    lab.agent(&format!(
        r#"command = ["sh", "-c", "echo fresh {{name}} >> {log}; exec cat"]
prompt = ""
resume = ["sh", "-c", "echo resumed {{name}} >> {log}; exec cat"]"#,
        log = log.display()
    ));
    let lines = || {
        let mut lines: Vec<String> = fs::read_to_string(&log)
            .unwrap_or_default()
            .lines()
            .map(String::from)
            .collect();
        lines.sort();
        lines
    };
    let wait_for = |count: usize| {
        wait_until(Duration::from_secs(10), "the agents never all ran", || {
            lines().len() >= count
        });
    };
    let mut server = lab.server();
    assert!(lab.start(&["SHOP-1 first"]).status.success());
    assert!(lab.start(&["SHOP-2 second"]).status.success());
    wait_for(3);
    assert_eq!(lines(), ["fresh master", "fresh shop-1", "fresh shop-2"]);

    server.kill().unwrap();
    server.wait().unwrap();
    let mut again = lab.server();
    wait_for(6);

    assert_eq!(
        lines()[3..],
        ["resumed master", "resumed shop-1", "resumed shop-2"]
    );
    assert!(lab.run(&["ui", "--stop"]).status.success());
    assert!(again.wait().unwrap().success());
}

#[test]
fn the_worker_gets_the_whole_request_however_long() {
    let lab = Lab::new();
    let prompts = lab.home.join("prompts");
    fs::create_dir_all(&prompts).unwrap();
    lab.agent(&format!(
        r#"command = ["sh", "-c", "printf %s \"$1\" > {dir}/{{name}}; exec cat", "sh", "{{prompt}}"]
prompt = "{{request}}"
resume = ["cat"]"#,
        dir = prompts.display()
    ));
    let request = format!("SHOP-300 {} the tail", "word ".repeat(120));
    let mut server = lab.server();

    assert!(lab.start(&[&request]).status.success());

    let prompt = || fs::read_to_string(prompts.join("shop-300")).unwrap_or_default();
    wait_until(Duration::from_secs(10), "the worker never ran", || {
        !prompt().is_empty()
    });
    assert_eq!(prompt(), request);
    let board: serde_json::Value =
        serde_json::from_slice(&saaio(&lab.home, &["status", "--json"]).stdout).unwrap();
    assert_eq!(board["flows"][0]["request"], request.as_str());
    assert!(lab.run(&["ui", "--stop"]).status.success());
    assert!(server.wait().unwrap().success());
}

/// Runs `flow hook claude-code` with a hook payload on stdin, as Claude Code does.
fn hook(home: &Path, flow_id: Option<&str>, payload: &str) -> Output {
    hook_with(home, &["claude-code"], flow_id, payload)
}

/// Runs `flow hook <args>` with a hook payload on stdin.
fn hook_with(home: &Path, args: &[&str], flow_id: Option<&str>, payload: &str) -> Output {
    feed(hook_command(home, args, flow_id), payload)
}

/// `flow hook <args>` for the flow `flow_id`, not yet run.
fn hook_command(home: &Path, args: &[&str], flow_id: Option<&str>) -> Command {
    let mut command = saaio_command(home);
    command.arg("hook").args(args);
    if let Some(id) = flow_id {
        command.env("SAAIO_FLOW_ID", id);
    }
    command
}

/// Runs `command` with `payload` on stdin.
fn feed(mut command: Command, payload: &str) -> Output {
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(payload.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

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

/// What `flow send` would append: the master's message to the flow's worker.
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
    let prompt = r#"{"session_id":"01a1","hook_event_name":"UserPromptSubmit","prompt":"go"}"#;
    let stop = r#"{"session_id":"01a1","hook_event_name":"Stop","last_assistant_message":"Tabs or spaces?"}"#;
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
    assert!(handed.stdout.is_empty());
    let news = String::from_utf8_lossy(&handed.stderr);
    assert!(news.contains("message from the flow master:"), "{news}");
    assert!(news.contains("Tabs."), "{news}");
    let detail = stdout(&saaio(home.path(), &["status", "shop-221", "--json"]));
    assert!(detail.contains(r#""state": "working""#), "{detail}");
    assert!(
        detail.contains(r#""id": "01a1""#),
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
    let prompt = r#"{"session_id":"7f3c","hook_event_name":"UserPromptSubmit","prompt":"go"}"#;
    let stop = r#"{"session_id":"7f3c","hook_event_name":"Stop","last_assistant_message":"Tabs or spaces?","background_tasks":[]}"#;
    let compact = |event: &str| {
        format!(r#"{{"session_id":"7f3c","hook_event_name":"{event}","trigger":"manual"}}"#)
    };

    assert!(hook(home.path(), Some(&id), prompt).status.success());
    assert_eq!(fs::read_to_string(&busy).unwrap(), "turn");
    message_for(home.path(), &id, "Tabs.");
    let stopped = hook(home.path(), Some(&id), stop);
    assert!(stopped.status.success(), "flow wait wakes it, not its Stop");
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
fn a_codex_master_records_its_session_and_reports_nothing() {
    let home = TempDir::new().unwrap();
    let id = new_flow(home.path(), "shop-221");
    let prompt = r#"{"session_id":"02b2","hook_event_name":"UserPromptSubmit","prompt":"status?"}"#;

    let mut command = saaio_command(home.path());
    command
        .args(["hook", "codex"])
        .env("SAAIO_MASTER", "1")
        .env("SAAIO_FLOW_ID", &id)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(prompt.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();

    assert!(output.status.success());
    assert!(output.stdout.is_empty() && output.stderr.is_empty());
    assert_eq!(
        fs::read_to_string(home.path().join("waits/master.session")).unwrap(),
        "02b2"
    );
    assert!(home.path().join("waits/master.busy").exists());
    let turn = fs::read_to_string(home.path().join("waits/master.turn")).unwrap();
    assert!(turn.contains(r#""state":"working""#), "{turn}");
    let log = fs::read_to_string(home.path().join(format!("flows/{id}.jsonl"))).unwrap();
    assert!(!log.contains("02b2"), "{log}");
}

/// Runs `flow hook claude-code` as the master's pane does.
fn master_hook(home: &Path, payload: &str) -> Output {
    let mut command = hook_command(home, &["claude-code"], None);
    command.env("SAAIO_MASTER", "1");
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
        "a turn woken by flow wait runs no prompt hook, so its end must still count: {kept}"
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
    fs::write(
        bin.join("ssh"),
        "#!/bin/sh\nfor host; do :; done\nif [ \"$host\" = github-work ]; then echo 'hostname github.com'; else echo \"hostname $host\"; fi\n",
    )
    .unwrap();
    set_executable(&bin.join("ssh"));
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
    let err = String::from_utf8_lossy(&output.stderr);
    assert!(err.contains("no-config.toml is not valid"), "{err}");
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
        hook(home.path(), Some("no-such-flow"), stop),
        hook(
            home.path(),
            Some(&id),
            r#"{"hook_event_name":"Notification"}"#,
        ),
    ];

    for output in &cases {
        assert!(output.status.success());
        assert!(output.stdout.is_empty(), "{}", stdout(output));
    }
    assert!(String::from_utf8_lossy(&cases[1].stderr).contains("not JSON"));
    assert!(String::from_utf8_lossy(&cases[2].stderr).contains("no flow no-such-flow"));
    let list = stdout(&saaio(home.path(), &["status"]));
    assert!(!list.contains("waiting"), "{list}");
}

#[test]
fn start_refuses_outside_a_repository() {
    let lab = Lab::new();
    let elsewhere = TempDir::new().unwrap();

    let output = saaio_command(&lab.home)
        .args(["start", "x"])
        .current_dir(elsewhere.path())
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
fn start_refuses_a_config_with_the_top_level_agent_key() {
    let lab = Lab::new();
    fs::create_dir_all(&lab.home).unwrap();
    fs::write(lab.home.join("no-config.toml"), "agent = \"codex\"\n").unwrap();

    let output = lab.start(&["SHOP-1 old config"]);

    assert_eq!(output.status.code(), Some(1));
    let said = format!(
        "{}{}",
        stdout(&output),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(said.contains("no-config.toml is not valid"), "{said}");
    assert!(said.contains("`agent` under [worker]"), "{said}");
    assert!(claimed_slots(&lab.home).is_empty());
}

/// The slots claimed in the lab's shared folder.
fn claimed_slots(home: &Path) -> Vec<String> {
    fs::read_dir(shared(home).join("flow/slots"))
        .map(|entries| {
            entries
                .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
                .filter(|name| name.parse::<u8>().is_ok())
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn start_plain_opens_an_agent_in_any_folder_without_a_worktree_branch_or_slot() {
    let lab = Lab::new();
    let scratch = lab.dir.path().canonicalize().unwrap().join("scratch");
    fs::create_dir_all(&scratch).unwrap();

    let output = saaio_command(&lab.home)
        .args(["start", "--plain"])
        .arg(&scratch)
        .current_dir(&scratch)
        .output()
        .unwrap();

    let text = stdout(&output);
    assert!(output.status.success(), "{text}");
    assert!(
        text.starts_with(
            "flow: scratch
"
        ),
        "{text}"
    );
    assert!(
        text.contains(&format!(
            "folder: {}
agent: claude
pane: when flow ui runs",
            scratch.display()
        )),
        "{text}"
    );
    assert!(!text.contains("branch"), "{text}");
    assert!(!scratch.join(".flow").exists(), "no worktree");
    assert!(claimed_slots(&lab.home).is_empty(), "no slot");

    let board: serde_json::Value =
        serde_json::from_slice(&saaio(&lab.home, &["status", "--json"]).stdout).unwrap();
    let session = &board["flows"][0];
    assert_eq!(session["mode"], "plain");
    assert_eq!(session["state"], "idle", "it waits at an empty prompt");
    assert_eq!(session["request"], "");
    assert_eq!(session["steps"], serde_json::json!([]));
    assert_eq!(session["workspace"]["path"], scratch.display().to_string());
    assert_eq!(session["workspace"]["branch"], serde_json::Value::Null);
    assert_eq!(session["workspace"]["slot"], serde_json::Value::Null);
}

#[test]
fn start_plain_takes_a_name_and_a_folder_under_home() {
    let lab = Lab::new();
    let home = lab.dir.path().canonicalize().unwrap();
    fs::create_dir_all(home.join("scratch")).unwrap();

    let output = saaio_command(&lab.home)
        .args(["start", "--plain", "~/scratch", "--name", "Try It"])
        .env("HOME", &home)
        .current_dir(&lab.repo)
        .output()
        .unwrap();

    let text = stdout(&output);
    assert!(output.status.success(), "{text}");
    assert!(
        text.starts_with(
            "flow: try-it
"
        ),
        "{text}"
    );
    assert!(
        text.contains(&format!(
            "folder: {}
",
            home.join("scratch").display()
        )),
        "{text}"
    );
}

#[test]
fn start_plain_refuses_a_request_mode_or_type_and_a_missing_folder() {
    let lab = Lab::new();
    let scratch = lab.repo.display().to_string();

    for extra in [
        vec!["SHOP-1 x"],
        vec!["--mode", "auto"],
        vec!["--type", "feature"],
    ] {
        let mut args = vec!["--plain", &scratch];
        args.extend(extra.iter().copied());
        let output = lab.start(&args);
        assert_eq!(
            output.status.code(),
            Some(2),
            "{extra:?}: {}",
            stdout(&output)
        );
    }
    let plain_mode = lab.start(&["x", "--mode", "plain"]);
    assert_eq!(plain_mode.status.code(), Some(2), "plain is no --mode");

    let missing = lab.start(&["--plain", "nowhere"]);
    let text = stdout(&missing);
    assert_eq!(missing.status.code(), Some(1), "{text}");
    assert!(
        text.starts_with(&format!(
            "error: no folder {}",
            lab.repo.join("nowhere").display()
        )),
        "{text}"
    );
    assert!(text.contains("Name an existing folder"), "{text}");
    let file = lab.repo.join("file.txt");
    fs::write(&file, "x").unwrap();
    assert_eq!(lab.start(&["--plain", "file.txt"]).status.code(), Some(1));

    let board: serde_json::Value =
        serde_json::from_slice(&saaio(&lab.home, &["status", "--json"]).stdout).unwrap();
    assert_eq!(board["count"], 0, "nothing written");
}

#[test]
fn start_and_new_refuse_a_request_over_the_limit_before_making_anything() {
    let lab = Lab::new();
    let long = format!("SHOP-301 {}", "ä".repeat(20_000 - 9 + 1));
    let refused = |output: Output| {
        assert_eq!(output.status.code(), Some(1), "{}", stdout(&output));
        stdout(&output)
    };

    let text = refused(lab.start(&[&long]));
    assert!(
        text.contains("the request is 20001 chars, over 20000"),
        "{text}"
    );
    assert!(text.contains("name it in the request"), "{text}");
    let text = refused(lab.run(&["new", "--name", "shop-301", "--request", &long]));
    assert!(text.contains("over 20000"), "{text}");
    assert!(stdout(&lab.run(&["status"])).contains("count: 0"));
    assert!(lab.branches().is_empty());

    let fits = &long[..long.len() - "ä".len()];
    assert!(lab.start(&[fits]).status.success());
    assert_eq!(lab.branches(), ["flow/shop-301"]);
}

#[test]
fn a_failing_setup_marks_the_flow_blocked_but_still_records_the_worker() {
    let lab = Lab::new();
    lab.hook(
        "setup",
        "#!/bin/sh\necho 'port 8001 is taken' >&2\nexit 1\n",
    );

    let output = lab.start(&["SHOP-218 fix totals"]);

    let text = stdout(&output);
    assert!(output.status.success(), "{text}");
    assert!(
        text.contains("setup: \"failed: port 8001 is taken\""),
        "{text}"
    );
    assert!(text.contains("pane: when flow ui runs"), "{text}");
    let board: serde_json::Value =
        serde_json::from_slice(&saaio(&lab.home, &["status", "--json"]).stdout).unwrap();
    assert_eq!(board["flows"][0]["state"], "blocked");
    assert_eq!(
        board["flows"][0]["blocker"],
        "setup failed: port 8001 is taken"
    );
}

#[test]
fn start_says_when_every_slot_is_taken() {
    let lab = Lab::new();
    for slot in 1..=20 {
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

    let output = lab.start(&["one more"]);

    assert_eq!(output.status.code(), Some(1));
    assert!(
        stdout(&output).contains("all 20 slots are in use"),
        "{}",
        stdout(&output)
    );
    let logs = fs::read_dir(lab.home.join("flows"))
        .unwrap()
        .filter(|entry| entry.as_ref().unwrap().path().extension() == Some("jsonl".as_ref()))
        .count();
    assert_eq!(logs, 20, "the failed start forgets its flow");
}

#[test]
fn a_start_whose_worktree_fails_forgets_its_flow_and_gives_its_slot_back() {
    let lab = Lab::new();
    let worktrees = lab.repo.join(".flow/worktrees");
    fs::create_dir_all(&worktrees).unwrap();
    fs::write(worktrees.join("shop-9"), "in the way").unwrap();

    let failed = lab.start(&["SHOP-9 export"]);

    assert_eq!(failed.status.code(), Some(1), "{}", stdout(&failed));
    let logs = fs::read_dir(lab.home.join("flows")).map_or(0, Iterator::count);
    assert_eq!(logs, 0, "the failed start forgets its flow");
    assert!(
        !shared(&lab.home).join("flow/slots/1").exists(),
        "the failed start gives its slot back"
    );
}

#[test]
fn projects_never_share_a_slot_and_show_up_together() {
    let shop = Lab::new();
    let mut api = Lab::new();
    api.home = shop.dir.path().join("projects/api");
    let shared_with_shop = |lab: &Lab, args: &[&str]| {
        saaio_command(&lab.home)
            .args(args)
            .current_dir(&lab.repo)
            .env("XDG_STATE_HOME", shared(&shop.home))
            .output()
            .unwrap()
    };

    let first = shared_with_shop(&shop, &["start", "SHOP-1 one"]);
    let second = shared_with_shop(&api, &["start", "API-1 one"]);

    assert!(stdout(&first).contains("slot: 1"), "{}", stdout(&first));
    assert!(stdout(&second).contains("slot: 2"), "{}", stdout(&second));
    let all = stdout(&shared_with_shop(&shop, &["status", "--all"]));
    assert!(all.contains("count: 2"), "{all}");
    assert!(all.contains("  api,api-1,"), "{all}");
    let json: serde_json::Value =
        serde_json::from_slice(&shared_with_shop(&api, &["status", "--all", "--json"]).stdout)
            .unwrap();
    let names: Vec<&str> = json["projects"]
        .as_array()
        .unwrap()
        .iter()
        .map(|project| project["name"].as_str().unwrap())
        .collect();
    assert_eq!(names.len(), 2, "{json}");
    assert!(names.contains(&"api"), "{json}");
}

#[test]
fn a_failed_start_gives_its_slot_back() {
    let lab = Lab::new();
    fs::create_dir_all(lab.repo.join(".flow/worktrees/shop-1")).unwrap();
    fs::write(lab.repo.join(".flow/worktrees/shop-1/x"), "in the way").unwrap();

    let failed = lab.start(&["SHOP-1 one"]);
    let next = lab.start(&["SHOP-2 two"]);

    assert_eq!(failed.status.code(), Some(1), "{}", stdout(&failed));
    assert!(stdout(&next).contains("slot: 1"), "{}", stdout(&next));
    let list = stdout(&lab.run(&["status"]));
    assert!(!list.contains("shop-1,"), "{list}");
}

#[test]
fn clear_tears_down_and_removes_clean_worktrees_but_keeps_uncommitted_work() {
    let lab = Lab::new();
    lab.hook(
        "teardown",
        "#!/bin/sh\necho \"$SAAIO_FLOW_NAME\" >> \"$SAAIO_REPO/torn-down\"\n",
    );
    assert!(lab.start(&["SHOP-1 clean one"]).status.success());
    assert!(lab.start(&["SHOP-2 dirty one"]).status.success());
    assert!(lab.start(&["SHOP-3 still running"]).status.success());
    let worktrees = lab.repo.join(".flow/worktrees");
    fs::write(worktrees.join("shop-2/work.txt"), "unsaved").unwrap();
    for name in ["shop-1", "shop-2"] {
        assert!(
            saaio(&lab.home, &["report", "--flow", name, "ended"])
                .status
                .success()
        );
    }

    let output = saaio(&lab.home, &["clear"]);

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
    let mut torn: Vec<String> = fs::read_to_string(lab.repo.join("torn-down"))
        .unwrap()
        .lines()
        .map(String::from)
        .collect();
    torn.sort();
    assert_eq!(torn, vec!["shop-1", "shop-2"]);
    assert_eq!(
        lab.branches(),
        vec!["flow/shop-2", "flow/shop-3"],
        "a cleared flow's branch goes when no commit of it is lost"
    );
}

#[test]
fn remove_tears_down_and_forgets_a_running_flow() {
    let lab = Lab::new();
    lab.hook(
        "teardown",
        "#!/bin/sh\necho \"$SAAIO_FLOW_NAME\" >> \"$SAAIO_REPO/torn-down\"\n",
    );
    assert!(lab.start(&["SHOP-1 first try"]).status.success());
    assert!(lab.start(&["SHOP-2 keep me"]).status.success());

    let output = lab.run(&["remove", "shop-1"]);

    let text = stdout(&output);
    assert!(output.status.success(), "{text}");
    assert!(
        text.starts_with("removed: shop-1\npane: none\nworktree: removed\nbranch: deleted\n"),
        "{text}"
    );
    assert!(!lab.repo.join(".flow/worktrees/shop-1").exists());
    assert_eq!(
        fs::read_to_string(lab.repo.join("torn-down"))
            .unwrap()
            .trim(),
        "shop-1"
    );
    let list = stdout(&saaio(&lab.home, &["status"]));
    assert!(
        list.contains("count: 1") && list.contains("shop-2"),
        "{list}"
    );
}

#[test]
fn remove_forgets_a_plain_session_and_leaves_its_folder_alone() {
    let lab = Lab::new();
    lab.hook("teardown", "#!/bin/sh\ntouch \"$SAAIO_REPO/torn-down\"\n");
    let folder = lab.repo.join(".flow/worktrees/mine");
    fs::create_dir_all(&folder).unwrap();
    fs::write(folder.join("notes.txt"), "keep").unwrap();
    assert!(
        lab.start(&["--plain", ".flow/worktrees/mine"])
            .status
            .success()
    );

    let detail = stdout(&lab.run(&["status", "mine"]));
    assert!(
        detail.contains(&format!(
            "mode: plain\nstate: idle\nfolder: {}\n",
            folder.display()
        )),
        "{detail}"
    );
    assert!(
        ["branch:", "worktree:", "request:", "steps"]
            .iter()
            .all(|field| !detail.contains(field)),
        "{detail}"
    );

    let output = lab.run(&["remove", "mine"]);

    let text = stdout(&output);
    assert!(output.status.success(), "{text}");
    assert!(text.contains("worktree: none\nbranch: none"), "{text}");
    assert_eq!(
        fs::read_to_string(folder.join("notes.txt")).unwrap(),
        "keep"
    );
    assert!(!lab.repo.join("torn-down").exists(), "no teardown");
    assert!(stdout(&saaio(&lab.home, &["status"])).contains("count: 0"));
}

#[test]
fn start_names_the_branch_after_the_repos_convention() {
    let lab = Lab::new();
    fs::create_dir_all(lab.repo.join(".flow")).unwrap();
    fs::write(
        lab.repo.join(".flow/config.toml"),
        "[branch]\nname = \"{type}/{ticket}-{slug}\"\ntypes = [\"feature\", \"bugfix\"]\n",
    )
    .unwrap();
    git(&lab.repo, &["branch", "feature/SHOP-2-export-orders"]);

    let bugfix = lab.start(&["SHOP-1 fix totals", "--type", "bugfix"]);
    let feature = lab.start(&["SHOP-2 export orders"]);
    let unknown = lab.start(&["SHOP-3 x", "--type", "hotfix"]);

    assert!(
        stdout(&bugfix).contains("branch: bugfix/SHOP-1-fix-totals"),
        "{}",
        stdout(&bugfix)
    );
    assert!(
        stdout(&feature).contains("branch: feature/SHOP-2-export-orders-2"),
        "a taken branch gets -2: {}",
        stdout(&feature)
    );
    assert!(
        lab.repo.join(".flow/worktrees/shop-1").exists(),
        "worktrees keep the flow's name"
    );
    assert_eq!(unknown.status.code(), Some(1));
    assert!(
        stdout(&unknown).contains("no branch type hotfix; this repo has feature, bugfix"),
        "{}",
        stdout(&unknown)
    );
    assert!(
        !stdout(&lab.run(&[])).contains("shop-3"),
        "nothing was created"
    );
}

#[test]
fn start_refuses_a_type_in_a_repo_without_types() {
    let lab = Lab::new();

    let output = lab.start(&["SHOP-1 x", "--type", "bugfix"]);

    assert_eq!(output.status.code(), Some(1));
    assert!(
        stdout(&output).contains("no .flow/config.toml"),
        "{}",
        stdout(&output)
    );
}

#[test]
fn remove_keeps_a_branch_with_its_own_commits_unless_told() {
    let lab = Lab::new();
    assert!(lab.start(&["SHOP-1 first try"]).status.success());
    lab.commit_in("shop-1");

    let refused = lab.run(&["remove", "shop-1"]);

    let text = stdout(&refused);
    assert_eq!(refused.status.code(), Some(1), "{text}");
    assert!(
        text.contains("flow/shop-1 has 1 commit on no other branch"),
        "{text}"
    );
    assert!(
        text.contains("flow remove shop-1 --delete-unmerged"),
        "{text}"
    );
    assert!(
        lab.repo.join(".flow/worktrees/shop-1").exists(),
        "nothing touched"
    );
    assert!(stdout(&lab.run(&[])).contains("shop-1"));

    let deleted = lab.run(&["remove", "shop-1", "--delete-unmerged"]);

    let text = stdout(&deleted);
    assert!(deleted.status.success(), "{text}");
    assert!(text.contains("branch: deleted"), "{text}");
    assert!(lab.branches().is_empty());
}

#[test]
fn remove_refuses_uncommitted_work_unless_forced() {
    let lab = Lab::new();
    assert!(lab.start(&["SHOP-1 dirty"]).status.success());
    let worktree = lab.repo.join(".flow/worktrees/shop-1");
    fs::write(worktree.join("work.txt"), "unsaved").unwrap();

    let refused = lab.run(&["remove", "shop-1"]);
    let text = stdout(&refused);
    assert_eq!(refused.status.code(), Some(1), "{text}");
    assert!(
        text.starts_with("error: shop-1 has uncommitted work"),
        "{text}"
    );
    assert!(text.contains("flow remove shop-1 --force"), "{text}");
    assert!(worktree.exists());
    assert!(stdout(&lab.run(&[])).contains("shop-1"));

    let forced = lab.run(&["remove", "shop-1", "--force"]);
    assert!(
        stdout(&forced).contains("worktree: removed"),
        "{}",
        stdout(&forced)
    );
    assert!(!worktree.exists());
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
        "[agents.claude]\ncommand = [\"cat\"]\nprompt = \"\"\nresume = [\"cat\"]\n\n[agents.master]\ncommand = [\"cat\"]\nprompt = \"\"\nresume = [\"cat\"]\n",
    )
    .unwrap();
    let socket = home.path().join("server.sock");
    let log = home.path().join("server.log");

    let none = saaio(home.path(), &["ui", "--stop"]);
    assert!(none.status.success());
    assert_eq!(stdout(&none).trim(), "no flow server runs");

    let server = saaio_command(home.path())
        .arg("server")
        .stdout(Stdio::null())
        .stderr(
            fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&log)
                .unwrap(),
        )
        .env("SAAIO_CONFIG", &config)
        .spawn()
        .unwrap();
    let server_pid = server.id();
    let mut server = Running(server);
    wait_for_socket(&socket);

    let stopped = saaio(home.path(), &["ui", "--stop"]);

    assert_eq!(
        stdout(&stopped).trim(),
        "stopped the flow server and every session in it"
    );
    assert!(server.wait().unwrap().success());
    assert!(!socket.exists());
    assert!(!home.path().join("master.json").exists());

    let lines = fs::read_to_string(&log).unwrap();
    let said = |text: &str| {
        lines
            .lines()
            .find(|line| line.contains(text))
            .unwrap_or_else(|| panic!("no line with {text:?} in\n{lines}"))
    };
    let pid_of = |line: &str| line.split(['[', ']']).nth(1).unwrap().to_string();
    let server_said = format!("[{server_pid}] ");
    assert!(said("started (flow ").contains(&server_said));
    let stopper = said(&format!("sends SIGTERM to pid {server_pid}"));
    assert!(stopper.contains("flow ui --stop (flow "), "{stopper}");
    let stopping = said("stopping: SIGTERM from pid ");
    assert!(stopping.contains(&server_said), "{stopping}");
    assert!(
        stopping.ends_with(&format!("from pid {}", pid_of(stopper))),
        "{lines}"
    );
    assert!(said("] stopped").contains(&server_said));
}

#[test]
fn ui_refuses_inside_a_worker_on_its_own_store_and_points_to_a_scratch_home() {
    let home = TempDir::new().unwrap();
    let id = new_flow(home.path(), "shop-221");

    for args in [&["ui"][..], &["ui", "--stop"]] {
        let refused = saaio_in(home.path(), &id, args);
        assert_eq!(refused.status.code(), Some(1), "{args:?}");
        let text = stdout(&refused);
        assert!(
            text.contains("would take over the user's flow ui"),
            "{text}"
        );
        assert!(text.contains("SAAIO_HOME"), "{text}");
    }

    let scratch = TempDir::new().unwrap();
    let elsewhere = saaio_in(scratch.path(), &id, &["ui", "--stop"]);
    assert!(elsewhere.status.success(), "{}", stdout(&elsewhere));
    assert_eq!(stdout(&elsewhere).trim(), "no flow server runs");
}

#[test]
fn ui_refuses_a_prefix_the_panes_need_before_it_starts_anything() {
    let home = TempDir::new().unwrap();
    let config = home.path().join("no-config.toml");
    fs::write(&config, "[ui]\nprefix = \"C-c\"\n").unwrap();

    let refused = saaio(home.path(), &["ui"]);

    assert_eq!(refused.status.code(), Some(1));
    let text = stdout(&refused);
    assert!(text.contains(&config.display().to_string()), "{text}");
    assert!(
        text.contains(
            "C-c interrupts the agent in a pane; pick another prefix, such as C-space or C-b"
        ),
        "{text}"
    );
    assert!(
        !home.path().join("server.sock").exists(),
        "no server started"
    );

    fs::write(&config, "[ui]\nprefix = \"Ctrl+b\"\n").unwrap();
    let unknown = stdout(&saaio(home.path(), &["ui"]));
    assert!(
        unknown.contains("unknown prefix \\\"Ctrl+b\\\": use C-space or C-a to C-z"),
        "{unknown}"
    );
}

#[test]
fn ui_stop_ends_a_server_that_speaks_another_protocol() {
    let home = TempDir::new().unwrap();
    let socket = home.path().join("server.sock");
    let listen = "import socket, sys, time\n\
        server = socket.socket(socket.AF_UNIX)\n\
        server.bind(sys.argv[1])\n\
        server.listen()\n\
        time.sleep(60)\n";
    let stranger = Command::new("python3")
        .args(["-c", listen])
        .arg(&socket)
        .spawn()
        .unwrap();
    let mut stranger = Running(stranger);
    wait_for_socket(&socket);

    let stopped = saaio(home.path(), &["ui", "--stop"]);

    assert_eq!(
        stdout(&stopped).trim(),
        "stopped the flow server and every session in it"
    );
    assert!(stranger.wait().unwrap().signal().is_some());
}

#[test]
fn the_server_command_stays_out_of_the_help() {
    let home = TempDir::new().unwrap();

    let help = stdout(&saaio(home.path(), &["--help"]));

    assert!(help.contains("ui"), "{help}");
    assert!(!help.contains("server"), "{help}");
}

fn seed_worker(home: &Path, id: &str, name: &str, last: &str) {
    let started = format!(
        r#"{{"v":1,"at":1000,"kind":"started","name":"{name}","request":"x","mode":"auto"}}"#
    );
    let workspace = format!(
        r#"{{"v":1,"at":1500,"kind":"workspace","path":"/repo/.flow/worktrees/{name}","branch":"flow/{name}","slot":1,"window":"flow-ui","agent":"claude"}}"#
    );
    seed(home, id, &[&started, &workspace, last]);
}

#[test]
fn send_puts_the_masters_message_into_the_flows_log() {
    let home = TempDir::new().unwrap();
    seed_worker(
        home.path(),
        "7f3c",
        "shop-217",
        r#"{"v":1,"at":2000,"kind":"waiting","question":"Which delimiter?"}"#,
    );

    let output = saaio(home.path(), &["send", "shop-217", "Semicolons."]);

    assert!(output.status.success(), "{}", stdout(&output));
    assert!(stdout(&output).starts_with("sent: shop-217\nstate: waiting\n"));
    let log = fs::read_to_string(home.path().join("flows/7f3c.jsonl")).unwrap();
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
            None => saaio(home.path(), args),
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
            r#"{"v":1,"at":1500,"kind":"workspace","path":"/tmp","branch":null,"slot":null,"window":"flow-ui","agent":"claude"}"#,
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
        "7f3c",
        "shop-217",
        r#"{"v":1,"at":2000,"kind":"idle","note":"plan done"}"#,
    );
    fs::write(home.path().join("no-config.toml"), "agent = [\n").unwrap();

    let sent = saaio(home.path(), &["send", "shop-217", "Tabs."]);
    let typed = saaio(home.path(), &["send", "--type", "shop-217", "/compact"]);

    assert!(sent.status.success(), "{}", stdout(&sent));
    assert_eq!(typed.status.code(), Some(1), "{}", stdout(&typed));
    assert!(
        stdout(&typed).contains("no-config.toml is not valid"),
        "{}",
        stdout(&typed)
    );
}

#[test]
fn send_type_queues_lines_in_order_in_the_flows_log() {
    let home = TempDir::new().unwrap();
    seed_worker(
        home.path(),
        "7f3c",
        "shop-217",
        r#"{"v":1,"at":2000,"kind":"idle","note":"plan done"}"#,
    );

    let first = saaio(home.path(), &["send", "--type", "shop-217", "/compact"]);
    let second = saaio(
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
    let log = fs::read_to_string(home.path().join("flows/7f3c.jsonl")).unwrap();
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
    fs::write(home.path().join("waits/7f3c.typed"), "2").unwrap();
    let after = saaio(home.path(), &["send", "--type", "shop-217", "/review"]);
    assert!(stdout(&after).contains("ahead: 0"), "{}", stdout(&after));
}

#[test]
fn send_type_refuses_more_than_one_line_and_an_agent_without_turns() {
    let home = TempDir::new().unwrap();
    seed_worker(
        home.path(),
        "7f3c",
        "shop-217",
        r#"{"v":1,"at":2000,"kind":"working"}"#,
    );
    seed(
        home.path(),
        "aaaa",
        &[
            r#"{"v":1,"at":1000,"kind":"started","name":"old-cli","request":"x","mode":"auto"}"#,
            r#"{"v":1,"at":1500,"kind":"workspace","path":"/repo","branch":"flow/old-cli","slot":2,"window":"flow-ui","agent":"aider"}"#,
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
            && lines.contains("Queue one `flow send --type` per line"),
        "{lines}"
    );
    let aider = refused(&["send", "--type", "old-cli", "/compact"]);
    assert!(
        aider.contains("old-cli runs aider, which reports no turns to flow")
            && aider.contains("Use `flow send` for a message"),
        "{aider}"
    );
    assert!(refused(&["send", "--type", "shop-217", "  "]).contains("nothing to send"));
    let log = fs::read_to_string(home.path().join("flows/7f3c.jsonl")).unwrap();
    assert!(!log.contains("typed"), "{log}");
}

fn waiter(home: &Path, flow_id: Option<&str>, args: &[&str]) -> Running {
    let mut command = saaio_command(home);
    command
        .arg("wait")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(id) = flow_id {
        command.env("SAAIO_FLOW_ID", id);
    }
    Running(command.spawn().expect("the flow binary runs"))
}

/// The waiter's exit code, stdout and stderr once it ends; None while it still runs after `limit`.
fn ended(child: &mut Child, limit: Duration) -> Option<(i32, String, String)> {
    let deadline = Instant::now() + limit;
    while Instant::now() < deadline {
        if let Some(status) = child.try_wait().unwrap() {
            let mut out = String::new();
            let mut err = String::new();
            child
                .stdout
                .take()
                .unwrap()
                .read_to_string(&mut out)
                .unwrap();
            child
                .stderr
                .take()
                .unwrap()
                .read_to_string(&mut err)
                .unwrap();
            return Some((status.code().unwrap_or(-1), out, err));
        }
        thread::sleep(Duration::from_millis(50));
    }
    None
}

#[test]
fn a_workers_wait_ends_with_the_masters_message_once() {
    let home = TempDir::new().unwrap();
    seed_worker(
        home.path(),
        "7f3c",
        "shop-217",
        r#"{"v":1,"at":2000,"kind":"waiting","question":"Which delimiter?"}"#,
    );
    let short = Duration::from_millis(1200);
    let mut first = waiter(home.path(), Some("7f3c"), &[]);
    assert!(ended(&mut first, short).is_none(), "nothing to deliver yet");

    saaio(home.path(), &["send", "shop-217", "Semicolons."]);

    let (code, out, err) = ended(&mut first, Duration::from_secs(5)).expect("woken");
    assert_eq!(code, 2);
    assert_eq!(out, "");
    assert_eq!(
        err.trim_end(),
        "message from the flow master:\n  Semicolons."
    );
    assert_eq!(
        fs::read_to_string(home.path().join("waits/7f3c.busy")).unwrap(),
        "turn",
        "the turn it wakes runs no prompt hook"
    );

    let mut again = waiter(home.path(), Some("7f3c"), &[]);
    assert!(ended(&mut again, short).is_none(), "delivered once only");
    let replacing = waiter(home.path(), Some("7f3c"), &[]);
    let (code, _, err) = ended(&mut again, Duration::from_secs(5)).expect("replaced");
    assert_eq!((code, err.as_str()), (0, ""));
    drop(replacing);
}

#[test]
fn the_masters_wait_ends_with_the_flows_that_stopped() {
    let home = TempDir::new().unwrap();
    seed_worker(
        home.path(),
        "7f3c",
        "shop-217",
        r#"{"v":1,"at":2000,"kind":"waiting","question":"Which delimiter?"}"#,
    );
    seed_worker(
        home.path(),
        "8a1b",
        "shop-218",
        r#"{"v":1,"at":2000,"kind":"working"}"#,
    );

    let mut master = waiter(home.path(), None, &["--master"]);
    let (code, _, err) = ended(&mut master, Duration::from_secs(6)).expect("woken");

    assert_eq!(code, 2);
    assert!(
        err.starts_with(
            "changed[1]{name,state,mode,says}:\n  shop-217,waiting,auto,Which delimiter?\n"
        ),
        "{err}"
    );

    let mut again = waiter(home.path(), None, &["--master"]);
    assert!(
        ended(&mut again, Duration::from_secs(3)).is_none(),
        "it saw shop-217 already"
    );
    fs::write(
        home.path().join("flows/8a1b.jsonl"),
        fs::read_to_string(home.path().join("flows/8a1b.jsonl")).unwrap()
            + r#"{"v":1,"at":3000,"kind":"idle","note":"Plan is ready."}"#
            + "\n",
    )
    .unwrap();
    let (code, _, err) = ended(&mut again, Duration::from_secs(6)).expect("woken again");
    assert_eq!(code, 2);
    assert!(err.contains("shop-218,idle,auto,Plan is ready."), "{err}");
    assert!(!err.contains("shop-217"), "{err}");
}

#[test]
fn a_wait_with_nobody_to_wait_for_ends_quietly() {
    let home = TempDir::new().unwrap();
    let mut nobody = waiter(home.path(), None, &[]);
    assert_eq!(
        ended(&mut nobody, Duration::from_secs(3)),
        Some((0, String::new(), String::new()))
    );
    let mut gone = waiter(home.path(), Some("nope"), &[]);
    assert_eq!(
        ended(&mut gone, Duration::from_secs(3)),
        Some((0, String::new(), String::new()))
    );
}
