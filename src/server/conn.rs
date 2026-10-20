use std::fs;
use std::io::ErrorKind;
use std::ops::ControlFlow;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::mpsc::Sender;
use std::time::Instant;

use anyhow::{Context, Result, bail};

use super::master::Start;
use super::{ConnId, Event, FRAME_EVERY, Server, Stop, WRITE_TIMEOUT, spawn_named};
use crate::private;
use crate::proto::{self, ClientMsg, PROTOCOL_VERSION, PaneKey, Peer, Role, ServerMsg};
use crate::server_log;

impl Server {
    pub(super) fn message(&mut self, conn: ConnId, message: ClientMsg) -> ControlFlow<Stop> {
        match message {
            ClientMsg::Hello {
                version,
                role,
                peer,
            } => self.hello(conn, version, role, peer.printable()),
            ClientMsg::Focus(key) => {
                self.focus = key;
                self.history = None;
                self.send_full_frame();
            }
            ClientMsg::Input(bytes) => {
                self.typed_at.insert(self.focus.clone(), Instant::now());
                if let Err(error) = self.host.write(&self.focus, &bytes) {
                    server_log::write((self.clock)(), &format!("{error:#}"));
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
            ClientMsg::StopServer => {
                return ControlFlow::Break(Stop::Asked(self.peers.get(&conn).cloned()));
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
                        "saaio server runs protocol {PROTOCOL_VERSION}, this saaio speaks {version}"
                    ),
                },
            );
            self.drop_conn(conn);
            return;
        }
        if let Role::Attach { size } = role {
            let line = match self.attached.replace(conn) {
                Some(old) => {
                    let dropped = self.peers.remove(&old);
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
        self.peers.insert(conn, peer);
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

    pub(super) fn accept(&mut self, stream: UnixStream) {
        let conn = self.next_conn;
        self.next_conn += 1;
        let Ok(reader) = stream.try_clone() else {
            return;
        };
        let _ = stream.set_write_timeout(Some(WRITE_TIMEOUT));
        self.conns.insert(conn, stream);
        read_in_background(conn, reader, self.events.clone());
    }

    /// Refuses a connection whose first message could not be read: a flow of an older protocol,
    /// which then says to run saaio ui --stop.
    pub(super) fn refuse_unreadable(&mut self, conn: ConnId) {
        server_log::write((self.clock)(), "refused an unreadable hello");
        self.send_to(
            conn,
            &ServerMsg::Refused {
                reason: format!(
                    "saaio server runs protocol {PROTOCOL_VERSION} and could not read this flow's hello"
                ),
            },
        );
        self.drop_conn(conn);
    }

    /// Closes a connection and forgets it; the attached UI leaving is logged.
    pub(super) fn drop_conn(&mut self, conn: ConnId) {
        if let Some(stream) = self.conns.remove(&conn) {
            let _ = stream.shutdown(std::net::Shutdown::Both);
        }
        let peer = self.peers.remove(&conn);
        if self.attached == Some(conn) {
            self.attached = None;
            let who = peer.map_or_else(|| "a ui".to_string(), |peer| peer.to_string());
            server_log::write((self.clock)(), &format!("ui left: {who}"));
        }
    }

    /// Writes one message; a connection that cannot take it is dropped.
    pub(super) fn send_to(&mut self, conn: ConnId, message: &ServerMsg) {
        let Some(stream) = self.conns.get_mut(&conn) else {
            return;
        };
        if proto::write_msg(stream, message).is_err() {
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

/// Binds the socket readable only by this user; a socket nobody answers on is stale and replaced.
pub(super) fn bind(socket: &Path) -> Result<UnixListener> {
    if let Some(dir) = socket.parent() {
        private::create_dir(dir).with_context(|| format!("could not create {}", dir.display()))?;
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

pub(super) fn accept_in_background(listener: UnixListener, events: Sender<Event>) {
    spawn_named("accept".to_string(), move || {
        for stream in listener.incoming().flatten() {
            if events.send(Event::Accepted(stream)).is_err() {
                break;
            }
        }
    });
}

fn read_in_background(conn: ConnId, mut stream: UnixStream, events: Sender<Event>) {
    spawn_named(format!("conn {conn}"), move || {
        let mut first = true;
        loop {
            match proto::read_msg::<_, ClientMsg>(&mut stream) {
                Ok(Some(message)) => {
                    first = false;
                    if events.send(Event::Message(conn, message)).is_err() {
                        return;
                    }
                }
                Err(_) if first => {
                    let _ = events.send(Event::Unreadable(conn));
                    return;
                }
                Ok(None) | Err(_) => break,
            }
        }
        let _ = events.send(Event::Gone(conn));
    });
}
