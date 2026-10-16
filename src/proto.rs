use std::fmt;
use std::io::{ErrorKind, Read, Write};
use std::ops::BitOr;

use anyhow::{Context, Result, bail};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::view::Board;

pub const PROTOCOL_VERSION: u32 = 1;
pub const MAX_MESSAGE: usize = 8 << 20;

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

/// Attach draws the panes; Control sends one request (flow remove, flow ui --stop).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Role {
    Attach { size: Size },
    Control,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClientMsg {
    Hello {
        version: u32,
        role: Role,
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
    /// Another flow ui attached.
    Detached,
    /// CloseFlow found no pane for the flow.
    NotOpen(String),
    /// Where the master runs: its directory and the branch checked out there.
    Master {
        cwd: String,
        branch: Option<String>,
    },
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
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Modes {
    pub paste: bool,
    pub mouse: Mouse,
    pub sgr_mouse: bool,
    pub app_cursor: bool,
    pub focus: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
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

    use super::*;
    use crate::view::VIEW_VERSION;

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

    fn client_messages() -> Vec<ClientMsg> {
        let size = Size {
            cols: 120,
            rows: 40,
        };
        vec![
            ClientMsg::Hello {
                version: PROTOCOL_VERSION,
                role: Role::Attach { size },
            },
            ClientMsg::Hello {
                version: PROTOCOL_VERSION,
                role: Role::Control,
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
                },
                history: None,
            }),
            ServerMsg::Closed("a".into()),
            ServerMsg::Detached,
            ServerMsg::NotOpen("b".into()),
            ServerMsg::Master {
                cwd: "/shop".into(),
                branch: Some("main".into()),
            },
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
