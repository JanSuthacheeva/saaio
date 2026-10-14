pub mod host;
pub mod master;
pub mod terminal;

use std::collections::HashMap;
use std::fs;
use std::io::ErrorKind;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};

use crate::config::Config;
use crate::launch::LaunchEnv;
use crate::proto::{self, ClientMsg, PROTOCOL_VERSION, PaneKey, PaneStatus, Role, ServerMsg, Size};
use crate::store::Store;
use host::{Host, HostEvent};
use master::MasterRecord;

/// The shortest time between two frames of the focused pane (60 a second).
const FRAME_EVERY: Duration = Duration::from_millis(16);
/// How long a stop waits for the panes to exit.
const STOP_WAIT: Duration = Duration::from_millis(3500);
/// A UI that does not take a frame within this time is dropped, so it cannot stall the server.
const WRITE_TIMEOUT: Duration = Duration::from_secs(2);
/// The size panes start with before a UI says how big it is.
const START_SIZE: Size = Size {
    cols: 120,
    rows: 40,
};

pub struct ServerPaths {
    pub socket: PathBuf,
    pub master: PathBuf,
}

type ConnId = u64;

enum Event {
    Accepted(UnixStream),
    Message(ConnId, ClientMsg),
    Gone(ConnId),
    Host(HostEvent),
}

struct Conn {
    stream: UnixStream,
    role: Option<Role>,
}

struct Server {
    config: Config,
    env: LaunchEnv,
    record: MasterRecord,
    host: Host,
    conns: HashMap<ConnId, Conn>,
    next_conn: ConnId,
    attached: Option<ConnId>,
    focus: PaneKey,
    history: Option<u32>,
    dirty: bool,
    flushed_at: Instant,
    events: Sender<Event>,
}

/// Binds the socket (removing a stale one), opens the master, then loops until StopServer.
pub fn run(
    paths: &ServerPaths,
    _store: Store,
    config: Config,
    env: LaunchEnv,
    _clock: fn() -> u64,
) -> Result<()> {
    let listener = bind(&paths.socket)?;
    let cwd = std::env::current_dir().context("could not read the current directory")?;
    let (record, fresh) = MasterRecord::load_or_create(&paths.master, &cwd)?;
    let (events, inbox) = mpsc::channel();
    let host = Host::new(START_SIZE, forward_host_events(events.clone()));
    let mut server = Server {
        config,
        env,
        record,
        host,
        conns: HashMap::new(),
        next_conn: 0,
        attached: None,
        focus: PaneKey::Master,
        history: None,
        dirty: false,
        flushed_at: Instant::now(),
        events: events.clone(),
    };
    server.open_master(fresh);
    accept_in_background(listener, events);

    let result = server.serve(&inbox);
    let _ = fs::remove_file(&paths.socket);
    result
}

impl Server {
    fn serve(&mut self, inbox: &Receiver<Event>) -> Result<()> {
        loop {
            let wait = FRAME_EVERY.saturating_sub(self.flushed_at.elapsed());
            match inbox.recv_timeout(if self.dirty {
                wait
            } else {
                Duration::from_secs(1)
            }) {
                Ok(event) => {
                    if !self.handle(event)? {
                        return self.stop(inbox);
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => bail!("the server lost its event channel"),
            }
            self.flush();
        }
    }

    /// One event; Ok(false) once the server should stop.
    fn handle(&mut self, event: Event) -> Result<bool> {
        match event {
            Event::Accepted(stream) => self.accept(stream),
            Event::Message(conn, message) => return Ok(self.message(conn, message)),
            Event::Gone(conn) => self.drop_conn(conn),
            Event::Host(HostEvent::Output(key, bytes)) => {
                self.host.feed(&key, &bytes);
                if key == self.focus {
                    self.dirty = true;
                }
            }
            Event::Host(HostEvent::Exited(key, code)) => {
                self.host.exited(&key, code);
                self.send_panes();
                if key == self.focus {
                    self.dirty = true;
                }
            }
        }
        Ok(true)
    }

    fn message(&mut self, conn: ConnId, message: ClientMsg) -> bool {
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
                self.open_master(false);
                self.send_panes();
                self.send_full_frame();
            }
            ClientMsg::Reopen(PaneKey::Flow(_)) | ClientMsg::CloseFlow(_) => {}
            ClientMsg::StopServer => return false,
        }
        true
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
            self.host.resize(size);
        }
        if let Some(entry) = self.conns.get_mut(&conn) {
            entry.role = Some(role);
        }
        self.send_to(
            conn,
            &ServerMsg::Welcome {
                version: PROTOCOL_VERSION,
            },
        );
        if self.attached == Some(conn) {
            self.send_panes();
            self.send_full_frame();
        }
    }

    fn open_master(&mut self, fresh: bool) {
        let Some((name, agent)) = self.config.agent(None) else {
            let reason = format!("no agent named {}", self.config.agent);
            self.host.fail(PaneKey::Master, reason);
            return;
        };
        match self.record.spec(fresh, agent, &self.env) {
            Some(spec) => {
                if let Err(error) = self.host.open(spec) {
                    eprintln!("could not start the master: {error:#}");
                }
            }
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

    fn send_panes(&mut self) {
        if let Some(conn) = self.attached {
            let panes = self.host.panes();
            self.send_to(conn, &ServerMsg::Panes(panes));
        }
    }

    /// Writes one message; a connection that cannot take it is dropped.
    fn send_to(&mut self, conn: ConnId, message: &ServerMsg) {
        let Some(entry) = self.conns.get_mut(&conn) else {
            return;
        };
        if proto::write_msg(&mut entry.stream, message).is_err() {
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
        self.conns.insert(conn, Conn { stream, role: None });
        read_in_background(conn, reader, self.events.clone());
    }

    fn drop_conn(&mut self, conn: ConnId) {
        if let Some(entry) = self.conns.remove(&conn) {
            let _ = entry.stream.shutdown(std::net::Shutdown::Both);
        }
        if self.attached == Some(conn) {
            self.attached = None;
        }
    }

    /// Closes every pane and waits for them to exit.
    fn stop(&mut self, inbox: &Receiver<Event>) -> Result<()> {
        let keys: Vec<PaneKey> = self.host.panes().into_iter().map(|pane| pane.key).collect();
        for key in &keys {
            self.host.close(key);
        }
        let deadline = Instant::now() + STOP_WAIT;
        while self
            .host
            .panes()
            .iter()
            .any(|pane| pane.status == PaneStatus::Running)
        {
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
        Ok(())
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
        loop {
            match proto::read_msg::<_, ClientMsg>(&mut stream) {
                Ok(Some(message)) => {
                    if events.send(Event::Message(conn, message)).is_err() {
                        return;
                    }
                }
                Ok(None) | Err(_) => {
                    let _ = events.send(Event::Gone(conn));
                    return;
                }
            }
        }
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
        let paths = ServerPaths {
            socket: dir.join("server.sock"),
            master: dir.join("master.json"),
        };
        let thread_paths = ServerPaths {
            socket: paths.socket.clone(),
            master: paths.master.clone(),
        };
        let store = Store::at(dir.to_path_buf());
        let config = Config {
            agent: "claude".into(),
            agents: BTreeMap::from([(
                "claude".to_string(),
                Agent {
                    command: vec!["cat".into()],
                    prompt: String::new(),
                    resume: Some(vec!["cat".into()]),
                },
            )]),
        };
        let handle =
            thread::spawn(move || run(&thread_paths, store, config, LaunchEnv::default(), || 0));
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
        assert!(paths.master.exists());
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
    fn replaces_a_stale_socket_but_not_a_live_one() {
        let dir = TempDir::new().unwrap();
        let socket = dir.path().join("server.sock");
        drop(UnixListener::bind(&socket).unwrap());

        let listener = bind(&socket).unwrap();

        let mode = fs::metadata(&socket).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        let error = bind(&socket).unwrap_err();
        assert!(error.to_string().contains("already runs"), "{error}");
        drop(listener);
    }
}
