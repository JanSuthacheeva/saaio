pub mod host;
pub mod master;
pub mod reconcile;
pub mod terminal;
pub mod watch;

mod conn;
mod panes;

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::fs;
use std::io::ErrorKind;
use std::ops::ControlFlow;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use signal_hook::consts::{SIGHUP, SIGINT, SIGTERM};
use signal_hook::iterator::SignalsInfo;
use signal_hook::iterator::exfiltrator::WithOrigin;
use signal_hook::low_level::signal_name;

use crate::config::Config;
use crate::fold;
use crate::launch::pane::LaunchEnv;
use crate::proto::{ClientMsg, PROTOCOL_VERSION, PaneKey, ServerMsg, Size};
use crate::server_log;
use crate::store::Store;
use crate::view::Board;
use conn::{accept_in_background, bind};
use host::{Host, HostEvent};
use master::MasterRecord;
use panes::forward_host_events;
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

pub(super) type ConnId = u64;

#[derive(Debug)]
enum Event {
    Accepted(UnixStream),
    Message(ConnId, ClientMsg),
    Gone(ConnId),
    Host(HostEvent),
    /// SIGTERM, SIGINT or SIGHUP: stop as StopServer does.
    Signal {
        name: &'static str,
        from: Option<u32>,
    },
}

/// Why the server stops.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Stop {
    /// A UI's StopServer (C-space Q, then y, k or e).
    Asked,
    /// SIGTERM, SIGINT or SIGHUP; `from` is the sender's pid when the system says.
    Signal {
        name: &'static str,
        from: Option<u32>,
    },
}

impl fmt::Display for Stop {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Stop::Asked => write!(f, "StopServer"),
            Stop::Signal {
                name,
                from: Some(pid),
            } => write!(f, "{name} from pid {pid}"),
            Stop::Signal { name, from: None } => write!(f, "{name}"),
        }
    }
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

/// Binds the socket (removing a stale one), opens the master, then loops until StopServer or a
/// stopping signal; logs the start, the reason and the end.
pub fn run(
    paths: &ServerPaths,
    store: Store,
    config: Config,
    env: LaunchEnv,
    cwd: &Path,
    clock: fn() -> u64,
) -> Result<()> {
    let listener = bind(&paths.socket)?;
    server_log::write(clock(), &format!("started (protocol {PROTOCOL_VERSION})"));
    if let Err(error) = master::ensure_instructions(&paths.instructions) {
        server_log::write(clock(), &format!("{error:#}"));
    }
    let (record, start) = MasterRecord::load_or_create(&paths.master, cwd)?;
    let (events, inbox) = mpsc::channel();
    stop_on_signals(events.clone())?;
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
    result?;
    server_log::write(clock(), "stopped");
    Ok(())
}

impl Server {
    /// Runs until something stops the server; why, once its panes are closed.
    fn serve(&mut self, inbox: &Receiver<Event>) -> Result<Stop> {
        loop {
            let watch = WATCH_EVERY.saturating_sub(self.refreshed_at.elapsed());
            let wait = if self.dirty {
                watch.min(FRAME_EVERY.saturating_sub(self.flushed_at.elapsed()))
            } else {
                watch
            };
            match inbox.recv_timeout(wait) {
                Ok(event) => {
                    if let ControlFlow::Break(stop) = self.handle(event) {
                        server_log::write((self.clock)(), &format!("stopping: {stop}"));
                        self.stop(inbox);
                        return Ok(stop);
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
    fn handle(&mut self, event: Event) -> ControlFlow<Stop> {
        match event {
            Event::Accepted(stream) => self.accept(stream),
            Event::Message(conn, message) => return self.message(conn, message),
            Event::Gone(conn) => self.drop_conn(conn),
            Event::Signal { name, from } => return ControlFlow::Break(Stop::Signal { name, from }),
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

    /// Closes every pane and waits for them to exit; forgets the master, so the next start is a new one.
    fn stop(&mut self, inbox: &Receiver<Event>) {
        if let Err(error) = fs::remove_file(&self.master_file)
            && error.kind() != ErrorKind::NotFound
        {
            server_log::write(
                (self.clock)(),
                &format!("could not forget the master: {error}"),
            );
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

/// SIGTERM, SIGINT and SIGHUP stop the server as StopServer does, with who sent them.
fn stop_on_signals(events: Sender<Event>) -> Result<()> {
    let mut signals = SignalsInfo::<WithOrigin>::new([SIGTERM, SIGINT, SIGHUP])
        .context("could not watch for SIGTERM, SIGINT and SIGHUP")?;
    spawn_named("signals".to_string(), move || {
        for origin in signals.forever() {
            let event = Event::Signal {
                name: signal_name(origin.signal).unwrap_or("a signal"),
                from: origin
                    .process
                    .and_then(|process| u32::try_from(process.pid).ok()),
            };
            if events.send(event).is_err() {
                return;
            }
        }
    });
    Ok(())
}

/// Starts a thread with a name, so a panic in it says where it happened.
pub(super) fn spawn_named<F: FnOnce() + Send + 'static>(name: String, body: F) {
    thread::Builder::new()
        .name(name)
        .spawn(body)
        .expect("the system could not start a thread");
}

#[cfg(test)]
mod tests;
