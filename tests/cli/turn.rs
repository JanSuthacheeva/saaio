//! Tests of taking turns: a heavy command waits while another flow of the project runs one

use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use tempfile::TempDir;

use crate::support::*;

/// Tells the projects of tests running at once apart: their flow ids and heavy command.
static PROJECTS: AtomicU32 = AtomicU32::new(0);

/// A store with flows a and b in one repository that lists a heavy command of its own (`pintN`,
/// so no other test's process counts), both with this test process as their agent, as the server
/// keeps the agent a pane started.
struct Project {
    home: TempDir,
    repo: TempDir,
    a: String,
    b: String,
    heavy: String,
}

impl Project {
    fn new() -> Project {
        let n = PROJECTS.fetch_add(1, Ordering::SeqCst);
        let tag = format!("{:04x}{:08x}", n, std::process::id());
        let project = Project {
            home: TempDir::new().unwrap(),
            repo: TempDir::new().unwrap(),
            a: format!("0a000000-0000-4000-8000-{tag}"),
            b: format!("0b000000-0000-4000-8000-{tag}"),
            heavy: format!("pint{tag}"),
        };
        let home = project.home.path();
        fs::create_dir_all(project.repo.path().join(".saaio")).unwrap();
        fs::write(
            project.repo.path().join(".saaio/config.toml"),
            format!(
                "[exclusive]\ncommands = [\"artisan test\", \"{}\"]\n",
                project.heavy
            ),
        )
        .unwrap();
        for (id, name) in [(&project.a, "shop-a"), (&project.b, "shop-b")] {
            let started = format!(
                r#"{{"v":1,"at":1000,"kind":"started","name":"{name}","request":"x","mode":"auto"}}"#
            );
            let workspace = format!(
                r#"{{"v":1,"at":1500,"kind":"workspace","path":"{}/.saaio/worktrees/{name}","branch":"flow/{name}","slot":1,"window":"saaio-ui","agent":"claude"}}"#,
                project.repo.path().display()
            );
            let working = r#"{"v":1,"at":2000,"kind":"working"}"#;
            seed(home, id, &[&started, &workspace, working]);
            key(home, id);
            fs::write(
                home.join(format!("waits/{id}.agent")),
                std::process::id().to_string(),
            )
            .unwrap();
        }
        project
    }

    fn log(&self, id: &str) -> String {
        fs::read_to_string(self.home.path().join(format!("flows/{id}.jsonl"))).unwrap()
    }

    fn server_log(&self) -> String {
        fs::read_to_string(self.home.path().join("server.log")).unwrap_or_default()
    }

    /// Waits until `id`'s log has `text`; panics with the logs past 30 s.
    fn wait_for(&self, id: &str, text: &str) {
        let deadline = Instant::now() + Duration::from_secs(30);
        while !self.log(id).contains(text) {
            assert!(
                Instant::now() < deadline,
                "no {text} in\n{}\nserver.log:\n{}",
                self.log(id),
                self.server_log()
            );
            thread::sleep(Duration::from_millis(50));
        }
    }

    /// `saaio hook claude-code` for `id` before a call that runs the heavy command through Sail,
    /// started but not awaited.
    fn hook(&self, id: &str, config: Option<&Path>) -> Child {
        let line = format!("./vendor/bin/sail {} --test", self.heavy);
        self.hook_for(id, &line, config)
    }

    fn hook_for(&self, id: &str, line: &str, config: Option<&Path>) -> Child {
        let mut command = hook_command(self.home.path(), &["claude-code"], Some(id));
        if let Some(config) = config {
            command.env("SAAIO_CONFIG", config);
        }
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let payload = serde_json::json!({
            "hook_event_name": "PreToolUse",
            "tool_name": "Bash",
            "tool_input": { "command": line },
        });
        child
            .stdin
            .take()
            .unwrap()
            .write_all(payload.to_string().as_bytes())
            .unwrap();
        child
    }

    /// The heavy command running below this test, as the agent's tool starts it.
    fn run_heavy(&self) -> Running {
        let script = self.repo.path().join(&self.heavy);
        if !script.exists() {
            executable(&script, "#!/bin/sh\nsleep 60\n");
        }
        Running(Command::new(&script).stdin(Stdio::null()).spawn().unwrap())
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        kill_watch(&self.a);
        kill_watch(&self.b);
    }
}

/// Kills the `saaio turn` that keeps `id`'s turn; whether there was one.
fn kill_watch(id: &str) -> bool {
    Command::new("pkill")
        .args(["-f", &format!("turn --flow {id}")])
        .status()
        .unwrap()
        .success()
}

/// Waits for the hook to end with exit 0 and nothing on stdout; panics past 30 s.
fn ends_silently(mut child: Child) {
    let (code, out, err) = ended(&mut child, Duration::from_secs(30)).expect("the hook ends");
    assert_eq!((code, out.as_str()), (0, ""), "{err}");
}

/// Whether `child` still runs after `wait`: only to show that nothing happens.
fn still_runs(child: &mut Child, wait: Duration) -> bool {
    thread::sleep(wait);
    child.try_wait().unwrap().is_none()
}

#[test]
fn a_heavy_call_waits_while_another_flow_runs_one_and_goes_on_after_it() {
    let project = Project::new();
    ends_silently(project.hook(&project.a, None));
    let heavy = project.run_heavy();

    let mut waiting = project.hook(&project.b, None);
    project.wait_for(
        &project.b,
        &format!(
            r#""kind":"turn-waiting","command":"{}","holder":"shop-a""#,
            project.heavy
        ),
    );
    assert!(
        still_runs(&mut waiting, Duration::from_secs(2)),
        "b waits while a's run lasts"
    );
    let status = stdout(&saaio(project.home.path(), &["status", "shop-b", "--json"]));
    assert!(status.contains(r#""holder": "shop-a""#), "{status}");

    drop(heavy);
    ends_silently(waiting);
    assert!(project.log(&project.b).contains(r#""kind":"turn-taken""#));
    let log = project.server_log();
    let ended = format!("turn watch flow {}: the run ended", project.a);
    assert!(log.contains(&ended), "{log}");
}

#[test]
fn a_flows_second_heavy_call_waits_for_its_first() {
    let project = Project::new();
    ends_silently(project.hook(&project.a, None));
    let heavy = project.run_heavy();

    let mut again = project.hook(&project.a, None);
    project.wait_for(&project.a, r#""holder":"shop-a""#);
    assert!(still_runs(&mut again, Duration::from_millis(500)));

    drop(heavy);
    ends_silently(again);
}

#[test]
fn a_killed_watch_frees_the_turn() {
    let project = Project::new();
    ends_silently(project.hook(&project.a, None));
    let _heavy = project.run_heavy();
    let mut waiting = project.hook(&project.b, None);
    project.wait_for(&project.b, "turn-waiting");
    assert!(still_runs(&mut waiting, Duration::from_millis(500)));

    assert!(kill_watch(&project.a), "no saaio turn for a");

    ends_silently(waiting);
}

#[test]
fn turned_off_a_heavy_call_never_waits() {
    let project = Project::new();
    ends_silently(project.hook(&project.a, None));
    let _heavy = project.run_heavy();
    let config = project.home.path().join("off.toml");
    fs::write(&config, "[exclusive]\nenabled = false\n").unwrap();

    ends_silently(project.hook(&project.b, Some(&config)));

    assert!(!project.log(&project.b).contains("turn-waiting"));
}

#[test]
fn a_call_that_runs_nothing_listed_never_waits() {
    let project = Project::new();
    ends_silently(project.hook(&project.a, None));
    let _heavy = project.run_heavy();

    let line = format!("cat {}.json", project.heavy);
    ends_silently(project.hook_for(&project.b, &line, None));

    assert!(!project.log(&project.b).contains("turn-waiting"));
}
