use std::io::ErrorKind;
use std::ops::ControlFlow;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::mpsc::Sender;
use std::time::Instant;
use std::{fs, thread};

use anyhow::{Context, Result, bail};

use super::master::Start;
use super::{ConnId, Event, FRAME_EVERY, Server, WRITE_TIMEOUT};
use crate::proto::{self, ClientMsg, PROTOCOL_VERSION, PaneKey, Role, ServerMsg};

impl Server {
    pub(super) fn message(&mut self, conn: ConnId, message: ClientMsg) -> ControlFlow<()> {
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

    pub(super) fn drop_conn(&mut self, conn: ConnId) {
        if let Some(stream) = self.conns.remove(&conn) {
            let _ = stream.shutdown(std::net::Shutdown::Both);
        }
        if self.attached == Some(conn) {
            self.attached = None;
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

    /// Where the master runs, when that changed since the attached UI last heard it.
    pub(super) fn send_master(&mut self) {
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

pub(super) fn accept_in_background(listener: UnixListener, events: Sender<Event>) {
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
