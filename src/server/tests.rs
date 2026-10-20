use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixListener;
use std::path::Path;
use std::thread;

use tempfile::TempDir;

use super::*;
use crate::adapter::Adapter;
use crate::config::Agent;
use crate::event::{Event as LogEvent, EventKind};
use crate::fixtures::executable;
use crate::launch::pane::HOSTED;
use crate::proto::{self, Frame, PROTOCOL_VERSION, PaneStatus, Peer, Role};
use crate::store::{Busy, Waiter};
use crate::view::{MasterTurn, State};

/// A server whose master is `cat`, run on its own thread.
fn start(dir: &Path) -> (ServerPaths, thread::JoinHandle<Result<()>>) {
    start_with(
        dir,
        Agent {
            command: vec!["cat".into()],
            prompt: None,
            adapter: None,
            resume: Some(vec!["cat".into()]),
        },
    )
}

fn start_with(dir: &Path, agent: Agent) -> (ServerPaths, thread::JoinHandle<Result<()>>) {
    start_with_agents(dir, BTreeMap::from([("claude".to_string(), agent)]))
}

/// How long a test waits for what should happen soon; only a failing test waits this long.
const PATIENCE: Duration = Duration::from_secs(30);

/// Quick to type and to call a start stalled, never quick to give up on a typed line.
fn quick() -> Timing {
    Timing {
        quiet: Duration::from_millis(200),
        first_turn_grace: Duration::from_millis(300),
        unstarted: Duration::from_secs(30),
    }
}

/// Polls `done` until it holds; fails with `what` after PATIENCE.
fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + PATIENCE;
    while !done() {
        assert!(Instant::now() < deadline, "{what}");
        thread::sleep(Duration::from_millis(20));
    }
}

/// Reads messages until `done` holds for one; fails with `what` after PATIENCE.
fn recv_until(stream: &mut UnixStream, what: &str, mut done: impl FnMut(ServerMsg) -> bool) {
    let deadline = Instant::now() + PATIENCE;
    while !done(recv(stream)) {
        assert!(Instant::now() < deadline, "{what}");
    }
}

fn start_with_agents(
    dir: &Path,
    agents: BTreeMap<String, Agent>,
) -> (ServerPaths, thread::JoinHandle<Result<()>>) {
    start_at(dir, agents, || 0)
}

/// A server whose master is `cat`, on the clock given.
fn start_at(
    dir: &Path,
    agents: BTreeMap<String, Agent>,
    clock: fn() -> u64,
) -> (ServerPaths, thread::JoinHandle<Result<()>>) {
    start_timed(dir, agents, quick(), clock)
}

fn start_timed(
    dir: &Path,
    agents: BTreeMap<String, Agent>,
    timing: Timing,
    clock: fn() -> u64,
) -> (ServerPaths, thread::JoinHandle<Result<()>>) {
    let paths = ServerPaths {
        socket: dir.join("server.sock"),
        master: dir.join("master.json"),
        instructions: dir.join("master.md"),
        config: Some(dir.join("config/config.toml")),
        theme: Some(dir.join("config/theme.toml")),
        cwd: std::env::current_dir().unwrap(),
    };
    let thread_paths = paths.clone();
    let store = Store::at(dir.to_path_buf());
    let config = Config {
        agents,
        workflow: crate::workflow::Workflow::builtin(),
        ..Config::default()
    };
    let handle = thread::spawn(move || {
        run(
            &thread_paths,
            store,
            config,
            LaunchEnv::default(),
            &System::default(),
            timing,
            clock,
        )
    });
    wait_until("the server did not start", || {
        UnixStream::connect(&paths.socket).is_ok()
    });
    // It listens before it has written its files and read config.toml; it answers after.
    let mut ready = connect(&paths.socket, Role::Control);
    assert!(matches!(recv(&mut ready), ServerMsg::Welcome { .. }));
    (paths, handle)
}

fn peer(pid: u32) -> Peer {
    Peer {
        pid,
        cwd: "/work".into(),
        build: "test".into(),
    }
}

fn connect(socket: &Path, role: Role) -> UnixStream {
    connect_as(socket, role, peer(1))
}

fn connect_as(socket: &Path, role: Role, peer: Peer) -> UnixStream {
    let mut stream = UnixStream::connect(socket).unwrap();
    stream.set_read_timeout(Some(PATIENCE)).unwrap();
    proto::write_msg(
        &mut stream,
        &ClientMsg::Hello {
            version: PROTOCOL_VERSION,
            role,
            peer,
        },
    )
    .unwrap();
    stream
}

fn recv(stream: &mut UnixStream) -> ServerMsg {
    proto::read_msg(stream).unwrap().expect("a message")
}

fn text(frame: &Frame, row: u16) -> String {
    frame
        .rows
        .iter()
        .find(|(index, _)| *index == row)
        .map(|(_, cells)| {
            cells
                .iter()
                .map(|cell| cell.text.as_str())
                .collect::<String>()
        })
        .unwrap_or_default()
        .trim_end()
        .to_string()
}

#[test]
fn a_ui_attaches_types_into_the_master_and_sees_it() {
    let dir = TempDir::new().unwrap();
    let (paths, handle) = start(dir.path());
    let size = Size { cols: 30, rows: 6 };

    let mut ui = connect(&paths.socket, Role::Attach { size });

    assert_eq!(
        recv(&mut ui),
        ServerMsg::Welcome {
            version: PROTOCOL_VERSION
        }
    );
    let ServerMsg::Master { cwd, .. } = recv(&mut ui) else {
        panic!("expected where the master runs");
    };
    assert_eq!(cwd, std::env::current_dir().unwrap().display().to_string());
    let ServerMsg::Board(board) = recv(&mut ui) else {
        panic!("expected the board");
    };
    assert_eq!(board.count, 0);
    let ServerMsg::Panes(panes) = recv(&mut ui) else {
        panic!("expected the panes");
    };
    assert_eq!(panes[0].key, PaneKey::Master);
    assert_eq!(panes[0].status, PaneStatus::Running);
    let ServerMsg::Frame(first) = recv(&mut ui) else {
        panic!("expected a full frame");
    };
    assert!(first.full);
    assert_eq!(first.rows.len(), 6);
    assert_eq!(first.rows[0].1.len(), 30);

    proto::write_msg(&mut ui, &ClientMsg::Input(b"hello\r".to_vec())).unwrap();
    recv_until(
        &mut ui,
        "never saw the echo",
        |message| matches!(message, ServerMsg::Frame(frame) if text(&frame, 0).contains("hello")),
    );

    let mut control = connect(&paths.socket, Role::Control);
    assert!(matches!(recv(&mut control), ServerMsg::Welcome { .. }));
    proto::write_msg(&mut control, &ClientMsg::StopServer).unwrap();
    handle.join().unwrap().unwrap();
    assert!(!paths.socket.exists());
    assert!(!paths.master.exists(), "a stop ends the master for good");
}

#[test]
fn tells_the_ui_where_the_masters_turn_stands_from_its_start() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path().to_path_buf());
    let old = MasterTurn {
        state: State::Waiting,
        at: 1,
    };
    store.set_master_turn(&old).unwrap();
    let (paths, handle) = start(dir.path());
    let mut ui = connect(
        &paths.socket,
        Role::Attach {
            size: Size { cols: 30, rows: 4 },
        },
    );

    let first = loop {
        if let ServerMsg::Master { turn, .. } = recv(&mut ui) {
            break turn;
        }
    };
    assert_eq!(first, None, "a master that opens starts without a turn");
    let working = MasterTurn {
        state: State::Working,
        at: 2,
    };
    store.set_master_turn(&working).unwrap();
    recv_until(&mut ui, "the turn never reached the ui", |message| {
        let ServerMsg::Master { turn, .. } = message else {
            return false;
        };
        assert_eq!(turn, Some(working));
        true
    });

    proto::write_msg(&mut ui, &ClientMsg::StopServer).unwrap();
    handle.join().unwrap().unwrap();
}

/// A flow with a worktree at dir/<name>, as `saaio start` leaves it.
fn started_flow(store: &Store, dir: &Path, name: &str, agent: &str) -> String {
    let id = store
        .create(&LogEvent::new(
            0,
            EventKind::Started {
                name: name.into(),
                request: "x".into(),
                mode: crate::event::Mode::Auto,
                phases: None,
            },
        ))
        .unwrap();
    let path = dir.join(name);
    fs::create_dir_all(&path).unwrap();
    let workspace = EventKind::Workspace {
        path: path.display().to_string(),
        branch: Some(format!("flow/{name}")),
        slot: Some(1),
        window: None,
        agent: agent.into(),
    };
    store.append(&id, &LogEvent::new(1, workspace)).unwrap();
    id
}

fn status_of(panes: &[crate::proto::PaneInfo], id: &str) -> Option<PaneStatus> {
    let key = PaneKey::Flow(id.to_string());
    panes
        .iter()
        .find(|pane| pane.key == key)
        .map(|pane| pane.status.clone())
}

#[test]
fn a_codex_worker_that_never_starts_a_turn_waits_once() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path().to_path_buf());
    let id = started_flow(&store, dir.path(), "stalled", "codex");
    let fake_codex = dir.path().join("fake-codex");
    executable(&fake_codex, "#!/bin/sh\nsleep 30\n");
    let codex = Agent {
        command: vec![fake_codex.display().to_string()],
        prompt: None,
        adapter: Some(Adapter::Codex),
        resume: None,
    };
    let (paths, handle) =
        start_with_agents(dir.path(), BTreeMap::from([("codex".to_string(), codex)]));

    let stalls = || {
        store
            .events(&id)
            .unwrap()
            .iter()
            .filter(|event| {
                matches!(
                    &event.kind,
                    EventKind::Waiting { question, .. } if question == super::startup::STALLED_START
                )
            })
            .count()
    };
    wait_until("the start never stalled", || stalls() > 0);
    thread::sleep(WATCH_EVERY * 2);
    assert_eq!(stalls(), 1, "once");

    let mut control = connect(&paths.socket, Role::Control);
    assert!(matches!(recv(&mut control), ServerMsg::Welcome { .. }));
    proto::write_msg(&mut control, &ClientMsg::StopServer).unwrap();
    handle.join().unwrap().unwrap();
}

#[test]
fn an_exited_worker_blocks_once_with_its_code_and_final_output() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path().to_path_buf());
    let id = started_flow(&store, dir.path(), "dead", "shell");
    let shell = Agent {
        command: vec![
            "sh".into(),
            "-c".into(),
            "printf 'one\\ntwo\\n'; exit 23".into(),
        ],
        prompt: Some(String::new()),
        adapter: None,
        resume: None,
    };
    let (paths, handle) =
        start_with_agents(dir.path(), BTreeMap::from([("shell".to_string(), shell)]));

    let blocked = || {
        store
            .events(&id)
            .unwrap()
            .into_iter()
            .find_map(|event| match event.kind {
                EventKind::Blocked { reason } => Some(reason),
                _ => None,
            })
    };
    wait_until("the worker did not block", || blocked().is_some());
    let reason = blocked().unwrap();
    assert!(reason.contains("code 23"), "{reason}");
    assert!(reason.contains("one\ntwo"), "{reason}");
    thread::sleep(WATCH_EVERY * 2);
    assert_eq!(
        store
            .events(&id)
            .unwrap()
            .iter()
            .filter(|event| matches!(event.kind, EventKind::Blocked { .. }))
            .count(),
        1
    );

    let mut control = connect(&paths.socket, Role::Control);
    assert!(matches!(recv(&mut control), ServerMsg::Welcome { .. }));
    proto::write_msg(&mut control, &ClientMsg::StopServer).unwrap();
    handle.join().unwrap().unwrap();
}

#[test]
fn a_failed_resume_retries_fresh_before_blocking() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path().to_path_buf());
    let id = started_flow(&store, dir.path(), "retry", "shell");
    store
        .append(
            &id,
            &LogEvent::new(
                2,
                EventKind::Workspace {
                    path: dir.path().join("retry").display().to_string(),
                    branch: Some("flow/retry".into()),
                    slot: Some(1),
                    window: Some(HOSTED.into()),
                    agent: "shell".into(),
                },
            ),
        )
        .unwrap();
    let shell = Agent {
        command: vec!["cat".into()],
        prompt: Some(String::new()),
        adapter: None,
        resume: Some(vec!["sh".into(), "-c".into(), "exit 23".into()]),
    };
    let (paths, handle) =
        start_with_agents(dir.path(), BTreeMap::from([("shell".to_string(), shell)]));

    thread::sleep(Duration::from_millis(600));
    assert_eq!(
        store
            .events(&id)
            .unwrap()
            .iter()
            .filter(|event| matches!(event.kind, EventKind::Blocked { .. }))
            .count(),
        0
    );
    let mut control = connect(&paths.socket, Role::Control);
    assert!(matches!(recv(&mut control), ServerMsg::Welcome { .. }));
    proto::write_msg(&mut control, &ClientMsg::StopServer).unwrap();
    handle.join().unwrap().unwrap();
}

#[test]
fn an_exited_idle_worker_keeps_its_state() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path().to_path_buf());
    let id = started_flow(&store, dir.path(), "idle", "shell");
    store
        .append(
            &id,
            &LogEvent::new(
                2,
                EventKind::Idle {
                    note: String::new(),
                },
            ),
        )
        .unwrap();
    let shell = Agent {
        command: vec!["sh".into(), "-c".into(), "exit 23".into()],
        prompt: Some(String::new()),
        adapter: None,
        resume: None,
    };
    let (paths, handle) =
        start_with_agents(dir.path(), BTreeMap::from([("shell".to_string(), shell)]));

    thread::sleep(Duration::from_millis(600));
    let flow = fold::fold(&id, &store.events(&id).unwrap(), None, 3).unwrap();
    assert_eq!(flow.state, State::Idle);
    let mut control = connect(&paths.socket, Role::Control);
    assert!(matches!(recv(&mut control), ServerMsg::Welcome { .. }));
    proto::write_msg(&mut control, &ClientMsg::StopServer).unwrap();
    handle.join().unwrap().unwrap();
}

#[test]
fn opens_a_pane_for_every_started_flow_and_marks_it_hosted() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path().to_path_buf());
    let cat = Agent {
        command: vec!["cat".into()],
        prompt: None,
        adapter: None,
        resume: Some(vec!["cat".into()]),
    };
    let broken = Agent {
        command: vec!["/nonexistent/flow-agent".into()],
        prompt: None,
        adapter: None,
        resume: None,
    };
    let csv = started_flow(&store, dir.path(), "csv", "claude");
    let odd = started_flow(&store, dir.path(), "odd", "codex");
    let (paths, handle) = start_with_agents(
        dir.path(),
        BTreeMap::from([("claude".to_string(), cat), ("broken".to_string(), broken)]),
    );
    let mut ui = connect(
        &paths.socket,
        Role::Attach {
            size: Size { cols: 30, rows: 4 },
        },
    );
    let later = started_flow(&store, dir.path(), "later", "broken");

    let mut flows = 0;
    let mut panes = Vec::new();
    recv_until(&mut ui, "not every flow got a pane", |message| {
        match message {
            ServerMsg::Board(board) => flows = board.count,
            ServerMsg::Panes(update) => panes = update,
            _ => {}
        }
        flows == 3 && status_of(&panes, &later).is_some()
    });

    assert_eq!(status_of(&panes, &csv), Some(PaneStatus::Running));
    assert_eq!(
        status_of(&panes, &odd),
        Some(PaneStatus::Failed {
            reason: "no agent named codex".into()
        })
    );
    assert!(matches!(
        status_of(&panes, &later),
        Some(PaneStatus::Failed { .. })
    ));
    let hosted = store.events(&csv).unwrap().into_iter().any(|event| {
        matches!(event.kind, EventKind::Workspace { window: Some(ref w), .. } if w == HOSTED)
    });
    assert!(hosted, "an opened worker is marked hosted");
    let blocked = store
        .events(&later)
        .unwrap()
        .into_iter()
        .any(|event| matches!(event.kind, EventKind::Blocked { .. }));
    assert!(blocked, "a worker that cannot start blocks its flow");

    proto::write_msg(&mut ui, &ClientMsg::Focus(PaneKey::Flow(csv))).unwrap();
    proto::write_msg(&mut ui, &ClientMsg::Input(b"csv here\r".to_vec())).unwrap();
    recv_until(&mut ui, "the worker never echoed", |message| {
        matches!(message, ServerMsg::Frame(frame)
            if (0..4).any(|row| text(&frame, row).contains("csv here")))
    });

    proto::write_msg(&mut ui, &ClientMsg::StopServer).unwrap();
    handle.join().unwrap().unwrap();
}

#[test]
fn a_worker_whose_start_cannot_be_recorded_never_starts() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path().to_path_buf());
    let ran = dir.path().join("ran");
    let marking = Agent {
        command: vec![
            "sh".into(),
            "-c".into(),
            format!("touch {}; exec cat", ran.display()),
        ],
        prompt: Some(String::new()),
        adapter: None,
        resume: Some(vec!["cat".into()]),
    };
    let cat = Agent {
        command: vec!["cat".into()],
        prompt: None,
        adapter: None,
        resume: Some(vec!["cat".into()]),
    };
    let id = started_flow(&store, dir.path(), "unrecorded", "marking");
    let log = dir.path().join(format!("flows/{id}.jsonl"));
    fs::set_permissions(&log, fs::Permissions::from_mode(0o444)).unwrap();
    let (paths, handle) = start_with_agents(
        dir.path(),
        BTreeMap::from([
            ("claude".to_string(), cat),
            ("marking".to_string(), marking),
        ]),
    );
    let mut ui = connect(
        &paths.socket,
        Role::Attach {
            size: Size { cols: 30, rows: 4 },
        },
    );

    let mut panes = Vec::new();
    recv_until(&mut ui, "the worker never got a pane", |message| {
        if let ServerMsg::Panes(update) = message {
            panes = update;
        }
        status_of(&panes, &id).is_some()
    });

    let status = status_of(&panes, &id);
    assert!(
        matches!(&status, Some(PaneStatus::Failed { reason }) if reason.starts_with("could not record")),
        "{status:?}"
    );
    assert!(
        !ran.exists(),
        "an agent the next server could not resume ran"
    );
    proto::write_msg(&mut ui, &ClientMsg::StopServer).unwrap();
    handle.join().unwrap().unwrap();
}

#[test]
fn a_worker_whose_agent_could_not_start_starts_fresh_when_reopened() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path().to_path_buf());
    let late = dir.path().join("late-agent");
    let unresumable = Agent {
        command: vec![late.display().to_string()],
        prompt: None,
        adapter: None,
        resume: None,
    };
    let cat = Agent {
        command: vec!["cat".into()],
        prompt: None,
        adapter: None,
        resume: Some(vec!["cat".into()]),
    };
    let id = started_flow(&store, dir.path(), "late", "late");
    let (paths, handle) = start_with_agents(
        dir.path(),
        BTreeMap::from([
            ("claude".to_string(), cat),
            ("late".to_string(), unresumable),
        ]),
    );
    let mut ui = connect(
        &paths.socket,
        Role::Attach {
            size: Size { cols: 30, rows: 4 },
        },
    );
    let mut panes = Vec::new();
    recv_until(&mut ui, "the worker never failed to start", |message| {
        if let ServerMsg::Panes(update) = message {
            panes = update;
        }
        matches!(status_of(&panes, &id), Some(PaneStatus::Failed { .. }))
    });

    executable(&late, "#!/bin/sh\nexec cat\n");
    proto::write_msg(&mut ui, &ClientMsg::Reopen(PaneKey::Flow(id.clone()))).unwrap();
    recv_until(
        &mut ui,
        "the worker never left its failed start",
        |message| {
            if let ServerMsg::Panes(update) = message {
                panes = update;
            }
            !matches!(status_of(&panes, &id), Some(PaneStatus::Failed { reason }) if reason.starts_with("could not start"))
        },
    );
    assert_eq!(status_of(&panes, &id), Some(PaneStatus::Running));

    proto::write_msg(&mut ui, &ClientMsg::StopServer).unwrap();
    handle.join().unwrap().unwrap();
}

#[test]
fn closes_a_flow_pane_on_request_and_says_when_there_is_none() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path().to_path_buf());
    let csv = started_flow(&store, dir.path(), "csv", "claude");
    let (paths, handle) = start(dir.path());
    let mut ui = connect(
        &paths.socket,
        Role::Attach {
            size: Size { cols: 30, rows: 4 },
        },
    );
    let mut panes = Vec::new();
    wait_for(&mut ui, &mut panes, &csv, Some(&PaneStatus::Running));

    let closed = crate::client::close_flow(&paths.socket, &csv, PATIENCE);
    let none = crate::client::close_flow(&paths.socket, "nope", PATIENCE);

    assert_eq!(closed.unwrap(), crate::client::Closing::Closed);
    assert_eq!(none.unwrap(), crate::client::Closing::NoPane);
    wait_for(&mut ui, &mut panes, &csv, None);
    assert!(
        store
            .events(&csv)
            .unwrap()
            .iter()
            .all(|event| !matches!(event.kind, EventKind::Blocked { .. })),
        "deliberately closing a working pane must not block its flow"
    );
    proto::write_msg(&mut ui, &ClientMsg::StopServer).unwrap();
    handle.join().unwrap().unwrap();
    assert_eq!(
        crate::client::close_flow(&paths.socket, &csv, Duration::from_secs(1)).unwrap(),
        crate::client::Closing::NoServer
    );
}

/// Reads messages until the UI's pane list shows `want` for the flow.
fn wait_for(
    ui: &mut UnixStream,
    panes: &mut Vec<crate::proto::PaneInfo>,
    id: &str,
    want: Option<&PaneStatus>,
) {
    if status_of(panes, id).as_ref() == want {
        return;
    }
    recv_until(ui, &format!("{id} never became {want:?}"), |message| {
        if let ServerMsg::Panes(update) = message {
            *panes = update;
        }
        status_of(panes, id).as_ref() == want
    });
}

#[test]
fn reopens_an_exited_worker_but_refuses_finished_flows() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path().to_path_buf());
    let csv = started_flow(&store, dir.path(), "csv", "claude");
    let old = started_flow(&store, dir.path(), "old", "claude");
    store
        .append(&old, &LogEvent::new(2, EventKind::Ended))
        .unwrap();
    let done = started_flow(&store, dir.path(), "done", "claude");
    store
        .append(
            &done,
            &LogEvent::new(
                2,
                EventKind::MergeRequest {
                    url: "https://github.com/a/b/pull/45".into(),
                },
            ),
        )
        .unwrap();
    store
        .append(&done, &LogEvent::new(3, EventKind::Ended))
        .unwrap();
    let (paths, handle) = start(dir.path());
    let mut ui = connect(
        &paths.socket,
        Role::Attach {
            size: Size { cols: 30, rows: 4 },
        },
    );
    let mut panes = Vec::new();
    wait_for(&mut ui, &mut panes, &csv, Some(&PaneStatus::Running));
    assert_eq!(
        status_of(&panes, &old),
        None,
        "an ended flow does not open by itself"
    );

    proto::write_msg(&mut ui, &ClientMsg::Focus(PaneKey::Flow(csv.clone()))).unwrap();
    proto::write_msg(&mut ui, &ClientMsg::Input(b"\x04".to_vec())).unwrap();
    wait_for(
        &mut ui,
        &mut panes,
        &csv,
        Some(&PaneStatus::Exited { code: Some(0) }),
    );
    proto::write_msg(&mut ui, &ClientMsg::Reopen(PaneKey::Flow(csv.clone()))).unwrap();
    wait_for(&mut ui, &mut panes, &csv, Some(&PaneStatus::Running));
    proto::write_msg(&mut ui, &ClientMsg::Reopen(PaneKey::Flow(old.clone()))).unwrap();
    let refused = PaneStatus::Failed {
        reason: "finished flow cannot reopen; start a new flow".into(),
    };
    wait_for(&mut ui, &mut panes, &old, Some(&refused));
    proto::write_msg(&mut ui, &ClientMsg::Reopen(PaneKey::Flow(done.clone()))).unwrap();
    wait_for(&mut ui, &mut panes, &done, Some(&refused));

    proto::write_msg(&mut ui, &ClientMsg::StopServer).unwrap();
    handle.join().unwrap().unwrap();
}

#[test]
fn reopen_reads_an_end_event_before_the_board_refreshes() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path().to_path_buf());
    let id = started_flow(&store, dir.path(), "old", "claude");
    let (paths, handle) = start(dir.path());
    let mut ui = connect(
        &paths.socket,
        Role::Attach {
            size: Size { cols: 30, rows: 4 },
        },
    );
    let mut panes = Vec::new();
    wait_for(&mut ui, &mut panes, &id, Some(&PaneStatus::Running));
    proto::write_msg(&mut ui, &ClientMsg::Focus(PaneKey::Flow(id.clone()))).unwrap();
    proto::write_msg(&mut ui, &ClientMsg::Input(b"\x04".to_vec())).unwrap();
    wait_for(
        &mut ui,
        &mut panes,
        &id,
        Some(&PaneStatus::Exited { code: Some(0) }),
    );

    store
        .append(&id, &LogEvent::new(2, EventKind::Ended))
        .unwrap();
    proto::write_msg(&mut ui, &ClientMsg::Reopen(PaneKey::Flow(id.clone()))).unwrap();
    let refused = PaneStatus::Failed {
        reason: "finished flow cannot reopen; start a new flow".into(),
    };
    wait_for(&mut ui, &mut panes, &id, Some(&refused));

    proto::write_msg(&mut ui, &ClientMsg::StopServer).unwrap();
    handle.join().unwrap().unwrap();
}

#[test]
fn reopen_refuses_to_launch_when_the_flow_log_is_unreadable() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path().to_path_buf());
    let id = started_flow(&store, dir.path(), "gone", "claude");
    let (paths, handle) = start(dir.path());
    let mut ui = connect(
        &paths.socket,
        Role::Attach {
            size: Size { cols: 30, rows: 4 },
        },
    );
    let mut panes = Vec::new();
    wait_for(&mut ui, &mut panes, &id, Some(&PaneStatus::Running));
    proto::write_msg(&mut ui, &ClientMsg::Focus(PaneKey::Flow(id.clone()))).unwrap();
    proto::write_msg(&mut ui, &ClientMsg::Input(b"\x04".to_vec())).unwrap();
    wait_for(
        &mut ui,
        &mut panes,
        &id,
        Some(&PaneStatus::Exited { code: Some(0) }),
    );

    fs::remove_file(dir.path().join("flows").join(format!("{id}.jsonl"))).unwrap();
    proto::write_msg(&mut ui, &ClientMsg::Reopen(PaneKey::Flow(id.clone()))).unwrap();
    let refused = PaneStatus::Failed {
        reason: format!("could not check flow: no flow {id}"),
    };
    wait_for(&mut ui, &mut panes, &id, Some(&refused));

    proto::write_msg(&mut ui, &ClientMsg::StopServer).unwrap();
    handle.join().unwrap().unwrap();
}

#[test]
fn ends_every_unfinished_flow_before_a_stop_when_asked() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path().to_path_buf());
    let csv = started_flow(&store, dir.path(), "csv", "claude");
    let done = started_flow(&store, dir.path(), "done", "claude");
    store
        .append(&done, &LogEvent::new(2, EventKind::Ended))
        .unwrap();
    let (paths, handle) = start(dir.path());
    let mut ui = connect(
        &paths.socket,
        Role::Attach {
            size: Size { cols: 30, rows: 4 },
        },
    );
    let mut panes = Vec::new();
    wait_for(&mut ui, &mut panes, &csv, Some(&PaneStatus::Running));

    proto::write_msg(&mut ui, &ClientMsg::EndFlows).unwrap();
    proto::write_msg(&mut ui, &ClientMsg::StopServer).unwrap();
    handle.join().unwrap().unwrap();

    let ends = |id: &str| {
        store
            .events(id)
            .unwrap()
            .iter()
            .filter(|event| event.kind == EventKind::Ended)
            .count()
    };
    assert_eq!(ends(&csv), 1);
    assert_eq!(ends(&done), 1, "a finished flow is left alone");
}

#[test]
fn a_ui_that_attaches_again_starts_on_the_master() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path().to_path_buf());
    let csv = started_flow(&store, dir.path(), "csv", "claude");
    let (paths, handle) = start(dir.path());
    let size = Size { cols: 30, rows: 4 };
    let mut first = connect(&paths.socket, Role::Attach { size });
    let mut panes = Vec::new();
    wait_for(&mut first, &mut panes, &csv, Some(&PaneStatus::Running));
    proto::write_msg(&mut first, &ClientMsg::Focus(PaneKey::Flow(csv.clone()))).unwrap();
    proto::write_msg(&mut first, &ClientMsg::History(Some(3))).unwrap();
    drop(first);

    let mut again = connect(&paths.socket, Role::Attach { size });
    let frame = loop {
        if let ServerMsg::Frame(frame) = recv(&mut again) {
            break frame;
        }
    };

    assert_eq!(frame.key, PaneKey::Master);
    assert_eq!(frame.history, None);
    proto::write_msg(&mut again, &ClientMsg::StopServer).unwrap();
    handle.join().unwrap().unwrap();
}

#[test]
fn a_second_ui_takes_over_and_an_old_protocol_is_refused() {
    let dir = TempDir::new().unwrap();
    let (paths, handle) = start(dir.path());
    let size = Size { cols: 20, rows: 4 };

    let mut first = connect_as(&paths.socket, Role::Attach { size }, peer(1));
    assert!(matches!(recv(&mut first), ServerMsg::Welcome { .. }));
    let mut second = connect_as(&paths.socket, Role::Attach { size }, peer(2));
    assert!(matches!(recv(&mut second), ServerMsg::Welcome { .. }));

    let mut told = false;
    while let Ok(Some(message)) = proto::read_msg::<_, ServerMsg>(&mut first) {
        told |= message == ServerMsg::Detached { by: peer(2) };
    }
    assert!(told);

    let mut old = UnixStream::connect(&paths.socket).unwrap();
    old.set_read_timeout(Some(PATIENCE)).unwrap();
    proto::write_msg(
        &mut old,
        &ClientMsg::Hello {
            version: PROTOCOL_VERSION + 1,
            role: Role::Control,
            peer: peer(3),
        },
    )
    .unwrap();
    assert!(matches!(recv(&mut old), ServerMsg::Refused { .. }));

    let mut unreadable = UnixStream::connect(&paths.socket).unwrap();
    unreadable.set_read_timeout(Some(PATIENCE)).unwrap();
    let mut garbage = 3u32.to_le_bytes().to_vec();
    garbage.extend([0xff, 0xff, 0xff]);
    std::io::Write::write_all(&mut unreadable, &garbage).unwrap();
    let ServerMsg::Refused { reason } = recv(&mut unreadable) else {
        panic!("expected a refusal");
    };
    assert!(reason.contains("could not read"), "{reason}");

    proto::write_msg(&mut second, &ClientMsg::StopServer).unwrap();
    handle.join().unwrap().unwrap();
}

#[test]
fn starts_the_master_fresh_when_its_session_cannot_be_resumed() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("master.json"),
        r#"{"session": "5e55", "cwd": "/tmp"}"#,
    )
    .unwrap();
    let (paths, handle) = start_with(
        dir.path(),
        Agent {
            command: vec!["cat".into()],
            prompt: None,
            adapter: None,
            resume: Some(vec![
                "sh".into(),
                "-c".into(),
                "echo No conversation found; exit 1".into(),
            ]),
        },
    );
    let mut ui = connect(
        &paths.socket,
        Role::Attach {
            size: Size { cols: 30, rows: 4 },
        },
    );
    thread::sleep(Duration::from_millis(500));

    proto::write_msg(&mut ui, &ClientMsg::Input(b"again\r".to_vec())).unwrap();

    let deadline = Instant::now() + PATIENCE;
    let mut running = false;
    let mut echoed = false;
    while !(running && echoed) {
        assert!(
            Instant::now() < deadline,
            "running {running}, echoed {echoed}"
        );
        match recv(&mut ui) {
            ServerMsg::Panes(panes) => running = panes[0].status == PaneStatus::Running,
            ServerMsg::Frame(frame) => {
                echoed |= (0..4).any(|row| text(&frame, row).contains("again"));
                if !echoed {
                    proto::write_msg(&mut ui, &ClientMsg::Input(b"again\r".to_vec())).unwrap();
                }
            }
            _ => {}
        }
    }
    proto::write_msg(&mut ui, &ClientMsg::StopServer).unwrap();
    handle.join().unwrap().unwrap();
}

#[test]
fn replaces_a_stale_socket_but_not_a_live_one() {
    let dir = TempDir::new().unwrap();
    let socket = dir.path().join("server.sock");
    drop(UnixListener::bind(&socket).unwrap());
    // A process another test forks right now holds the listener until it execs.
    wait_until("a forked process kept the stale socket", || {
        UnixStream::connect(&socket).is_err()
    });

    let listener = bind(&socket).unwrap();

    let mode = fs::metadata(&socket).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o600);
    let error = bind(&socket).unwrap_err();
    assert!(error.to_string().contains("already runs"), "{error}");
    drop(listener);
}

fn stop(paths: &ServerPaths, handle: thread::JoinHandle<Result<()>>) {
    let mut control = connect(&paths.socket, Role::Control);
    assert!(matches!(recv(&mut control), ServerMsg::Welcome { .. }));
    proto::write_msg(&mut control, &ClientMsg::StopServer).unwrap();
    handle.join().unwrap().unwrap();
}

#[test]
fn writes_the_default_files_on_its_first_start_and_keeps_a_config_of_your_own() {
    let fresh = TempDir::new().unwrap();
    let (paths, handle) = start(fresh.path());
    stop(&paths, handle);

    let read = |path: &Option<PathBuf>| fs::read_to_string(path.as_ref().unwrap()).unwrap();
    assert_eq!(
        fs::read_to_string(&paths.instructions).unwrap(),
        master::INSTRUCTIONS.text
    );
    assert_eq!(read(&paths.config), crate::config::EXAMPLE.text);
    assert_eq!(read(&paths.theme), crate::ui::theme_file::EXAMPLE.text);

    let edited = TempDir::new().unwrap();
    let mine = edited.path().join("config/config.toml");
    fs::create_dir_all(mine.parent().unwrap()).unwrap();
    fs::write(&mine, "[worker]\nagent = \"codex\"\n").unwrap();
    let (paths, handle) = start(edited.path());
    stop(&paths, handle);

    assert_eq!(read(&paths.config), "[worker]\nagent = \"codex\"\n");
    assert_eq!(read(&paths.theme), crate::ui::theme_file::EXAMPLE.text);
}

#[test]
fn says_why_it_stops() {
    let signal = |from| Stop::Signal {
        name: "SIGTERM",
        from,
    };
    assert_eq!(signal(Some(2810)).to_string(), "SIGTERM from pid 2810");
    assert_eq!(signal(None).to_string(), "SIGTERM");
    assert_eq!(Stop::Asked(None).to_string(), "StopServer");
    assert_eq!(
        Stop::Asked(Some(peer(4321))).to_string(),
        "StopServer from pid 4321 in /work (saaio test)"
    );
}

/// A Codex stand-in: ignores flow's wiring and keeps what is typed into it in `typed`.
fn fake_codex(dir: &Path) -> Agent {
    let script = dir.join("fake-codex");
    executable(&script, "#!/bin/sh\nexec cat > typed\n");
    Agent {
        command: vec![script.display().to_string()],
        prompt: None,
        adapter: Some(crate::adapter::Adapter::Codex),
        resume: Some(vec![script.display().to_string()]),
    }
}

/// What a session that had its first turn reported, so saaio server may type into it.
fn had_a_turn(store: &Store, id: &str) {
    let session = EventKind::Session {
        id: "01a1".into(),
        transcript: None,
    };
    store.append(id, &LogEvent::new(2, session)).unwrap();
}

fn tell(store: &Store, id: &str, text: &str) {
    let message = EventKind::Message { text: text.into() };
    store.append(id, &LogEvent::new(3, message)).unwrap();
}

fn typed(dir: &Path, name: &str) -> String {
    fs::read_to_string(dir.join(name).join("typed")).unwrap_or_default()
}

#[test]
fn types_the_masters_message_into_an_idle_codex_worker_once() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path().to_path_buf());
    let csv = started_flow(&store, dir.path(), "csv", "codex");
    let busy = started_flow(&store, dir.path(), "busy", "codex");
    had_a_turn(&store, &csv);
    had_a_turn(&store, &busy);
    store
        .set_busy(&Waiter::Flow(csv.clone()), Busy::Turn)
        .unwrap();
    let (paths, handle) = start_with_agents(
        dir.path(),
        BTreeMap::from([("codex".to_string(), fake_codex(dir.path()))]),
    );
    wait_until("the busy worker never started", || {
        dir.path().join("busy/typed").exists()
    });
    store
        .set_busy(&Waiter::Flow(busy.clone()), Busy::Turn)
        .unwrap();

    tell(&store, &csv, "Tabs.");
    tell(&store, &busy, "Spaces.");
    wait_until_typed(dir.path(), "csv", "Tabs.");
    thread::sleep(WATCH_EVERY * 3);

    let text = typed(dir.path(), "csv");
    assert!(text.contains("message from the saaio master:"), "{text:?}");
    assert_eq!(text.matches("Tabs.").count(), 1, "once: {text:?}");
    assert_eq!(
        store.delivered(&csv),
        1,
        "a busy mark from before its pane opened is stale"
    );
    assert_eq!(
        typed(dir.path(), "busy"),
        "",
        "a busy worker gets it at its Stop"
    );
    assert_eq!(store.delivered(&busy), 0);

    let mut ui = connect(
        &paths.socket,
        Role::Attach {
            size: Size { cols: 30, rows: 4 },
        },
    );
    proto::write_msg(&mut ui, &ClientMsg::StopServer).unwrap();
    handle.join().unwrap().unwrap();
}

#[test]
fn never_types_into_a_session_that_had_no_turn_or_a_claude_worker() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path().to_path_buf());
    let fresh = started_flow(&store, dir.path(), "fresh", "codex");
    let claude = started_flow(&store, dir.path(), "claude", "claude-cat");
    had_a_turn(&store, &claude);
    let cat = Agent {
        adapter: Some(crate::adapter::Adapter::ClaudeCode),
        ..fake_codex(dir.path())
    };
    let (paths, handle) = start_with_agents(
        dir.path(),
        BTreeMap::from([
            ("codex".to_string(), fake_codex(dir.path())),
            ("claude-cat".to_string(), cat),
        ]),
    );

    tell(&store, &fresh, "Tabs.");
    tell(&store, &claude, "Tabs.");
    thread::sleep(WATCH_EVERY * 4);

    assert_eq!(
        typed(dir.path(), "fresh"),
        "",
        "still starting, maybe at its hooks review"
    );
    assert_eq!(typed(dir.path(), "claude"), "", "saaio wait wakes it");
    let mut ui = connect(
        &paths.socket,
        Role::Attach {
            size: Size { cols: 30, rows: 4 },
        },
    );
    proto::write_msg(&mut ui, &ClientMsg::StopServer).unwrap();
    handle.join().unwrap().unwrap();
}

/// A Claude Code stand-in that keeps each line typed into it in `typed` and, as its prompt hook
/// would, marks flow `id`'s turn as begun.
fn fake_claude(dir: &Path, id: &str) -> Agent {
    let script = dir.join("fake-claude");
    let busy = dir.join(format!("waits/{id}.busy"));
    let body = format!(
        "#!/bin/sh\nwhile IFS= read -r line; do printf '%s\\n' \"$line\" >> typed; printf turn > '{}'; done\n",
        busy.display()
    );
    executable(&script, &body);
    Agent {
        command: vec![script.display().to_string()],
        prompt: None,
        adapter: Some(crate::adapter::Adapter::ClaudeCode),
        resume: Some(vec![script.display().to_string()]),
    }
}

fn queue(store: &Store, id: &str, line: &str) {
    let typed = EventKind::Typed { text: line.into() };
    store.append(id, &LogEvent::new(3, typed)).unwrap();
}

fn wait_until_typed(dir: &Path, name: &str, want: &str) {
    wait_until(&format!("{want} never typed into {name}"), || {
        typed(dir, name).contains(want)
    });
}

#[test]
fn types_queued_lines_after_pr_once_the_turn_before_it_ended() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path().to_path_buf());
    let id = started_flow(&store, dir.path(), "csv", "claude-cat");
    let answer = EventKind::Answer {
        text: "Plan done.".into(),
    };
    store.append(&id, &LogEvent::new(2, answer)).unwrap();
    store
        .append(
            &id,
            &LogEvent::new(
                2,
                EventKind::MergeRequest {
                    url: "https://github.com/a/b/pull/7".into(),
                },
            ),
        )
        .unwrap();
    queue(&store, &id, "/compact");
    queue(&store, &id, "continue shipping");
    let (paths, handle) = start_with_agents(
        dir.path(),
        BTreeMap::from([("claude-cat".to_string(), fake_claude(dir.path(), &id))]),
    );
    let waiter = Waiter::Flow(id.clone());

    wait_until_typed(dir.path(), "csv", "/compact");
    wait_until("its turn never began", || store.busy(&waiter) == Busy::Turn);
    thread::sleep(WATCH_EVERY * 3);
    assert_eq!(
        typed(dir.path(), "csv"),
        "\x1b[200~/compact\x1b[201~\n",
        "never mid-turn"
    );
    assert_eq!(store.typed(&id), 1);

    store.set_busy(&waiter, Busy::Idle).unwrap();
    wait_until_typed(dir.path(), "csv", "continue shipping");
    assert_eq!(
        typed(dir.path(), "csv"),
        "\x1b[200~/compact\x1b[201~\n\x1b[200~continue shipping\x1b[201~\n"
    );
    assert_eq!(store.typed(&id), 2);
    stop(&paths, handle);
}

#[test]
fn a_typed_line_that_begins_no_turn_lets_the_next_one_follow() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path().to_path_buf());
    let id = started_flow(&store, dir.path(), "csv", "codex");
    let fresh = started_flow(&store, dir.path(), "fresh", "codex");
    had_a_turn(&store, &id);
    let answer = EventKind::Answer {
        text: "Plan done.".into(),
    };
    store.append(&id, &LogEvent::new(2, answer)).unwrap();
    queue(&store, &id, "/cost");
    queue(&store, &id, "go on");
    queue(&store, &fresh, "/compact");
    let (paths, handle) = start_timed(
        dir.path(),
        BTreeMap::from([("codex".to_string(), fake_codex(dir.path()))]),
        Timing {
            unstarted: Duration::from_millis(300),
            ..quick()
        },
        || 0,
    );

    wait_until_typed(dir.path(), "csv", "go on");
    assert_eq!(store.busy(&Waiter::Flow(id.clone())), Busy::Typed);
    assert_eq!(
        typed(dir.path(), "fresh"),
        "",
        "a session that never ended a turn may still be starting up"
    );
    stop(&paths, handle);
}

/// A Claude Code stand-in that keeps every byte sent to it in `typed`, a key such as Esc included.
fn fake_claude_keys(dir: &Path) -> Agent {
    let script = dir.join("fake-claude-keys");
    executable(&script, "#!/bin/sh\nstty raw -echo\nexec cat > typed\n");
    Agent {
        command: vec![script.display().to_string()],
        prompt: None,
        adapter: Some(crate::adapter::Adapter::ClaudeCode),
        resume: Some(vec![script.display().to_string()]),
    }
}

fn ask(store: &Store, id: &str, questions: Vec<crate::event::Question>) {
    let waiting = EventKind::Waiting {
        question: "Which color?".into(),
        questions,
    };
    store.append(id, &LogEvent::new(2, waiting)).unwrap();
}

#[test]
fn presses_esc_once_into_each_dialog_the_master_answered() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path().to_path_buf());
    let color = || {
        vec![crate::event::Question::of(
            "Which color?",
            &["Red", "Blue"],
            false,
        )]
    };
    let asks = started_flow(&store, dir.path(), "asks", "claude-keys");
    let unanswered = started_flow(&store, dir.path(), "unanswered", "claude-keys");
    let worded = started_flow(&store, dir.path(), "worded", "claude-keys");
    let pr = "https://github.com/a/b/pull/42";
    let opened = EventKind::MergeRequest { url: pr.into() };
    store.append(&asks, &LogEvent::new(1, opened)).unwrap();
    ask(&store, &asks, color());
    ask(&store, &unanswered, color());
    ask(&store, &worded, Vec::new());
    tell(&store, &asks, "The user says: Red");
    tell(&store, &worded, "The user says: Red");
    let (paths, handle) = start_with_agents(
        dir.path(),
        BTreeMap::from([("claude-keys".to_string(), fake_claude_keys(dir.path()))]),
    );

    wait_until("no Esc into the answered dialog", || {
        typed(dir.path(), "asks").contains('\x1b')
    });
    let closed = EventKind::MergeRequestClosed { url: pr.into() };
    store.append(&asks, &LogEvent::new(4, closed)).unwrap();
    thread::sleep(WATCH_EVERY * 3);
    assert_eq!(
        typed(dir.path(), "asks"),
        "\x1b",
        "once, though the dialog still shows open and the flow changed"
    );
    let log = dir.path().join(format!("flows/{asks}.jsonl"));
    fs::set_permissions(&log, fs::Permissions::from_mode(0o000)).unwrap();
    thread::sleep(WATCH_EVERY * 3);
    fs::set_permissions(&log, fs::Permissions::from_mode(0o644)).unwrap();
    thread::sleep(WATCH_EVERY * 3);
    assert_eq!(
        typed(dir.path(), "asks"),
        "\x1b",
        "once, though its log could not be read for a while"
    );

    let idle = EventKind::Idle {
        note: "declined".into(),
    };
    store.append(&asks, &LogEvent::new(5, idle)).unwrap();
    thread::sleep(WATCH_EVERY * 3);
    let again = EventKind::Waiting {
        question: "Which color?".into(),
        questions: color(),
    };
    store.append(&asks, &LogEvent::new(6, again)).unwrap();
    tell(&store, &asks, "The user says: Blue");
    wait_until("no Esc into the next dialog", || {
        typed(dir.path(), "asks") == "\x1b\x1b"
    });
    assert_eq!(typed(dir.path(), "unanswered"), "", "no message yet");
    assert_eq!(typed(dir.path(), "worded"), "", "no dialog");
    stop(&paths, handle);
}

#[test]
fn opens_the_next_worker_with_the_config_saved_while_it_runs() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path().to_path_buf());
    let (paths, handle) = start(dir.path());
    let config = paths.config.clone().unwrap();
    let saves_its_prompt = "[agents.claude]\ncommand = [\"sh\", \"-c\", \"echo \\\"$0\\\" > typed; exec cat\", \"{prompt}\"]\nprompt = \"/mine {request}\"\n";
    fs::write(&config, saves_its_prompt).unwrap();
    started_flow(&store, dir.path(), "csv", "claude");
    wait_until("the worker never saved its prompt", || {
        !typed(dir.path(), "csv").is_empty()
    });
    assert_eq!(typed(dir.path(), "csv"), "/mine x\n");

    fs::write(&config, "[agents.claude]\ncommand = [\n").unwrap();
    started_flow(&store, dir.path(), "odd", "claude");
    wait_until("a broken file dropped the last config", || {
        !typed(dir.path(), "odd").is_empty()
    });
    assert_eq!(typed(dir.path(), "odd"), "/mine x\n");

    stop(&paths, handle);
}

#[test]
fn a_running_worker_keeps_the_adapter_it_started_with() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path().to_path_buf());
    let csv = started_flow(&store, dir.path(), "csv", "codex");
    had_a_turn(&store, &csv);
    let codex = fake_codex(dir.path());
    let (paths, handle) = start_with_agents(
        dir.path(),
        BTreeMap::from([("codex".to_string(), codex.clone())]),
    );
    wait_until("the worker never started", || {
        dir.path().join("csv/typed").exists()
    });
    let without_adapter = format!("[agents.codex]\ncommand = [\"{}\"]\n", codex.command[0]);
    fs::write(paths.config.as_ref().unwrap(), without_adapter).unwrap();
    thread::sleep(WATCH_EVERY * 2);

    tell(&store, &csv, "Tabs.");
    wait_until("an edited agent took the running worker's adapter", || {
        typed(dir.path(), "csv").contains("Tabs.")
    });

    stop(&paths, handle);
}

#[test]
fn the_start_line_names_the_build_and_the_system() {
    let system = System {
        os: Some("Darwin 25.6.0 arm64".to_string()),
        tools: vec![("glab", crate::system::Probe::Missing)],
    };
    assert_eq!(
        started(&system),
        format!(
            "started: saaio {BUILD}, protocol {PROTOCOL_VERSION}, Darwin 25.6.0 arm64, glab missing"
        )
    );
}

#[test]
fn passes_on_what_the_pane_in_view_copies_and_nothing_else() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path().to_path_buf());
    let script = |name: &str, body: &str| {
        let path = dir.path().join(name);
        executable(&path, &format!("#!/bin/sh\n{body}\n"));
        let command = vec![path.display().to_string()];
        Agent {
            command: command.clone(),
            prompt: None,
            adapter: None,
            resume: Some(command),
        }
    };
    let printed = dir.path().join("printed");
    // The worker copies again and again, so a copy wrongly passed on reaches the UI before the
    // master's, whenever the server reads it.
    let worker = script(
        "worker",
        &format!(
            "printf '\\033]52;c;Ynll\\007'; touch '{}'\nwhile :; do printf '\\033]52;c;Ynll\\007'; sleep 0.05; done",
            printed.display()
        ),
    );
    let master = script("master", "read line; printf '\\033]52;c;aGk=\\007'; cat");
    started_flow(&store, dir.path(), "csv", "worker");
    let (paths, handle) = start_with_agents(
        dir.path(),
        BTreeMap::from([
            ("claude".to_string(), master),
            ("worker".to_string(), worker),
        ]),
    );
    let mut ui = connect(
        &paths.socket,
        Role::Attach {
            size: Size { cols: 30, rows: 4 },
        },
    );
    let deadline = Instant::now() + PATIENCE;
    while !printed.exists() {
        assert!(Instant::now() < deadline, "the worker never copied");
        thread::sleep(Duration::from_millis(20));
    }

    proto::write_msg(&mut ui, &ClientMsg::Input(b"go\r".to_vec())).unwrap();

    let copied = loop {
        assert!(Instant::now() < deadline, "nothing was passed on");
        if let ServerMsg::Clipboard(text) = recv(&mut ui) {
            break text;
        }
    };
    assert_eq!(copied, "hi", "the worker is not in view");
    proto::write_msg(&mut ui, &ClientMsg::StopServer).unwrap();
    handle.join().unwrap().unwrap();
}

fn now() -> u64 {
    crate::event::unix_ms(std::time::SystemTime::now())
}

/// A flow idle for ten minutes in a worktree of `repo`.
fn idle_flow(store: &Store, repo: &Path, name: &str) -> String {
    let id = store
        .create(&LogEvent::new(
            now() - 20 * 60_000,
            EventKind::Started {
                name: name.into(),
                request: "x".into(),
                mode: crate::event::Mode::Auto,
                phases: None,
            },
        ))
        .unwrap();
    let path = repo.join(".saaio/worktrees").join(name);
    fs::create_dir_all(&path).unwrap();
    let workspace = EventKind::Workspace {
        path: path.display().to_string(),
        branch: Some(format!("flow/{name}")),
        slot: Some(4),
        window: None,
        agent: "claude".into(),
    };
    store
        .append(&id, &LogEvent::new(now() - 20 * 60_000, workspace))
        .unwrap();
    let idle = EventKind::Idle {
        note: "Plan ready.".into(),
    };
    store
        .append(&id, &LogEvent::new(now() - 10 * 60_000, idle))
        .unwrap();
    id
}

fn services(store: &Store, id: &str) -> Vec<crate::event::ServiceStatus> {
    store
        .events(id)
        .unwrap()
        .into_iter()
        .filter_map(|event| match event.kind {
            EventKind::Services { status, .. } => Some(status),
            _ => None,
        })
        .collect()
}

fn wait_for_services(store: &Store, id: &str, last: crate::event::ServiceStatus) {
    wait_until(&format!("never {last:?}"), || {
        services(store, id).last() == Some(&last)
    });
}

#[test]
fn pauses_an_idle_flows_services_and_resumes_them_when_it_works() {
    use crate::event::ServiceStatus::{Paused, Pausing, Resuming, Running};
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path().to_path_buf());
    let repo = dir.path().join("shop");
    let ran = dir.path().join("ran.txt");
    for name in ["pause", "resume"] {
        let script = repo.join(".saaio").join(name);
        fs::create_dir_all(script.parent().unwrap()).unwrap();
        let body = format!(
            "#!/bin/sh\necho \"{name} $SAAIO_SLOT $(basename \"$PWD\")\" >> '{}'\n",
            ran.display()
        );
        executable(&script, &body);
    }
    let id = idle_flow(&store, &repo, "csv");
    let bare = idle_flow(&store, &dir.path().join("plain-repo"), "pdf");
    let (paths, handle) = start_at(
        dir.path(),
        BTreeMap::from([(
            "claude".to_string(),
            Agent {
                command: vec!["cat".into()],
                prompt: None,
                adapter: None,
                resume: Some(vec!["cat".into()]),
            },
        )]),
        now,
    );

    wait_for_services(&store, &id, Paused);
    assert_eq!(fs::read_to_string(&ran).unwrap(), "pause 4 csv\n");
    store
        .append(&id, &LogEvent::new(now(), EventKind::Working))
        .unwrap();
    wait_for_services(&store, &id, Running);

    assert_eq!(services(&store, &id), [Pausing, Paused, Resuming, Running]);
    assert_eq!(
        fs::read_to_string(&ran).unwrap(),
        "pause 4 csv\nresume 4 csv\n"
    );
    assert!(services(&store, &bare).is_empty(), "a repo without scripts");
    stop(&paths, handle);
}
