//! The status reports panes send (OSC 7501): a Claude Code turn an Esc ended

use super::*;

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
    let id = started_flow(&store, dir.path(), "csv", "claude");
    let answer = EventKind::Answer {
        text: "Plan done.".into(),
    };
    store.append(id, &LogEvent::new(2, answer)).unwrap();
    store
        .append(id, &LogEvent::new(3, EventKind::Working))
        .unwrap();
    queue(&store, id, "continue shipping");
    let (paths, handle) = start_with(dir.path(), interrupted_claude(dir.path(), "csv"));
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
    let id = started_flow(&store, dir.path(), "csv", "claude");
    store
        .append(id, &LogEvent::new(3, EventKind::Working))
        .unwrap();
    let (paths, handle) = start_with(dir.path(), interrupted_claude(dir.path(), "csv"));
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
