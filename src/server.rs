pub mod host;
pub mod master;
pub mod reconcile;
pub mod terminal;
pub mod watch;

mod conn;
mod forge_watch;
mod news;
mod panes;
mod startup;
mod typing;

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

use crate::adapter::Adapter;
use crate::config::watch::ConfigWatch;
use crate::config::{self, Config};
use crate::fold;
use crate::launch::pane::LaunchEnv;
use crate::proto::{BUILD, ClientMsg, PROTOCOL_VERSION, PaneKey, Peer, ServerMsg, Size};
use crate::server_log;
use crate::store::Store;
use crate::ui::theme_file;
use crate::view::Board;
use conn::{accept_in_background, bind};
use forge_watch::{Answer, ForgeWatch};
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
    /// Where saaio writes the example config.toml; None without a config location.
    pub config: Option<PathBuf>,
    /// Where saaio writes the example theme.toml, beside config.toml.
    pub theme: Option<PathBuf>,
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
    /// A connection whose first message could not be read: a flow of an older protocol.
    Unreadable(ConnId),
    /// What gh or glab said about a flow's merge request.
    Forge(Answer),
}

/// Why the server stops.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Stop {
    /// A UI's StopServer (the prefix and Q, then y, k or e); None when its hello never came.
    Asked(Option<Peer>),
    /// SIGTERM, SIGINT or SIGHUP; `from` is the sender's pid when the system says.
    Signal {
        name: &'static str,
        from: Option<u32>,
    },
}

impl fmt::Display for Stop {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Stop::Asked(Some(peer)) => write!(f, "StopServer from {peer}"),
            Stop::Asked(None) => write!(f, "StopServer"),
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
    /// config.toml, looked at before every refresh.
    configs: ConfigWatch,
    env: LaunchEnv,
    record: MasterRecord,
    master_file: PathBuf,
    instructions: PathBuf,
    host: Host,
    /// The adapter each open pane started with.
    adapters: HashMap<PaneKey, Adapter>,
    conns: HashMap<ConnId, UnixStream>,
    /// Who each connection said it is.
    peers: HashMap<ConnId, Peer>,
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
    /// When each pane last got a key from the person, so typed news waits until they pause.
    typed_at: HashMap<PaneKey, Instant>,
    /// When each pane opened, so nothing is typed into a harness still starting up.
    opened_at: HashMap<PaneKey, Instant>,
    /// When the server last typed into each pane, for a line that begins no turn.
    line_at: HashMap<PaneKey, Instant>,
    /// When to ask gh and glab about the flows' merge requests.
    forges: ForgeWatch,
}

/// Writes master.md and, when missing, the example config.toml and theme.toml; logs what fails.
fn write_defaults(paths: &ServerPaths, clock: fn() -> u64) {
    let files = [
        (master::INSTRUCTIONS, Some(&paths.instructions)),
        (config::EXAMPLE, paths.config.as_ref()),
        (theme_file::EXAMPLE, paths.theme.as_ref()),
    ];
    for (file, path) in files {
        if let Some(path) = path
            && let Err(error) = file.ensure(path)
        {
            server_log::write(clock(), &format!("{error:#}"));
        }
    }
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
    server_log::write(
        clock(),
        &format!("started (saaio {BUILD}, protocol {PROTOCOL_VERSION})"),
    );
    write_defaults(paths, clock);
    let mut configs = ConfigWatch::new(paths.config.clone());
    // The server was handed this config; the watch only needs to know the file as it is.
    let _ = configs.changed();
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
        configs,
        env,
        record,
        master_file: paths.master.clone(),
        instructions: paths.instructions.clone(),
        host,
        adapters: HashMap::new(),
        conns: HashMap::new(),
        peers: HashMap::new(),
        next_conn: 0,
        attached: None,
        focus: PaneKey::Master,
        history: None,
        resumed: HashMap::new(),
        closing: HashMap::new(),
        dirty: false,
        flushed_at: Instant::now(),
        events: events.clone(),
        typed_at: HashMap::new(),
        opened_at: HashMap::new(),
        line_at: HashMap::new(),
        forges: ForgeWatch::default(),
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
                self.deliver_news();
                self.refresh();
                self.notice_stalled_starts();
                self.ask_forges();
                self.type_lines();
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
            Event::Unreadable(conn) => self.refuse_unreadable(conn),
            Event::Signal { name, from } => return ControlFlow::Break(Stop::Signal { name, from }),
            Event::Forge(answer) => self.forge_answer(answer),
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
