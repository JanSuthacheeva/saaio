pub mod host;
pub mod master;
pub mod reconcile;
pub mod terminal;
pub mod watch;

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::ErrorKind;
use std::ops::ControlFlow;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};

use crate::config::Config;
use crate::event::{Event as LogEvent, EventKind};
use crate::fold;
use crate::launch::{self, HOSTED, LaunchEnv, PaneSpec};
use crate::proto::{self, ClientMsg, PROTOCOL_VERSION, PaneKey, PaneStatus, Role, ServerMsg, Size};
use crate::store::Store;
use crate::view::{Board, FlowView, Workspace};
use host::{Host, HostEvent};
use master::{MasterRecord, Start};
use reconcile::{Wanted, reconcile};
use watch::StoreWatch;

/// The shortest time between two frames of the focused pane (60 a second).
const FRAME_EVERY: Duration = Duration::from_millis(16);
/// How long a stop waits for the panes to exit.
const STOP_WAIT: Duration = Duration::from_millis(3500);
/// A UI that does not take a frame within this time is dropped, so it cannot stall the server.
const WRITE_TIMEOUT: Duration = Duration::from_secs(2);
/// A resumed pane that fails within this time is started fresh instead: its agent never saved
/// the session (Claude Code saves one only after the first message).
const RESUME_GRACE: Duration = Duration::from_secs(5);
/// How often the server looks at the store for new, changed and removed flows.
const WATCH_EVERY: Duration = Duration::from_millis(500);
/// The size panes start with before a UI says how big it is.
const START_SIZE: Size = Size {
    cols: 120,
    rows: 40,
};

#[derive(Debug, Clone)]
pub struct ServerPaths {
    pub socket: PathBuf,
    pub master: PathBuf,
    /// The master's orchestrator instructions, master.md beside config.toml.
    pub instructions: PathBuf,
}

type ConnId = u64;

#[derive(Debug)]
enum Event {
    Accepted(UnixStream),
    Message(ConnId, ClientMsg),
    Gone(ConnId),
    Host(HostEvent),
}

#[derive(Debug)]
struct Server {
    store: Store,
    clock: fn() -> u64,
    watch: StoreWatch,
    board: Board,
    sent_board: Option<Board>,
    sent_master: Option<ServerMsg>,
    known: HashSet<String>,
    refreshed_at: Instant,
    config: Config,
    env: LaunchEnv,
    record: MasterRecord,
    master_file: PathBuf,
    instructions: PathBuf,
    host: Host,
    conns: HashMap<ConnId, UnixStream>,
    next_conn: ConnId,
    attached: Option<ConnId>,
    focus: PaneKey,
    history: Option<u32>,
    resumed: HashMap<PaneKey, Instant>,
    /// The connections waiting for a pane to close, by pane.
    closing: HashMap<PaneKey, Vec<ConnId>>,
    dirty: bool,
    flushed_at: Instant,
    events: Sender<Event>,
}

/// Binds the socket (removing a stale one), opens the master, then loops until StopServer.
pub fn run(
    paths: &ServerPaths,
    store: Store,
    config: Config,
    env: LaunchEnv,
    cwd: &Path,
    clock: fn() -> u64,
) -> Result<()> {
    let listener = bind(&paths.socket)?;
    if let Err(error) = master::ensure_instructions(&paths.instructions) {
        eprintln!("{error:#}");
    }
    let (record, start) = MasterRecord::load_or_create(&paths.master, cwd)?;
    let (events, inbox) = mpsc::channel();
    let host = Host::new(START_SIZE, forward_host_events(events.clone()));
    let mut server = Server {
        store,
        clock,
        watch: StoreWatch::default(),
        board: fold::board(Vec::new(), clock()),
        sent_board: None,
        sent_master: None,
        known: HashSet::new(),
        refreshed_at: Instant::now(),
        config,
        env,
        record,
        master_file: paths.master.clone(),
        instructions: paths.instructions.clone(),
        host,
        conns: HashMap::new(),
        next_conn: 0,
        attached: None,
        focus: PaneKey::Master,
        history: None,
        resumed: HashMap::new(),
        closing: HashMap::new(),
        dirty: false,
        flushed_at: Instant::now(),
        events: events.clone(),
    };
    server.open_master(start);
    server.refresh();
    accept_in_background(listener, events);

    let result = server.serve(&inbox);
    let _ = fs::remove_file(&paths.socket);
    result
}

impl Server {
    fn serve(&mut self, inbox: &Receiver<Event>) -> Result<()> {
        loop {
            let watch = WATCH_EVERY.saturating_sub(self.refreshed_at.elapsed());
            let wait = if self.dirty {
                watch.min(FRAME_EVERY.saturating_sub(self.flushed_at.elapsed()))
            } else {
                watch
            };
            match inbox.recv_timeout(wait) {
                Ok(event) => {
                    if self.handle(event).is_break() {
                        self.stop(inbox);
                        return Ok(());
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => bail!("the server lost its event channel"),
            }
            if self.refreshed_at.elapsed() >= WATCH_EVERY {
                self.refresh();
            }
            self.flush();
        }
    }

    /// One event; Break once the server should stop.
    fn handle(&mut self, event: Event) -> ControlFlow<()> {
        match event {
            Event::Accepted(stream) => self.accept(stream),
            Event::Message(conn, message) => return self.message(conn, message),
            Event::Gone(conn) => self.drop_conn(conn),
            Event::Host(HostEvent::Output(key, bytes)) => {
                self.host.feed(&key, &bytes);
                if key == self.focus {
                    self.dirty = true;
                }
            }
            Event::Host(HostEvent::Exited(key, code)) => {
                self.host.exited(&key, code);
                let resumed_at = self.resumed.remove(&key);
                let resume_failed =
                    code != Some(0) && resumed_at.is_some_and(|at| at.elapsed() < RESUME_GRACE);
                if resume_failed {
                    self.open_fresh(&key);
                }
                self.tell_closed(&key);
                self.send_panes();
                if key == self.focus {
                    self.dirty = true;
                }
            }
        }
        ControlFlow::Continue(())
    }

    fn message(&mut self, conn: ConnId, message: ClientMsg) -> ControlFlow<()> {
        match message {
            ClientMsg::Hello { version, role } => self.hello(conn, version, role),
            ClientMsg::Focus(key) => {
                self.focus = key;
                self.history = None;
                self.send_full_frame();
            }
            ClientMsg::Input(bytes) => {
                if let Err(error) = self.host.write(&self.focus, &bytes) {
                    eprintln!("{error:#}");
                }
            }
            ClientMsg::Resize(size) => {
                self.host.resize(size);
                self.send_full_frame();
            }
            ClientMsg::History(history) => {
                self.history = history;
                self.send_full_frame();
            }
            ClientMsg::Reopen(PaneKey::Master) => {
                self.open_master(Start::Resume);
                self.send_panes();
                self.send_full_frame();
            }
            ClientMsg::CloseFlow(id) => self.close_flow(conn, id),
            ClientMsg::Reopen(PaneKey::Flow(id)) => {
                self.reopen_worker(&id);
                self.send_panes();
                self.send_full_frame();
            }
            ClientMsg::StopServer => return ControlFlow::Break(()),
            ClientMsg::EndFlows => self.end_flows(),
        }
        ControlFlow::Continue(())
    }

    fn hello(&mut self, conn: ConnId, version: u32, role: Role) {
        if version != PROTOCOL_VERSION {
            self.send_to(
                conn,
                &ServerMsg::Refused {
                    reason: format!(
                        "flow server runs protocol {PROTOCOL_VERSION}, this flow speaks {version}"
                    ),
                },
            );
            self.drop_conn(conn);
            return;
        }
        if let Role::Attach { size } = role {
            if let Some(old) = self.attached.replace(conn) {
                self.send_to(old, &ServerMsg::Detached);
                self.drop_conn(old);
            }
            // A UI starts on the master, live; the server follows, whatever the last UI looked at.
            self.focus = PaneKey::Master;
            self.history = None;
            self.host.resize(size);
        }
        self.send_to(
            conn,
            &ServerMsg::Welcome {
                version: PROTOCOL_VERSION,
            },
        );
        if self.attached == Some(conn) {
            self.sent_board = None;
            self.sent_master = None;
            self.send_master();
            self.send_board();
            self.send_panes();
            self.send_full_frame();
        }
    }

    /// Re-folds when the store changed, reconciles, sends Board and Panes to the attached UI.
    fn refresh(&mut self) {
        self.refreshed_at = Instant::now();
        self.send_master();
        let now = (self.clock)();
        match self.watch.changed(&self.store, now) {
            Ok(true) => {}
            Ok(false) => return,
            Err(error) => {
                eprintln!("could not look at the flows: {error:#}");
                return;
            }
        }
        let (logs, warnings) = self.store.logs();
        for warning in warnings {
            eprintln!("{warning}");
        }
        self.board = fold::board(fold::views(&logs, now), now);
        let open: Vec<PaneKey> = self.host.keys().cloned().collect();
        let wanted = reconcile(
            &self.board.flows,
            &open,
            &self.known,
            &self.config,
            &self.env,
        );
        let panes_change = wanted != Wanted::default();
        self.apply(wanted);
        self.send_board();
        if panes_change {
            self.send_panes();
        }
    }

    /// Opens and closes what reconcile asked for; marks new workers HOSTED in their log; Host::fail for failed.
    fn apply(&mut self, wanted: Wanted) {
        for key in &wanted.close {
            self.host.close(key);
        }
        for (key, reason) in wanted.failed {
            if let PaneKey::Flow(id) = &key {
                self.known.insert(id.clone());
            }
            self.host.fail(key, reason);
        }
        for spec in wanted.open {
            self.open_worker(spec);
        }
    }

    fn open_worker(&mut self, spec: PaneSpec) {
        let key = spec.key.clone();
        let PaneKey::Flow(id) = &key else {
            return;
        };
        self.known.insert(id.clone());
        let Some(space) = self.flow(id).and_then(|flow| flow.workspace.clone()) else {
            return;
        };
        let now = (self.clock)();
        let event = match self.host.open(spec) {
            Ok(()) => {
                if space.window.is_some() {
                    self.resumed.insert(key.clone(), Instant::now());
                }
                if space.window.as_deref() == Some(HOSTED) {
                    return;
                }
                EventKind::from(Workspace {
                    window: Some(HOSTED.to_string()),
                    ..space
                })
            }
            Err(error) => EventKind::Blocked {
                reason: format!("could not start the agent: {error:#}"),
            },
        };
        if let Err(error) = self.store.append(id, &LogEvent::new(now, event)) {
            eprintln!("{error:#}");
        }
    }

    /// Resumes a worker whose pane exited or never opened, also one whose flow ended.
    fn reopen_worker(&mut self, id: &str) {
        let key = PaneKey::Flow(id.to_string());
        if self.host.status(&key) == Some(&PaneStatus::Running) {
            return;
        }
        let Some(flow) = self.flow(id).cloned() else {
            return;
        };
        let Some(space) = &flow.workspace else {
            return;
        };
        let Some((name, agent)) = self.config.agent(Some(&space.agent)) else {
            self.host
                .fail(key, format!("no agent named {}", space.agent));
            return;
        };
        match launch::worker_spec(&flow, agent, &self.env) {
            Some(spec) => self.open_worker(spec),
            None => self.host.fail(key, reconcile::unopenable(name)),
        }
    }

    /// Opens a pane once more with the agent's first command: its resume failed before the agent
    /// ever saved the session.
    fn open_fresh(&mut self, key: &PaneKey) {
        eprintln!("resuming {key} failed, starting it fresh");
        let PaneKey::Flow(id) = key else {
            self.open_master(Start::Fresh);
            return;
        };
        let Some(mut flow) = self.flow(id).cloned() else {
            return;
        };
        let Some(space) = flow.workspace.as_mut() else {
            return;
        };
        space.window = None;
        let spec = self
            .config
            .agent(Some(&space.agent))
            .and_then(|(_, agent)| launch::worker_spec(&flow, agent, &self.env));
        if let Some(spec) = spec
            && let Err(error) = self.host.open(spec)
        {
            eprintln!("{error:#}");
        }
    }

    /// Hangs up a flow's pane for `flow remove` or `flow clear`; the connection hears Closed once
    /// it exited, NotOpen when there is no pane.
    fn close_flow(&mut self, conn: ConnId, id: String) {
        let key = PaneKey::Flow(id.clone());
        self.known.insert(id.clone());
        let Some(running) = self
            .host
            .status(&key)
            .map(|status| *status == PaneStatus::Running)
        else {
            self.send_to(conn, &ServerMsg::NotOpen(id));
            return;
        };
        self.closing.entry(key.clone()).or_default().push(conn);
        self.host.close(&key);
        if !running {
            self.tell_closed(&key);
        }
    }

    /// Tells everyone waiting for this pane that it is gone, once it is.
    fn tell_closed(&mut self, key: &PaneKey) {
        if self.host.status(key).is_some() {
            return;
        }
        let Some(waiting) = self.closing.remove(key) else {
            return;
        };
        if let PaneKey::Flow(id) = key {
            for conn in waiting {
                self.send_to(conn, &ServerMsg::Closed(id.clone()));
            }
        }
        self.send_panes();
    }

    /// An Ended event for every unfinished flow with a worker, so the next start opens none of them.
    fn end_flows(&mut self) {
        let now = (self.clock)();
        let unfinished: Vec<String> = self
            .board
            .flows
            .iter()
            .filter(|flow| flow.workspace.is_some() && !flow.is_finished())
            .map(|flow| flow.id.clone())
            .collect();
        for id in unfinished {
            if let Err(error) = self
                .store
                .append(&id, &LogEvent::new(now, EventKind::Ended))
            {
                eprintln!("{error:#}");
            }
        }
    }

    fn flow(&self, id: &str) -> Option<&FlowView> {
        self.board.flows.iter().find(|flow| flow.id == id)
    }

    fn open_master(&mut self, start: Start) {
        let master = self
            .config
            .agent(Some(crate::config::MASTER_AGENT))
            .or_else(|| self.config.agent(None));
        let Some((name, agent)) = master else {
            let reason = format!("no agent named {}", self.config.agent);
            self.host.fail(PaneKey::Master, reason);
            return;
        };
        match self.record.spec(
            start,
            agent,
            &self.env,
            &self.instructions,
            self.config.master.wake,
        ) {
            Some(spec) => match self.host.open(spec) {
                Ok(()) if start == Start::Resume => {
                    self.resumed.insert(PaneKey::Master, Instant::now());
                }
                Ok(()) => {}
                Err(error) => eprintln!("could not start the master: {error:#}"),
            },
            None => self.host.fail(
                PaneKey::Master,
                format!("agent {name} cannot resume a session"),
            ),
        }
    }

    /// Sends the focused pane's changed rows, at most 60 times a second.
    fn flush(&mut self) {
        if !self.dirty || self.flushed_at.elapsed() < FRAME_EVERY {
            return;
        }
        self.dirty = false;
        self.flushed_at = Instant::now();
        self.host.expire_sync();
        let Some(conn) = self.attached else {
            return;
        };
        if let Some(frame) = self.host.frame(&self.focus, false, self.history)
            && (!frame.rows.is_empty() || frame.history.is_some())
        {
            self.send_to(conn, &ServerMsg::Frame(frame));
        }
    }

    fn send_full_frame(&mut self) {
        let Some(conn) = self.attached else {
            return;
        };
        if let Some(frame) = self.host.frame(&self.focus, true, self.history) {
            self.send_to(conn, &ServerMsg::Frame(frame));
        }
        self.dirty = false;
        self.flushed_at = Instant::now();
    }

    /// Where the master runs, when that changed since the attached UI last heard it.
    fn send_master(&mut self) {
        let Some(conn) = self.attached else {
            return;
        };
        let master = ServerMsg::Master {
            cwd: self.record.cwd.display().to_string(),
            branch: crate::workspace::current_branch(&self.record.cwd),
        };
        if self.sent_master.as_ref() == Some(&master) {
            return;
        }
        self.send_to(conn, &master);
        self.sent_master = Some(master);
    }

    /// The board, when its flows changed since the attached UI last got it.
    fn send_board(&mut self) {
        let Some(conn) = self.attached else {
            return;
        };
        if self
            .sent_board
            .as_ref()
            .is_some_and(|sent| sent.flows == self.board.flows)
        {
            return;
        }
        let board = self.board.clone();
        self.send_to(conn, &ServerMsg::Board(board.clone()));
        self.sent_board = Some(board);
    }

    fn send_panes(&mut self) {
        if let Some(conn) = self.attached {
            let panes = self.host.panes();
            self.send_to(conn, &ServerMsg::Panes(panes));
        }
    }

    /// Writes one message; a connection that cannot take it is dropped.
    fn send_to(&mut self, conn: ConnId, message: &ServerMsg) {
        let Some(stream) = self.conns.get_mut(&conn) else {
            return;
        };
        if proto::write_msg(stream, message).is_err() {
            self.drop_conn(conn);
        }
    }

    fn accept(&mut self, stream: UnixStream) {
        let conn = self.next_conn;
        self.next_conn += 1;
        let Ok(reader) = stream.try_clone() else {
            return;
        };
        let _ = stream.set_write_timeout(Some(WRITE_TIMEOUT));
        self.conns.insert(conn, stream);
        read_in_background(conn, reader, self.events.clone());
    }

    fn drop_conn(&mut self, conn: ConnId) {
        if let Some(stream) = self.conns.remove(&conn) {
            let _ = stream.shutdown(std::net::Shutdown::Both);
        }
        if self.attached == Some(conn) {
            self.attached = None;
        }
    }

    /// Closes every pane and waits for them to exit; forgets the master, so the next start is a new one.
    fn stop(&mut self, inbox: &Receiver<Event>) {
        if let Err(error) = fs::remove_file(&self.master_file)
            && error.kind() != ErrorKind::NotFound
        {
            eprintln!("could not forget the master: {error}");
        }
        let keys: Vec<PaneKey> = self.host.keys().cloned().collect();
        for key in &keys {
            self.host.close(key);
        }
        let deadline = Instant::now() + STOP_WAIT;
        while self.host.any_running() {
            let Some(left) = deadline.checked_duration_since(Instant::now()) else {
                break;
            };
            match inbox.recv_timeout(left) {
                Ok(Event::Host(HostEvent::Exited(key, code))) => self.host.exited(&key, code),
                Ok(_) => {}
                Err(_) => break,
            }
        }
        let conns: Vec<ConnId> = self.conns.keys().copied().collect();
        for conn in conns {
            self.drop_conn(conn);
        }
    }
}

/// Binds the socket readable only by this user; a socket nobody answers on is stale and replaced.
fn bind(socket: &Path) -> Result<UnixListener> {
    if let Some(dir) = socket.parent() {
        fs::create_dir_all(dir).with_context(|| format!("could not create {}", dir.display()))?;
    }
    let listener = match UnixListener::bind(socket) {
        Ok(listener) => listener,
        Err(error) if error.kind() == ErrorKind::AddrInUse => {
            if UnixStream::connect(socket).is_ok() {
                bail!("a flow server already runs on {}", socket.display());
            }
            fs::remove_file(socket)
                .with_context(|| format!("could not remove {}", socket.display()))?;
            UnixListener::bind(socket)
                .with_context(|| format!("could not listen on {}", socket.display()))?
        }
        Err(error) => {
            return Err(error).with_context(|| format!("could not listen on {}", socket.display()));
        }
    };
    fs::set_permissions(socket, fs::Permissions::from_mode(0o600))
        .with_context(|| format!("could not restrict {}", socket.display()))?;
    Ok(listener)
}

fn accept_in_background(listener: UnixListener, events: Sender<Event>) {
    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            if events.send(Event::Accepted(stream)).is_err() {
                break;
            }
        }
    });
}

fn read_in_background(conn: ConnId, mut stream: UnixStream, events: Sender<Event>) {
    thread::spawn(move || {
        while let Ok(Some(message)) = proto::read_msg::<_, ClientMsg>(&mut stream) {
            if events.send(Event::Message(conn, message)).is_err() {
                return;
            }
        }
        let _ = events.send(Event::Gone(conn));
    });
}

fn forward_host_events(events: Sender<Event>) -> Sender<HostEvent> {
    let (host_events, inbox) = mpsc::channel();
    thread::spawn(move || {
        for event in inbox {
            if events.send(Event::Host(event)).is_err() {
                break;
            }
        }
    });
    host_events
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use tempfile::TempDir;

    use super::*;
    use crate::config::Agent;
    use crate::proto::Frame;

    /// A server whose master is `cat`, run on its own thread.
    fn start(dir: &Path) -> (ServerPaths, thread::JoinHandle<Result<()>>) {
        start_with(
            dir,
            Agent {
                command: vec!["cat".into()],
                prompt: String::new(),
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
        };
        let thread_paths = paths.clone();
        let store = Store::at(dir.to_path_buf());
        let config = Config {
            agent: "claude".into(),
            agents,
            phases: crate::config::builtin_phases(),
            master: crate::config::MasterConfig::default(),
        };
        let handle = thread::spawn(move || {
            run(
                &thread_paths,
                store,
                config,
                LaunchEnv::default(),
                &std::env::current_dir().unwrap(),
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

    fn connect(socket: &Path, role: Role) -> UnixStream {
        let mut stream = UnixStream::connect(socket).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        proto::write_msg(
            &mut stream,
            &ClientMsg::Hello {
                version: PROTOCOL_VERSION,
                role,
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

    /// A flow with a worktree at dir/<name>, as `flow start` leaves it.
    fn started_flow(store: &Store, dir: &Path, name: &str, agent: &str) -> String {
        let id = store
            .create(&LogEvent::new(
                0,
                EventKind::Started {
                    name: name.into(),
                    request: "x".into(),
                    mode: crate::event::Mode::Auto,
                },
            ))
            .unwrap();
        let path = dir.join(name);
        fs::create_dir_all(&path).unwrap();
        let workspace = EventKind::Workspace {
            path: path.display().to_string(),
            branch: format!("flow/{name}"),
            slot: 1,
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
    fn opens_a_pane_for_every_started_flow_and_marks_it_hosted() {
        let dir = TempDir::new().unwrap();
        let store = Store::at(dir.path().to_path_buf());
        let cat = Agent {
            command: vec!["cat".into()],
            prompt: String::new(),
            adapter: None,
            resume: Some(vec!["cat".into()]),
        };
        let broken = Agent {
            command: vec!["/nonexistent/flow-agent".into()],
            prompt: String::new(),
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
    fn reopens_an_exited_worker_and_one_whose_flow_ended() {
        let dir = TempDir::new().unwrap();
        let store = Store::at(dir.path().to_path_buf());
        let csv = started_flow(&store, dir.path(), "csv", "claude");
        let old = started_flow(&store, dir.path(), "old", "claude");
        store
            .append(&old, &LogEvent::new(2, EventKind::Ended))
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
        wait_for(&mut ui, &mut panes, &old, Some(&PaneStatus::Running));

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

        let mut first = connect(&paths.socket, Role::Attach { size });
        assert!(matches!(recv(&mut first), ServerMsg::Welcome { .. }));
        let mut second = connect(&paths.socket, Role::Attach { size });
        assert!(matches!(recv(&mut second), ServerMsg::Welcome { .. }));

        let mut told = false;
        while let Ok(Some(message)) = proto::read_msg::<_, ServerMsg>(&mut first) {
            told |= message == ServerMsg::Detached;
        }
        assert!(told);

        let mut old = UnixStream::connect(&paths.socket).unwrap();
        old.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        proto::write_msg(
            &mut old,
            &ClientMsg::Hello {
                version: PROTOCOL_VERSION + 1,
                role: Role::Control,
            },
        )
        .unwrap();
        assert!(matches!(recv(&mut old), ServerMsg::Refused { .. }));

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
                prompt: String::new(),
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
}
