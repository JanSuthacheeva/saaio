use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixListener;
use std::thread;

use tempfile::TempDir;

use super::*;
use crate::adapter::Adapter;
use crate::config::Agent;
use crate::event::{Event as LogEvent, EventKind};
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

fn start_with_agents(
    dir: &Path,
    agents: BTreeMap<String, Agent>,
) -> (ServerPaths, thread::JoinHandle<Result<()>>) {
    let paths = ServerPaths {
        socket: dir.join("server.sock"),
        master: dir.join("master.json"),
        instructions: dir.join("master.md"),
        config: Some(dir.join("config/config.toml")),
        theme: Some(dir.join("config/theme.toml")),
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
            &std::env::current_dir().unwrap(),
            &System::default(),
            || 0,
        )
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    while UnixStream::connect(&paths.socket).is_err() {
        assert!(Instant::now() < deadline, "the server did not start");
        thread::sleep(Duration::from_millis(20));
    }
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
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
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
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut seen = String::new();
    while !seen.contains("hello") {
        assert!(Instant::now() < deadline, "never saw the echo: {seen:?}");
        if let ServerMsg::Frame(frame) = recv(&mut ui) {
            seen = text(&frame, 0);
        }
    }

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
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        assert!(Instant::now() < deadline, "the turn never reached the ui");
        if let ServerMsg::Master { turn, .. } = recv(&mut ui) {
            assert_eq!(turn, Some(working));
            break;
        }
    }

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
    fs::write(&fake_codex, "#!/bin/sh\nsleep 30\n").unwrap();
    let mut permissions = fs::metadata(&fake_codex).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&fake_codex, permissions).unwrap();
    let codex = Agent {
        command: vec![fake_codex.display().to_string()],
        prompt: None,
        adapter: Some(Adapter::Codex),
        resume: None,
    };
    let (paths, handle) =
        start_with_agents(dir.path(), BTreeMap::from([("codex".to_string(), codex)]));

    thread::sleep(super::startup::FIRST_TURN_GRACE + WATCH_EVERY * 2);
    let events = store.events(&id).unwrap();
    let waiting: Vec<_> = events
        .iter()
        .filter(|event| {
            matches!(
                &event.kind,
                EventKind::Waiting { question, .. } if question == super::startup::STALLED_START
            )
        })
        .collect();
    assert_eq!(waiting.len(), 1, "{events:?}");

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

    let deadline = Instant::now() + Duration::from_secs(5);
    let mut flows = 0;
    let mut panes = Vec::new();
    while !(flows == 3 && status_of(&panes, &later).is_some()) {
        assert!(Instant::now() < deadline, "flows {flows}, panes {panes:?}");
        match recv(&mut ui) {
            ServerMsg::Board(board) => flows = board.count,
            ServerMsg::Panes(update) => panes = update,
            _ => {}
        }
    }

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
    let mut echoed = false;
    while !echoed {
        assert!(Instant::now() < deadline, "the worker never echoed");
        if let ServerMsg::Frame(frame) = recv(&mut ui) {
            echoed = (0..4).any(|row| text(&frame, row).contains("csv here"));
        }
    }

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
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut panes = Vec::new();
    while status_of(&panes, &csv) != Some(PaneStatus::Running) {
        assert!(Instant::now() < deadline, "the worker never opened");
        if let ServerMsg::Panes(update) = recv(&mut ui) {
            panes = update;
        }
    }

    let closed = crate::client::close_flow(&paths.socket, &csv, Duration::from_secs(5));
    let none = crate::client::close_flow(&paths.socket, "nope", Duration::from_secs(5));

    assert_eq!(closed.unwrap(), crate::client::Closing::Closed);
    assert_eq!(none.unwrap(), crate::client::Closing::NoPane);
    while status_of(&panes, &csv).is_some() {
        assert!(Instant::now() < deadline, "the ui still lists the pane");
        if let ServerMsg::Panes(update) = recv(&mut ui) {
            panes = update;
        }
    }
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
    let deadline = Instant::now() + Duration::from_secs(5);
    while status_of(panes, id).as_ref() != want {
        assert!(
            Instant::now() < deadline,
            "{id} never became {want:?}: {panes:?}"
        );
        if let ServerMsg::Panes(update) = recv(ui) {
            *panes = update;
        }
    }
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
    old.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
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
    unreadable
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
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

    let deadline = Instant::now() + Duration::from_secs(5);
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
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while UnixStream::connect(&socket).is_ok() && std::time::Instant::now() < deadline {
        thread::sleep(Duration::from_millis(5));
    }

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
    fs::write(&script, "#!/bin/sh\nexec cat > typed\n").unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
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
    let deadline = Instant::now() + Duration::from_secs(5);
    while !dir.path().join("busy/typed").exists() {
        assert!(Instant::now() < deadline, "the busy worker never started");
        thread::sleep(Duration::from_millis(20));
    }
    store
        .set_busy(&Waiter::Flow(busy.clone()), Busy::Turn)
        .unwrap();

    tell(&store, &csv, "Tabs.");
    tell(&store, &busy, "Spaces.");
    while !typed(dir.path(), "csv").contains("Tabs.") {
        assert!(
            Instant::now() < deadline,
            "never typed: {:?}",
            typed(dir.path(), "csv")
        );
        thread::sleep(Duration::from_millis(50));
    }
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
    fs::write(&script, body).unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
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
    let deadline = Instant::now() + typing::UNSTARTED + Duration::from_secs(5);
    while !typed(dir, name).contains(want) {
        assert!(
            Instant::now() < deadline,
            "{want} never typed: {:?}",
            typed(dir, name)
        );
        thread::sleep(Duration::from_millis(20));
    }
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
    let deadline = Instant::now() + Duration::from_secs(5);
    while store.busy(&waiter) != Busy::Turn {
        assert!(Instant::now() < deadline, "its turn never began");
        thread::sleep(Duration::from_millis(20));
    }
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
    let (paths, handle) = start_with_agents(
        dir.path(),
        BTreeMap::from([("codex".to_string(), fake_codex(dir.path()))]),
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

#[test]
fn opens_the_next_worker_with_the_config_saved_while_it_runs() {
    let dir = TempDir::new().unwrap();
    let store = Store::at(dir.path().to_path_buf());
    let (paths, handle) = start(dir.path());
    let config = paths.config.clone().unwrap();
    let saves_its_prompt = "[agents.claude]\ncommand = [\"sh\", \"-c\", \"echo \\\"$0\\\" > typed; exec cat\", \"{prompt}\"]\nprompt = \"/mine {request}\"\n";
    fs::write(&config, saves_its_prompt).unwrap();
    started_flow(&store, dir.path(), "csv", "claude");
    let deadline = Instant::now() + Duration::from_secs(5);
    while typed(dir.path(), "csv").is_empty() {
        assert!(
            Instant::now() < deadline,
            "the worker never saved its prompt"
        );
        thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(typed(dir.path(), "csv"), "/mine x\n");

    fs::write(&config, "[agents.claude]\ncommand = [\n").unwrap();
    started_flow(&store, dir.path(), "odd", "claude");
    while typed(dir.path(), "odd").is_empty() {
        assert!(
            Instant::now() < deadline,
            "a broken file dropped the last config"
        );
        thread::sleep(Duration::from_millis(20));
    }
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
    let deadline = Instant::now() + Duration::from_secs(5);
    while !dir.path().join("csv/typed").exists() {
        assert!(Instant::now() < deadline, "the worker never started");
        thread::sleep(Duration::from_millis(20));
    }
    let without_adapter = format!("[agents.codex]\ncommand = [\"{}\"]\n", codex.command[0]);
    fs::write(paths.config.as_ref().unwrap(), without_adapter).unwrap();
    thread::sleep(WATCH_EVERY * 2);

    tell(&store, &csv, "Tabs.");
    while !typed(dir.path(), "csv").contains("Tabs.") {
        assert!(
            Instant::now() < deadline,
            "an edited agent took the running worker's adapter"
        );
        thread::sleep(Duration::from_millis(50));
    }

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
