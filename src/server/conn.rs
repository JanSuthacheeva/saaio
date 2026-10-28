//! UI connections (`Conn`): the socket, hello, messages, and the frames, board and panes sent

use std::fs;
use std::io::{self, ErrorKind, Read};
use std::ops::ControlFlow;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::mpsc::Sender;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use rustix::io::Errno;

use super::master::Start;
use super::outbox::Outbox;
use super::{ConnId, Event, FRAME_EVERY, Server, Stop, spawn_named};

/// At most this many connections at once; one more is closed.
const MAX_CONNS: usize = 16;
/// How long accept rests after an error such as EMFILE before it tries again.
const ACCEPT_PAUSE: Duration = Duration::from_millis(100);
use crate::binaries::{BUILD, Binary};
use crate::private;
use crate::proto::{self, ClientMsg, PROTOCOL_VERSION, PaneKey, Peer, Role, ServerMsg};
use crate::server_log;

/// One connection: its socket, the queue its messages leave by, and who it said it is and in
/// which role once its hello came.
#[derive(Debug)]
pub(super) struct Conn {
    /// Kept to shut its reading side when the connection is dropped.
    stream: UnixStream,
    outbox: Outbox,
    peer: Option<Peer>,
    role: Option<Role>,
}

impl Server {
    pub(super) fn message(&mut self, conn: ConnId, message: ClientMsg) -> ControlFlow<Stop> {
        let Some(open) = self.conns.get(&conn) else {
            return ControlFlow::Continue(());
        };
        if !proto::allowed(open.role, &message) {
            let who = match (&open.peer, open.role) {
                (Some(peer), Some(Role::Control)) => format!("a control connection of {peer}"),
                (Some(peer), _) => peer.to_string(),
                (None, _) => "a connection before its hello".to_string(),
            };
            let what = message.name();
            server_log::write((self.clock)(), &format!("refused {what} from {who}"));
            self.drop_conn(conn);
            return ControlFlow::Continue(());
        }
        match message {
            ClientMsg::Hello {
                version,
                role,
                peer,
            } => {
                let role = match role {
                    Role::Attach { size } => Role::Attach {
                        size: size.clamped(),
                    },
                    Role::Control => Role::Control,
                };
                self.hello(conn, version, role, peer.printable());
            }
            ClientMsg::Focus(key) => {
                self.focus = key;
                self.history = None;
                self.send_full_frame();
            }
            ClientMsg::Input(bytes) => {
                let focus = self.focus.clone();
                self.pane_mut(&focus).typed_at = Some(Instant::now());
                if let Err(error) = self.host.write(&self.focus, &bytes) {
                    server_log::write((self.clock)(), &format!("{error:#}"));
                }
            }
            ClientMsg::Resize(size) => {
                self.host.resize(size.clamped());
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
            ClientMsg::StopServer => {
                let peer = self.conns.get(&conn).and_then(|open| open.peer.clone());
                return ControlFlow::Break(Stop::Asked(peer));
            }
            ClientMsg::EndFlows => self.end_flows(),
        }
        ControlFlow::Continue(())
    }

    fn hello(&mut self, conn: ConnId, version: u32, role: Role, peer: Peer) {
        if version != PROTOCOL_VERSION {
            server_log::write(
                (self.clock)(),
                &format!("refused {peer}: protocol {version}"),
            );
            self.send_to(
                conn,
                &ServerMsg::Refused {
                    reason: format!(
                        "saaio server {BUILD} from {} speaks protocol {PROTOCOL_VERSION}, this saaio speaks {version}",
                        self.env.flow_exe.display()
                    ),
                },
            );
            self.drop_conn(conn);
            return;
        }
        if let Role::Attach { size } = role {
            let line = match self.attached.replace(conn) {
                Some(old) => {
                    let dropped = self.conns.get_mut(&old).and_then(|open| open.peer.take());
                    self.send_to(old, &ServerMsg::Detached { by: peer.clone() });
                    self.drop_conn(old);
                    match dropped {
                        Some(dropped) => format!("ui attached: {peer}; dropped {dropped}"),
                        None => format!("ui attached: {peer}; dropped another"),
                    }
                }
                None => format!("ui attached: {peer}"),
            };
            server_log::write((self.clock)(), &line);
            // A UI starts on the master, live; the server follows, whatever the last UI looked at.
            self.focus = PaneKey::Master;
            self.history = None;
            self.host.resize(size);
        }
        if let Some(open) = self.conns.get_mut(&conn) {
            open.peer = Some(peer);
            open.role = Some(role);
        }
        self.send_to(
            conn,
            &ServerMsg::Welcome {
                version: PROTOCOL_VERSION,
                server: Binary {
                    exe: self.env.flow_exe.clone(),
                    build: BUILD.to_string(),
                },
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

    pub(super) fn accept(&mut self, stream: UnixStream) {
        let mine = rustix::process::getuid().as_raw();
        match proto::peer_uid(&stream) {
            Ok(uid) if uid == mine => {}
            Ok(uid) => {
                server_log::write((self.clock)(), &format!("refused a process of uid {uid}"));
                return;
            }
            Err(error) => {
                let line = format!("refused a connection whose user is unknown: {error}");
                server_log::write((self.clock)(), &line);
                return;
            }
        }
        if self.conns.len() >= MAX_CONNS {
            let line = format!("refused a connection: {MAX_CONNS} are open");
            server_log::write((self.clock)(), &line);
            return;
        }
        let conn = self.next_conn;
        self.next_conn += 1;
        let (Ok(reader), Ok(writer)) = (stream.try_clone(), stream.try_clone()) else {
            return;
        };
        let outbox = match Outbox::start(conn, writer, self.timing.write, self.events.clone()) {
            Ok(outbox) => outbox,
            Err(error) => {
                let line = format!("dropped a connection: {error}");
                server_log::write((self.clock)(), &line);
                return;
            }
        };
        self.conns.insert(
            conn,
            Conn {
                stream,
                outbox,
                peer: None,
                role: None,
            },
        );
        if let Err(error) = read_in_background(conn, reader, self.timing.hello, self.events.clone())
        {
            let line = format!("dropped a connection: {error}");
            server_log::write((self.clock)(), &line);
            self.drop_conn(conn);
        }
    }

    /// Refuses a connection whose first message could not be read: a flow of an older protocol,
    /// which then says to run saaio --stop.
    pub(super) fn refuse_unreadable(&mut self, conn: ConnId) {
        server_log::write((self.clock)(), "refused an unreadable hello");
        self.send_to(
            conn,
            &ServerMsg::Refused {
                reason: format!(
                    "saaio server {BUILD} from {} speaks protocol {PROTOCOL_VERSION} and could not read this saaio's hello",
                    self.env.flow_exe.display()
                ),
            },
        );
        self.drop_conn(conn);
    }

    /// Forgets a connection and shuts its reading side; its Outbox still writes what was queued
    /// before (Refused, Detached), then closes it. The attached UI leaving is logged.
    pub(super) fn drop_conn(&mut self, conn: ConnId) {
        let gone = self.conns.remove(&conn);
        if let Some(gone) = &gone {
            let _ = gone.stream.shutdown(std::net::Shutdown::Read);
        }
        let peer = gone.and_then(|gone| gone.peer);
        if self.attached == Some(conn) {
            self.attached = None;
            let who = peer.map_or_else(|| "a ui".to_string(), |peer| peer.to_string());
            server_log::write((self.clock)(), &format!("ui left: {who}"));
        }
    }

    /// Queues one message; a connection that cannot take it is logged and dropped.
    pub(super) fn send_to(&mut self, conn: ConnId, message: &ServerMsg) {
        let Some(open) = self.conns.get(&conn) else {
            return;
        };
        if let Err(error) = open.outbox.send(message) {
            let who = open
                .peer
                .as_ref()
                .map_or_else(|| "a connection".to_string(), ToString::to_string);
            server_log::write((self.clock)(), &format!("dropped {who}: {error:#}"));
            self.drop_conn(conn);
        }
    }

    /// Sends the text to the attached UI; over MAX_MESSAGE it is logged and not sent.
    pub(super) fn send_clipboard(&mut self, text: String) {
        let Some(conn) = self.attached else {
            return;
        };
        // Room for the message's tag and the text's length.
        if text.len() > proto::MAX_MESSAGE - 16 {
            let line = format!("a pane copied {} bytes, too many to pass on", text.len());
            server_log::write((self.clock)(), &line);
            return;
        }
        self.send_to(conn, &ServerMsg::Clipboard(text));
    }

    /// Sends the focused pane's changed rows, at most 60 times a second.
    pub(super) fn flush(&mut self) {
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

    /// Where the master runs and where its turn stands, when that changed since the attached UI
    /// last heard it.
    pub(super) fn send_master(&mut self) {
        let Some(conn) = self.attached else {
            return;
        };
        let master = ServerMsg::Master {
            cwd: self.record.cwd.display().to_string(),
            branch: crate::workspace::git::current_branch(&self.record.cwd),
            turn: self.store.master_turn(),
        };
        if self.sent_master.as_ref() == Some(&master) {
            return;
        }
        self.send_to(conn, &master);
        self.sent_master = Some(master);
    }

    /// The board, when its flows changed since the attached UI last got it.
    pub(super) fn send_board(&mut self) {
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

    pub(super) fn send_panes(&mut self) {
        if let Some(conn) = self.attached {
            let panes = self.host.panes();
            self.send_to(conn, &ServerMsg::Panes(panes));
        }
    }
}

/// Binds the socket readable only by this user, in a folder of this user closed to others; a
/// socket nobody answers on is stale and replaced.
pub(super) fn bind(socket: &Path) -> Result<UnixListener> {
    if let Some(dir) = socket.parent() {
        private::create_dir(dir).with_context(|| format!("could not create {}", dir.display()))?;
        private::verify_dir(dir)
            .context("saaio listens only in a folder of this user closed to others")?;
    }
    let listener = match UnixListener::bind(socket) {
        Ok(listener) => listener,
        Err(error) if error.kind() == ErrorKind::AddrInUse => {
            if UnixStream::connect(socket).is_ok() {
                bail!("a saaio server already runs on {}", socket.display());
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

/// Accepts connections on the "accept" thread; after an error it rests, so running out of files
/// or memory never spins, and logs the first of a run of them.
pub(super) fn accept_in_background(
    listener: UnixListener,
    events: Sender<Event>,
    clock: fn() -> u64,
) -> Result<()> {
    spawn_named("accept".to_string(), move || {
        let mut failing = false;
        for stream in listener.incoming() {
            match stream {
                Ok(stream) => {
                    failing = false;
                    if events.send(Event::Accepted(stream)).is_err() {
                        break;
                    }
                }
                Err(error) => {
                    let Some(pause) = accept_pause(&error) else {
                        continue;
                    };
                    if !failing {
                        let line = format!("could not accept a connection: {error}; tries again");
                        server_log::write(clock(), &line);
                        failing = true;
                    }
                    thread::sleep(pause);
                }
            }
        }
    })
    .context("could not start the thread that accepts connections")
}

/// How long accept rests after `error`: not at all after a client that gave up or a signal
/// (ECONNABORTED, EINTR), else ACCEPT_PAUSE, so running out of files or memory never spins.
fn accept_pause(error: &io::Error) -> Option<Duration> {
    match Errno::from_io_error(error) {
        Some(Errno::CONNABORTED | Errno::INTR) => None,
        _ => Some(ACCEPT_PAUSE),
    }
}

/// Reads a connection's messages into `events`; one whose hello does not come within `hello`, or
/// cannot be read, is Unreadable.
fn read_in_background(
    conn: ConnId,
    mut stream: UnixStream,
    hello: Duration,
    events: Sender<Event>,
) -> io::Result<()> {
    spawn_named(format!("conn {conn}"), move || {
        let deadline = Instant::now() + hello;
        let first = proto::read_msg::<_, ClientMsg>(&mut Until {
            stream: &mut stream,
            deadline,
        });
        // macOS refuses socket options once the client hung up (EINVAL); the read says so.
        let _ = stream.set_read_timeout(None);
        let mut next = match first {
            Ok(Some(message)) => message,
            Ok(None) => {
                let _ = events.send(Event::Gone(conn));
                return;
            }
            Err(_) => {
                let _ = events.send(Event::Unreadable(conn));
                return;
            }
        };
        loop {
            if events.send(Event::Message(conn, next)).is_err() {
                return;
            }
            match proto::read_msg::<_, ClientMsg>(&mut stream) {
                Ok(Some(message)) => next = message,
                Ok(None) | Err(_) => break,
            }
        }
        let _ = events.send(Event::Gone(conn));
    })
}

/// A stream whose reads time out at one deadline, however slowly its bytes come.
struct Until<'a> {
    stream: &'a mut UnixStream,
    deadline: Instant,
}

impl Read for Until<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let left = self.deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(ErrorKind::TimedOut.into());
        }
        let _ = self.stream.set_read_timeout(Some(left));
        self.stream.read(buf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rests_after_running_out_of_files_but_not_after_a_client_that_gave_up() {
        for errno in [Errno::MFILE, Errno::NFILE, Errno::NOBUFS, Errno::NOMEM] {
            let error = io::Error::from_raw_os_error(errno.raw_os_error());
            assert_eq!(accept_pause(&error), Some(ACCEPT_PAUSE), "{error}");
        }
        for errno in [Errno::CONNABORTED, Errno::INTR] {
            let error = io::Error::from_raw_os_error(errno.raw_os_error());
            assert_eq!(accept_pause(&error), None, "{error}");
        }
    }
}
