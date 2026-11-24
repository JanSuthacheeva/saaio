//! The messages between the UI and `saaio server` as length-prefixed postcard; `Peer`

use std::borrow::Cow;
use std::fmt;
use std::io::{self, ErrorKind, Read, Write};
use std::ops::BitOr;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;

use nix::sys::socket::{getsockopt, sockopt};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::binaries::Binary;
use crate::id::FlowId;
use crate::text;
use crate::view::{Board, MasterTurn};

pub const PROTOCOL_VERSION: u32 = 9;
pub const MAX_MESSAGE: usize = 8 << 20;
/// The widest and tallest pane the server makes: past it a terminal's cells cost too much.
pub const MAX_SIZE: Size = Size {
    cols: 1000,
    rows: 500,
};
/// The longest folder or build a peer keeps, in chars.
const PEER_TEXT_LIMIT: usize = 4096;

/// The process at the other end: what the log and a dropped UI say about it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Peer {
    pub pid: u32,
    pub cwd: PathBuf,
    pub build: String,
}

impl Peer {
    /// This peer with its folder and build stripped of control characters and cut to
    /// PEER_TEXT_LIMIT chars: what a client claims about itself, safe for server.log and another
    /// UI's terminal.
    pub fn printable(self) -> Peer {
        let clean = |text: &str| -> String {
            text::printable(text)
                .chars()
                .take(PEER_TEXT_LIMIT)
                .collect()
        };
        Peer {
            pid: self.pid,
            cwd: PathBuf::from(clean(&self.cwd.to_string_lossy())),
            build: clean(&self.build),
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

/// The uid of the process at the other end of a Unix socket.
#[cfg(target_os = "macos")]
pub fn peer_uid(stream: &UnixStream) -> io::Result<u32> {
    Ok(getsockopt(stream, sockopt::LocalPeerCred)?.uid())
}

/// The uid of the process at the other end of a Unix socket.
#[cfg(target_os = "linux")]
pub fn peer_uid(stream: &UnixStream) -> io::Result<u32> {
    Ok(getsockopt(stream, sockopt::PeerCredentials)?.uid())
}

/// Which session a pane runs: the master, or the worker of a flow id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum PaneKey {
    Master,
    Flow(FlowId),
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

impl Size {
    /// This size within 1..=MAX_SIZE.cols by 1..=MAX_SIZE.rows.
    pub fn clamped(self) -> Size {
        Size {
            cols: self.cols.clamp(1, MAX_SIZE.cols),
            rows: self.rows.clamp(1, MAX_SIZE.rows),
        }
    }
}

/// Attach draws the panes; Control sends one request (saaio remove, saaio clear).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Role {
    Attach { size: Size },
    Control,
}

/// Whether a connection in `role` (None before its hello) may send `message`: a hello first and
/// once, then anything from a UI and only CloseFlow from saaio remove and clear.
pub fn allowed(role: Option<Role>, message: &ClientMsg) -> bool {
    match (role, message) {
        (None, ClientMsg::Hello { .. }) => true,
        (_, ClientMsg::Hello { .. }) | (None, _) => false,
        (Some(Role::Attach { .. }), _) => true,
        (Some(Role::Control), message) => matches!(message, ClientMsg::CloseFlow(_)),
    }
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
    CloseFlow(FlowId),
    StopServer,
    /// Ends every unfinished flow (an Ended event), so none reopens; the UI sends it before StopServer.
    EndFlows,
}

impl ClientMsg {
    /// The message's kind, as server.log names it: Hello, StopServer, CloseFlow.
    pub fn name(&self) -> &'static str {
        match self {
            ClientMsg::Hello { .. } => "Hello",
            ClientMsg::Focus(_) => "Focus",
            ClientMsg::Input(_) => "Input",
            ClientMsg::Resize(_) => "Resize",
            ClientMsg::History(_) => "History",
            ClientMsg::Reopen(_) => "Reopen",
            ClientMsg::CloseFlow(_) => "CloseFlow",
            ClientMsg::StopServer => "StopServer",
            ClientMsg::EndFlows => "EndFlows",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ServerMsg {
    Welcome {
        version: u32,
        /// The server's build and the binary it started from.
        server: Binary,
    },
    Refused {
        reason: String,
    },
    Board(Board),
    Panes(Vec<PaneInfo>),
    Frame(Frame),
    /// The flow's pane exited after CloseFlow.
    Closed(FlowId),
    /// Another saaio ui attached: the process `by`.
    Detached {
        by: Peer,
    },
    /// CloseFlow found no pane for the flow.
    NotOpen(FlowId),
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

/// Why a message could not be written or read.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error("message of {0} bytes is over the limit")]
    TooLarge(usize),
    #[error("the stream ended inside a message")]
    Cut,
    #[error("could not encode a message")]
    Encode(#[source] postcard::Error),
    #[error("could not decode a message")]
    Decode(#[source] postcard::Error),
}

impl Error {
    /// The read's deadline passed (WouldBlock or TimedOut): the stream is fine, it was quiet.
    pub fn timed_out(&self) -> bool {
        matches!(self, Error::Io(io) if matches!(io.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut))
    }
}

/// A message as its u32 length and postcard bytes, ready to write; Err past MAX_MESSAGE.
pub fn encode<T: Serialize>(message: &T) -> Result<Vec<u8>, Error> {
    let body = postcard::to_stdvec(message).map_err(Error::Encode)?;
    if body.len() > MAX_MESSAGE {
        return Err(Error::TooLarge(body.len()));
    }
    let len = u32::try_from(body.len()).expect("MAX_MESSAGE fits in u32");
    let mut bytes = Vec::with_capacity(4 + body.len());
    bytes.extend(len.to_le_bytes());
    bytes.extend(body);
    Ok(bytes)
}

/// Each message as a u32 length followed by its postcard bytes.
pub fn write_msg<W: Write, T: Serialize>(writer: &mut W, message: &T) -> Result<(), Error> {
    writer.write_all(&encode(message)?)?;
    writer.flush()?;
    Ok(())
}

/// The next message; None on a clean end of stream; Err on a message over MAX_MESSAGE or bytes
/// that do not decode. The buffer grows with the bytes that arrive, so a length alone allocates
/// nothing.
pub fn read_msg<R: Read, T: DeserializeOwned>(reader: &mut R) -> Result<Option<T>, Error> {
    let mut len = [0u8; 4];
    if !read_full(reader, &mut len)? {
        return Ok(None);
    }
    let len = u32::from_le_bytes(len) as usize;
    if len > MAX_MESSAGE {
        return Err(Error::TooLarge(len));
    }
    let mut bytes = Vec::new();
    reader.take(len as u64).read_to_end(&mut bytes)?;
    if bytes.len() < len {
        return Err(Error::Cut);
    }
    let message = postcard::from_bytes(&bytes).map_err(Error::Decode)?;
    Ok(Some(message))
}

/// Fills buf; false when the stream ended before the first byte, Err when it ended in the middle.
fn read_full<R: Read>(reader: &mut R, buf: &mut [u8]) -> Result<bool, Error> {
    let mut filled = 0;
    while filled < buf.len() {
        match reader.read(&mut buf[filled..]) {
            Ok(0) if filled == 0 => return Ok(false),
            Ok(0) => return Err(Error::Cut),
            Ok(read) => filled += read,
            Err(error) if error.kind() == ErrorKind::Interrupted => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests;
