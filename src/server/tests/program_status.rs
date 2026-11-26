//! The status reports panes send (OSC 7501): a Claude Code turn an Esc ended, a Codex turn that
//! failed, a dialog before a Codex worker's first turn

use super::*;
use crate::program_status::Need;

const WORKING: &str = r"\033]7501;state=working:app=claude-code\033\\";
const IDLE: &str = r"\033]7501;state=idle:app=claude-code\007";

/// A Claude Code stand-in: marks itself started, then waits for `go` in its folder before it
/// reports working and then idle, as an Esc mid-turn does, and keeps what is typed in `typed`.
fn interrupted_claude(dir: &Path, name: &str) -> Agent {
    let script = dir.join(format!("{name}-claude"));
    let body = format!(
        "#!/bin/sh\ntouch started\nwhile [ ! -e go ]; do sleep 0.02; done\nprintf '{WORKING}{IDLE}'\nexec cat > typed\n"
    );
    executable(&script, &body);
    Agent {
        command: vec![script.display().to_string()],
        prompt: None,
        adapter: Some(Adapter::ClaudeCode),
        resume: Some(vec![script.display().to_string()]),
    }
}

/// The master as `cat`, the flows' agent `claude-esc`.
fn agents(flows: Agent) -> BTreeMap<String, Agent> {
    let cat = Agent {
        command: vec!["cat".into()],
        prompt: None,
        adapter: None,
        resume: Some(vec!["cat".into()]),
    };
    BTreeMap::from([
        ("claude".to_string(), cat),
        ("claude-esc".to_string(), flows),
    ])
}

fn interrupted_events(store: &Store, id: FlowId) -> usize {
    store
        .events(id)
        .unwrap()
        .iter()
        .filter(|event| {
            event.kind
                == EventKind::Idle {
                    note: "interrupted".into(),
                }
        })
        .count()
}

#[test]
fn an_esc_mid_turn_ends_the_turn_and_types_the_queued_line() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path().to_path_buf());
    let id = started_flow(&store, dir.path(), "csv", "claude-esc");
    let answer = EventKind::Answer {
        text: "Plan done.".into(),
    };
    store.append(id, &LogEvent::new(2, answer)).unwrap();
    store
        .append(id, &LogEvent::new(3, EventKind::Working))
        .unwrap();
    queue(&store, id, "continue shipping");
    let (paths, handle) =
        start_with_agents(dir.path(), agents(interrupted_claude(dir.path(), "csv")));
    let waiter = Waiter::Flow(id);
    wait_until("the worker never started", || {
        dir.path().join("csv/started").exists()
    });
    store.set_busy(&waiter, Busy::Turn).unwrap();

    fs::write(dir.path().join("csv/go"), "").unwrap();

    wait_until_typed(dir.path(), "csv", "continue shipping");
    assert_eq!(interrupted_events(&store, id), 1);
    stop(&paths, handle);
}

#[test]
fn idle_after_the_stop_hook_appends_nothing() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path().to_path_buf());
    let id = started_flow(&store, dir.path(), "csv", "claude-esc");
    store
        .append(id, &LogEvent::new(3, EventKind::Working))
        .unwrap();
    let (paths, handle) =
        start_with_agents(dir.path(), agents(interrupted_claude(dir.path(), "csv")));
    wait_until("the worker never started", || {
        dir.path().join("csv/started").exists()
    });

    fs::write(dir.path().join("csv/go"), "").unwrap();
    thread::sleep(WATCH_EVERY * 3);

    assert_eq!(interrupted_events(&store, id), 0);
    stop(&paths, handle);
}

#[test]
fn an_esc_mid_turn_in_the_master_makes_its_turn_idle() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path().to_path_buf());
    let script = dir.path().join("master-claude");
    let go = dir.path().join("go");
    let started = dir.path().join("started");
    executable(
        &script,
        &format!(
            "#!/bin/sh\ntouch '{}'\nwhile [ ! -e '{}' ]; do sleep 0.02; done\nprintf '{WORKING}{IDLE}'\nexec cat\n",
            started.display(),
            go.display()
        ),
    );
    let (paths, handle) = start_with(
        dir.path(),
        Agent {
            command: vec![script.display().to_string()],
            prompt: None,
            adapter: Some(Adapter::ClaudeCode),
            resume: Some(vec![script.display().to_string()]),
        },
    );
    wait_until("the master never started", || started.exists());
    store.set_busy(&Waiter::Master, Busy::Turn).unwrap();
    let working = MasterTurn {
        state: State::Working,
        at: 1,
    };
    store.set_master_turn(&working).unwrap();

    fs::write(&go, "").unwrap();

    wait_until("the master's turn never went idle", || {
        store.master_turn().map(|turn| turn.state) == Some(State::Idle)
    });
    assert_eq!(store.busy(&Waiter::Master), Busy::Idle);
    stop(&paths, handle);
}

/// A path under the crate's fixtures.
fn fixture(path: &str) -> String {
    format!("{}/src/{path}", env!("CARGO_MANIFEST_DIR"))
}

/// A Codex stand-in: marks itself started, waits for `go` in its folder, then replays what Codex
/// 0.172 wrote for `recording` and keeps reading.
fn recorded_codex(dir: &Path, name: &str, recording: &str) -> Agent {
    let script = dir.join(format!("{name}-codex"));
    let raw = fixture(&format!("program_status/fixtures/{recording}.raw"));
    let body = format!(
        "#!/bin/sh\ntouch started\nwhile [ ! -e go ]; do sleep 0.02; done\ncat '{raw}'\nexec cat > /dev/null\n"
    );
    executable(&script, &body);
    Agent {
        command: vec![script.display().to_string()],
        prompt: None,
        adapter: Some(Adapter::Codex),
        resume: Some(vec![script.display().to_string()]),
    }
}

fn blockers(store: &Store, id: FlowId) -> Vec<String> {
    store
        .events(id)
        .unwrap()
        .into_iter()
        .filter_map(|event| match event.kind {
            EventKind::Blocked { reason } => Some(reason),
            _ => None,
        })
        .collect()
}

#[test]
fn a_codex_turn_that_failed_blocks_with_its_error_and_ends_the_turn() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path().to_path_buf());
    let id = started_flow(&store, dir.path(), "csv", "codex-failing");
    let session = EventKind::Session {
        id: fixtures::session_id("a_codex_turn_that_failed"),
        transcript: Some(fixture("adapter/fixtures/codex/rollout_turn_failed.jsonl")),
    };
    store.append(id, &LogEvent::new(2, session)).unwrap();
    store
        .append(id, &LogEvent::new(3, EventKind::Working))
        .unwrap();
    let codex = recorded_codex(dir.path(), "csv", "codex-0.172-turn-failed");
    let agents = BTreeMap::from([("codex-failing".to_string(), codex)]);
    let (paths, handle) = start_with_agents(dir.path(), agents);
    let waiter = Waiter::Flow(id);
    wait_until("the worker never started", || {
        dir.path().join("csv/started").exists()
    });
    store.set_busy(&waiter, Busy::Turn).unwrap();

    fs::write(dir.path().join("csv/go"), "").unwrap();

    wait_until("the turn never failed", || !blockers(&store, id).is_empty());
    thread::sleep(WATCH_EVERY * 2);
    assert_eq!(
        blockers(&store, id),
        [
            "turn failed: The 'saaio-no-such-model' model is not supported when using Codex with a ChatGPT account."
        ]
    );
    assert_eq!(store.busy(&waiter), Busy::Idle);
    stop(&paths, handle);
}

#[test]
fn an_error_after_the_hooks_ended_the_turn_blocks_nothing() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path().to_path_buf());
    let id = started_flow(&store, dir.path(), "csv", "codex-failing");
    store
        .append(id, &LogEvent::new(3, EventKind::Working))
        .unwrap();
    let codex = recorded_codex(dir.path(), "csv", "codex-0.172-turn-failed");
    let agents = BTreeMap::from([("codex-failing".to_string(), codex)]);
    let (paths, handle) = start_with_agents(dir.path(), agents);
    wait_until("the worker never started", || {
        dir.path().join("csv/started").exists()
    });

    fs::write(dir.path().join("csv/go"), "").unwrap();
    thread::sleep(WATCH_EVERY * 3);

    assert!(blockers(&store, id).is_empty());
    stop(&paths, handle);
}

fn waits_on(store: &Store, id: FlowId, asked: &str) -> usize {
    store
        .events(id)
        .unwrap()
        .iter()
        .filter(
            |event| matches!(&event.kind, EventKind::Waiting { question, .. } if question == asked),
        )
        .count()
}

#[test]
fn a_sign_in_before_a_codex_workers_first_turn_waits_at_once_and_never_stalls() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path().to_path_buf());
    let id = started_flow(&store, dir.path(), "csv", "codex-sign-in");
    let script = dir.path().join("sign-in-codex");
    executable(
        &script,
        "#!/bin/sh\nprintf '\\033]7501;state=blocked:app=codex:kind=auth\\033\\\\'\nexec cat > /dev/null\n",
    );
    let codex = Agent {
        command: vec![script.display().to_string()],
        prompt: None,
        adapter: Some(Adapter::Codex),
        resume: None,
    };
    let grace = Duration::from_secs(2);
    let timing = Timing {
        first_turn_grace: grace,
        ..quick()
    };
    let agents = BTreeMap::from([("codex-sign-in".to_string(), codex)]);
    let (paths, handle) = start_timed(dir.path(), agents, timing, || 0);
    let sign_in = super::super::startup::dialog_question(Some(Need::Auth));

    wait_until("the sign-in never waited", || {
        waits_on(&store, id, sign_in) > 0
    });
    thread::sleep(grace + WATCH_EVERY * 2);

    assert_eq!(waits_on(&store, id, sign_in), 1);
    assert_eq!(
        waits_on(&store, id, super::super::startup::STALLED_START),
        0
    );
    stop(&paths, handle);
}

#[test]
fn a_failed_turn_in_the_master_makes_its_turn_idle() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path().to_path_buf());
    let script = dir.path().join("master-claude");
    let go = dir.path().join("go");
    let started = dir.path().join("started");
    let error = r"\033]7501;state=error:app=claude-code\033\\";
    executable(
        &script,
        &format!(
            "#!/bin/sh\ntouch '{}'\nwhile [ ! -e '{}' ]; do sleep 0.02; done\nprintf '{WORKING}{error}'\nexec cat\n",
            started.display(),
            go.display()
        ),
    );
    let (paths, handle) = start_with(
        dir.path(),
        Agent {
            command: vec![script.display().to_string()],
            prompt: None,
            adapter: Some(Adapter::ClaudeCode),
            resume: Some(vec![script.display().to_string()]),
        },
    );
    wait_until("the master never started", || started.exists());
    store.set_busy(&Waiter::Master, Busy::Turn).unwrap();
    let working = MasterTurn {
        state: State::Working,
        at: 1,
    };
    store.set_master_turn(&working).unwrap();

    fs::write(&go, "").unwrap();

    wait_until("the master's turn never went idle", || {
        store.master_turn().map(|turn| turn.state) == Some(State::Idle)
    });
    assert_eq!(store.busy(&Waiter::Master), Busy::Idle);
    stop(&paths, handle);
}
