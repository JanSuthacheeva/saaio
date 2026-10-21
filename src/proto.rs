//! The messages between the UI and `saaio server` as length-prefixed postcard; `Peer`

use std::borrow::Cow;
use std::fmt;
use std::io::{ErrorKind, Read, Write};
use std::ops::BitOr;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::text;
use crate::view::{Board, MasterTurn};

pub const PROTOCOL_VERSION: u32 = 7;
/// This binary: `0.1.0 (2904734)`, the commit from build.rs, `unknown` outside a git checkout.
pub const BUILD: &str = concat!(env!("CARGO_PKG_VERSION"), " (", env!("SAAIO_COMMIT"), ")");
pub const MAX_MESSAGE: usize = 8 << 20;

/// The process at the other end: what the log and a dropped UI say about it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Peer {
    pub pid: u32,
    pub cwd: PathBuf,
    pub build: String,
}

impl Peer {
    /// This peer with its folder and build stripped of control characters: what a client claims
    /// about itself, safe for server.log and another UI's terminal.
    pub fn printable(self) -> Peer {
        Peer {
            pid: self.pid,
            cwd: PathBuf::from(text::printable(&self.cwd.to_string_lossy())),
            build: text::printable(&self.build),
        }
    }

    /// Its folder, or "an unknown folder" when the process could not read its own.
    pub fn folder(&self) -> Cow<'_, str> {
        if self.cwd.as_os_str().is_empty() {
            Cow::Borrowed("an unknown folder")
        } else {
            self.cwd.to_string_lossy()
        }
    }
}

/// pid 4321 in /Users/jan/projects/flow (saaio 0.1.0 2904734)
impl fmt::Display for Peer {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "pid {} in {} (saaio {})",
            self.pid,
            self.folder(),
            self.build
        )
    }
}

/// Which session a pane runs: the master, or the worker of a flow id.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum PaneKey {
    Master,
    Flow(String),
}

/// the master, flow 7f3c
impl fmt::Display for PaneKey {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            PaneKey::Master => f.write_str("the master"),
            PaneKey::Flow(id) => write!(f, "flow {id}"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Size {
    pub cols: u16,
    pub rows: u16,
}

/// Attach draws the panes; Control sends one request (saaio remove, saaio clear).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Role {
    Attach { size: Size },
    Control,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClientMsg {
    /// `peer` comes last, so a server of an older protocol still reads the version and refuses.
    Hello {
        version: u32,
        role: Role,
        peer: Peer,
    },
    Focus(PaneKey),
    Input(Vec<u8>),
    Resize(Size),
    /// Lines back into the focused pane's history; None is live.
    History(Option<u32>),
    Reopen(PaneKey),
    CloseFlow(String),
    StopServer,
    /// Ends every unfinished flow (an Ended event), so none reopens; the UI sends it before StopServer.
    EndFlows,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ServerMsg {
    Welcome {
        version: u32,
    },
    Refused {
        reason: String,
    },
    Board(Board),
    Panes(Vec<PaneInfo>),
    Frame(Frame),
    /// The flow's pane exited after CloseFlow.
    Closed(String),
    /// Another saaio ui attached: the process `by`.
    Detached {
        by: Peer,
    },
    /// CloseFlow found no pane for the flow.
    NotOpen(String),
    /// Where the master runs (its directory and the branch checked out there), and where its turn
    /// stands.
    Master {
        cwd: String,
        branch: Option<String>,
        turn: Option<MasterTurn>,
    },
    /// Text the focused pane's program put on the clipboard (OSC 52).
    Clipboard(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaneInfo {
    pub key: PaneKey,
    pub status: PaneStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PaneStatus {
    Running,
    Exited { code: Option<u32> },
    Failed { reason: String },
}

/// Rows of one pane: all of them when full, else only the changed ones.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Frame {
    pub key: PaneKey,
    pub full: bool,
    pub rows: Vec<(u16, Vec<Cell>)>,
    pub cursor: Option<(u16, u16)>,
    pub modes: Modes,
    pub history: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cell {
    pub text: String,
    pub fg: Color,
    pub bg: Color,
    pub flags: Flags,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Color {
    Default,
    Indexed(u8),
    Rgb(u8, u8, u8),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Flags(u8);

impl Flags {
    pub const BOLD: Flags = Flags(1);
    pub const DIM: Flags = Flags(1 << 1);
    pub const ITALIC: Flags = Flags(1 << 2);
    pub const UNDERLINE: Flags = Flags(1 << 3);
    pub const REVERSE: Flags = Flags(1 << 4);
    pub const STRIKE: Flags = Flags(1 << 5);
    /// The first column of a double-width character.
    pub const WIDE: Flags = Flags(1 << 6);
    /// The column a double-width character covers after its first.
    pub const SPACER: Flags = Flags(1 << 7);

    pub fn has(self, flag: Flags) -> bool {
        self.0 & flag.0 != 0
    }
}

impl BitOr for Flags {
    type Output = Flags;

    fn bitor(self, other: Flags) -> Flags {
        Flags(self.0 | other.0)
    }
}

/// The terminal modes a pane's program turned on, mirrored on the real terminal.
#[expect(
    clippy::struct_excessive_bools,
    reason = "independent terminal mode switches"
)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Modes {
    pub paste: bool,
    pub mouse: Mouse,
    pub sgr_mouse: bool,
    pub app_cursor: bool,
    pub focus: bool,
    /// The program is on the alternate screen, where the wheel scrolls it with arrow keys.
    pub alt_screen: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Mouse {
    #[default]
    Off,
    Press,
    Drag,
    Motion,
}

/// Each message as a u32 length followed by its postcard bytes.
pub fn write_msg<W: Write, T: Serialize>(writer: &mut W, message: &T) -> Result<()> {
    let bytes = postcard::to_stdvec(message).context("could not encode a message")?;
    if bytes.len() > MAX_MESSAGE {
        bail!("message of {} bytes is over the limit", bytes.len());
    }
    let len = u32::try_from(bytes.len()).expect("MAX_MESSAGE fits in u32");
    writer.write_all(&len.to_le_bytes())?;
    writer.write_all(&bytes)?;
    writer.flush()?;
    Ok(())
}

/// The next message; None on a clean end of stream; Err on a message over MAX_MESSAGE or bytes that do not decode.
pub fn read_msg<R: Read, T: DeserializeOwned>(reader: &mut R) -> Result<Option<T>> {
    let mut len = [0u8; 4];
    if !read_full(reader, &mut len)? {
        return Ok(None);
    }
    let len = u32::from_le_bytes(len) as usize;
    if len > MAX_MESSAGE {
        bail!("message of {len} bytes is over the limit");
    }
    let mut bytes = vec![0u8; len];
    if !read_full(reader, &mut bytes)? && len > 0 {
        bail!("the stream ended inside a message");
    }
    let message = postcard::from_bytes(&bytes).context("could not decode a message")?;
    Ok(Some(message))
}

/// Fills buf; false when the stream ended before the first byte, Err when it ended in the middle.
fn read_full<R: Read>(reader: &mut R, buf: &mut [u8]) -> Result<bool> {
    let mut filled = 0;
    while filled < buf.len() {
        match reader.read(&mut buf[filled..]) {
            Ok(0) if filled == 0 => return Ok(false),
            Ok(0) => bail!("the stream ended inside a message"),
            Ok(read) => filled += read,
            Err(error) if error.kind() == ErrorKind::Interrupted => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;
    use std::path::Path;

    use super::*;
    use crate::event::{Event, EventKind, Mode};
    use crate::fold;
    use crate::view::{FlowView, VIEW_VERSION};

    #[test]
    fn a_printable_peer_has_no_control_characters() {
        let peer = Peer {
            pid: 7,
            cwd: "/tmp/\x1b]52;c;aGk=\x07x".into(),
            build: "0.1.0\u{9b}2J".into(),
        };

        let clean = peer.printable();

        assert_eq!(clean.pid, 7);
        assert_eq!(clean.cwd, Path::new("/tmp/]52;c;aGk=x"));
        assert_eq!(clean.build, "0.1.02J");
    }

    /// A flow whose harness picked its own session, as a Codex worker's does.
    fn flow_with_session() -> FlowView {
        let events = [
            Event::new(
                1,
                EventKind::Started {
                    name: "csv".into(),
                    request: "add csv".into(),
                    mode: Mode::Guided,
                    phases: None,
                },
            ),
            Event::new(
                2,
                EventKind::Session {
                    id: "01a1".into(),
                    transcript: Some("/home/dev/.codex/sessions/rollout-01a1.jsonl".into()),
                },
            ),
        ];
        fold::fold("7f3c", &events, None, 3).unwrap()
    }

    /// Hands out one byte per read, like a socket under load.
    struct Trickle(Cursor<Vec<u8>>);

    impl Read for Trickle {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            let end = buf.len().min(1);
            self.0.read(&mut buf[..end])
        }
    }

    fn cell(text: &str) -> Cell {
        Cell {
            text: text.into(),
            fg: Color::Indexed(4),
            bg: Color::Rgb(1, 2, 3),
            flags: Flags::BOLD | Flags::WIDE,
        }
    }

    fn peer() -> Peer {
        Peer {
            pid: 4321,
            cwd: "/Users/jan/projects/flow".into(),
            build: "0.1.0 2904734".into(),
        }
    }

    fn client_messages() -> Vec<ClientMsg> {
        let size = Size {
            cols: 120,
            rows: 40,
        };
        vec![
            ClientMsg::Hello {
                version: PROTOCOL_VERSION,
                role: Role::Attach { size },
                peer: peer(),
            },
            ClientMsg::Hello {
                version: PROTOCOL_VERSION,
                role: Role::Control,
                peer: peer(),
            },
            ClientMsg::Focus(PaneKey::Flow("7f3c".into())),
            ClientMsg::Input(vec![0, 27, b'[', b'A']),
            ClientMsg::Resize(size),
            ClientMsg::History(Some(12)),
            ClientMsg::History(None),
            ClientMsg::Reopen(PaneKey::Master),
            ClientMsg::CloseFlow("7f3c".into()),
            ClientMsg::StopServer,
            ClientMsg::EndFlows,
        ]
    }

    fn server_messages() -> Vec<ServerMsg> {
        vec![
            ServerMsg::Welcome {
                version: PROTOCOL_VERSION,
            },
            ServerMsg::Refused {
                reason: "protocol 2".into(),
            },
            ServerMsg::Board(Board {
                version: VIEW_VERSION,
                now: 5,
                count: 0,
                needs_you: 0,
                flows: vec![],
            }),
            ServerMsg::Board(Board {
                version: VIEW_VERSION,
                now: 5,
                count: 1,
                needs_you: 0,
                flows: vec![flow_with_session()],
            }),
            ServerMsg::Panes(vec![
                PaneInfo {
                    key: PaneKey::Master,
                    status: PaneStatus::Running,
                },
                PaneInfo {
                    key: PaneKey::Flow("a".into()),
                    status: PaneStatus::Exited { code: Some(1) },
                },
                PaneInfo {
                    key: PaneKey::Flow("b".into()),
                    status: PaneStatus::Failed {
                        reason: "no agent".into(),
                    },
                },
            ]),
            ServerMsg::Frame(Frame {
                key: PaneKey::Master,
                full: true,
                rows: vec![(0, vec![cell("a"), cell("界")])],
                cursor: Some((3, 0)),
                modes: Modes {
                    paste: true,
                    mouse: Mouse::Drag,
                    sgr_mouse: true,
                    app_cursor: false,
                    focus: true,
                    alt_screen: true,
                },
                history: None,
            }),
            ServerMsg::Closed("a".into()),
            ServerMsg::Detached { by: peer() },
            ServerMsg::NotOpen("b".into()),
            ServerMsg::Master {
                cwd: "/shop".into(),
                branch: Some("main".into()),
                turn: Some(MasterTurn {
                    state: crate::view::State::Waiting,
                    at: 7,
                }),
            },
            ServerMsg::Clipboard("copied".into()),
        ]
    }

    #[test]
    fn every_message_survives_a_round_trip() {
        let mut wire = Vec::new();
        for message in client_messages() {
            write_msg(&mut wire, &message).unwrap();
        }
        let mut reader = Cursor::new(wire);
        for message in client_messages() {
            assert_eq!(
                read_msg::<_, ClientMsg>(&mut reader).unwrap(),
                Some(message)
            );
        }
        assert_eq!(read_msg::<_, ClientMsg>(&mut reader).unwrap(), None);

        let mut wire = Vec::new();
        for message in server_messages() {
            write_msg(&mut wire, &message).unwrap();
        }
        let mut reader = Trickle(Cursor::new(wire));
        for message in server_messages() {
            assert_eq!(
                read_msg::<_, ServerMsg>(&mut reader).unwrap(),
                Some(message)
            );
        }
        assert_eq!(read_msg::<_, ServerMsg>(&mut reader).unwrap(), None);
    }

    #[test]
    fn an_older_server_still_reads_the_version_of_a_newer_hello() {
        #[derive(Deserialize)]
        enum OlderClientMsg {
            Hello { version: u32, role: Role },
        }
        let mut wire = Vec::new();
        write_msg(
            &mut wire,
            &ClientMsg::Hello {
                version: PROTOCOL_VERSION,
                role: Role::Control,
                peer: peer(),
            },
        )
        .unwrap();

        let Some(OlderClientMsg::Hello { version, role }) =
            read_msg(&mut Cursor::new(wire)).unwrap()
        else {
            panic!("no hello");
        };
        assert_eq!((version, role), (PROTOCOL_VERSION, Role::Control));
    }

    #[test]
    fn names_a_peer_by_pid_folder_and_build() {
        assert_eq!(
            peer().to_string(),
            "pid 4321 in /Users/jan/projects/flow (saaio 0.1.0 2904734)"
        );
        let lost = Peer {
            cwd: PathBuf::new(),
            ..peer()
        };
        assert_eq!(
            lost.to_string(),
            "pid 4321 in an unknown folder (saaio 0.1.0 2904734)"
        );
    }

    #[test]
    fn a_stream_cut_inside_a_message_is_an_error() {
        let mut wire = Vec::new();
        write_msg(&mut wire, &ClientMsg::CloseFlow("abc".into())).unwrap();
        wire.truncate(wire.len() - 1);

        assert!(read_msg::<_, ClientMsg>(&mut Cursor::new(wire)).is_err());
        assert!(read_msg::<_, ClientMsg>(&mut Cursor::new(vec![9, 0])).is_err());
    }

    #[test]
    fn refuses_oversized_and_undecodable_messages() {
        let oversized = ((MAX_MESSAGE + 1) as u32).to_le_bytes().to_vec();
        let error = read_msg::<_, ClientMsg>(&mut Cursor::new(oversized)).unwrap_err();
        assert!(error.to_string().contains("over the limit"), "{error}");

        let mut garbage = 3u32.to_le_bytes().to_vec();
        garbage.extend([0xff, 0xff, 0xff]);
        assert!(read_msg::<_, ClientMsg>(&mut Cursor::new(garbage)).is_err());

        let huge = ClientMsg::Input(vec![0; MAX_MESSAGE + 1]);
        assert!(write_msg(&mut Vec::new(), &huge).is_err());
    }
}
