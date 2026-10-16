use std::io::Write;

use ratatui::layout::{Position, Rect};

/// What raw keyboard and mouse bytes from the real terminal turn into.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Routed {
    /// Bytes for the focused pane, unchanged except for mouse coordinates.
    Pane(Vec<u8>),
    /// The key after C-space.
    Command(Command),
    /// Keys for a modal view (a dialog, the board).
    View(Vec<Key>),
    /// A mouse press outside the pane, in terminal cells from 0.
    Click { col: u16, row: u16 },
}

/// Which way C-space j / k moves the peek cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Next,
    Prev,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Focus(u8),
    Cursor(Step),
    Jump,
    Reopen,
    NewFlow,
    Board,
    Rail,
    History,
    Help,
    Detach,
    StopServer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Key {
    Char(char),
    Enter,
    Esc,
    Backspace,
    Tab,
    Up,
    Down,
    Left,
    Right,
    Ctrl(char),
    Other,
}

/// The prefix as a terminal without the kitty keyboard protocol sends it (C-space is NUL).
const PREFIX: u8 = 0x00;
/// The prefix as a terminal with the kitty keyboard protocol sends it.
const KITTY_PREFIX: &[u8] = b"\x1b[32;5u";

/// Splits raw input at the C-space prefix; remembers a prefix whose key has not arrived yet.
#[derive(Debug, Default)]
pub struct Router {
    prefixed: bool,
    pending: Vec<u8>,
}

impl Router {
    /// Splits bytes at C-space (NUL or CSI 32;5u): the next key is a Command, C-space twice sends one.
    /// In a modal view every key is decoded for the view instead. SGR mouse events inside `pane`
    /// are rewritten to pane coordinates; outside it they become Routed::Click for the UI.
    pub fn route(&mut self, bytes: &[u8], modal: bool, pane: Rect) -> Vec<Routed> {
        let mut input = std::mem::take(&mut self.pending);
        input.extend_from_slice(bytes);
        let mut routed = Vec::new();
        let mut plain = Vec::new();
        let mut at = 0;
        while at < input.len() {
            let rest = &input[at..];
            if self.prefixed {
                self.prefixed = false;
                let (command, used) = command_after_prefix(rest);
                at += used;
                match command {
                    AfterPrefix::Command(command) => {
                        flush(&mut plain, modal, &mut routed);
                        routed.push(Routed::Command(command));
                    }
                    AfterPrefix::Literal => plain.push(PREFIX),
                    AfterPrefix::Nothing => {}
                }
                continue;
            }
            if rest[0] == PREFIX {
                self.prefixed = true;
                at += 1;
                continue;
            }
            if rest.starts_with(KITTY_PREFIX) {
                self.prefixed = true;
                at += KITTY_PREFIX.len();
                continue;
            }
            if rest.starts_with(b"\x1b[") {
                match csi_len(rest) {
                    None => {
                        self.pending = rest.to_vec();
                        break;
                    }
                    Some(len) => {
                        let sequence = &rest[..len];
                        match mouse(sequence) {
                            Some(event) => route_mouse(event, pane, &mut plain, &mut routed, modal),
                            None => plain.extend_from_slice(sequence),
                        }
                        at += len;
                        continue;
                    }
                }
            }
            plain.push(rest[0]);
            at += 1;
        }
        flush(&mut plain, modal, &mut routed);
        routed
    }
}

/// Decodes raw bytes into keys for a modal view.
pub fn decode(bytes: &[u8]) -> Vec<Key> {
    let text = String::from_utf8_lossy(bytes);
    let mut chars = text.chars().peekable();
    let mut keys = Vec::new();
    while let Some(c) = chars.next() {
        let key = match c {
            '\r' | '\n' => Key::Enter,
            '\t' => Key::Tab,
            '\x7f' | '\x08' => Key::Backspace,
            '\x1b' => match chars.peek() {
                Some('[' | 'O') => {
                    chars.next();
                    let last = chars.find(|next| next.is_ascii_alphabetic() || *next == '~');
                    match last {
                        Some('A') => Key::Up,
                        Some('B') => Key::Down,
                        Some('C') => Key::Right,
                        Some('D') => Key::Left,
                        _ => Key::Other,
                    }
                }
                _ => Key::Esc,
            },
            c if (c as u32) < 0x20 => Key::Ctrl((b'a' + c as u8 - 1) as char),
            c => Key::Char(c),
        };
        keys.push(key);
    }
    keys
}

#[derive(Debug)]
enum AfterPrefix {
    Command(Command),
    Literal,
    Nothing,
}

/// The command a key after the prefix means, and how many bytes the key took.
fn command_after_prefix(rest: &[u8]) -> (AfterPrefix, usize) {
    if rest[0] == PREFIX {
        return (AfterPrefix::Literal, 1);
    }
    if rest.starts_with(KITTY_PREFIX) {
        return (AfterPrefix::Literal, KITTY_PREFIX.len());
    }
    let command = match rest[0] {
        digit @ b'0'..=b'9' => Command::Focus(digit - b'0'),
        b'j' => Command::Cursor(Step::Next),
        b'k' => Command::Cursor(Step::Prev),
        b'\r' => Command::Jump,
        b'r' => Command::Reopen,
        b'n' => Command::NewFlow,
        b'o' => Command::Board,
        b'b' => Command::Rail,
        b'[' => Command::History,
        b'?' => Command::Help,
        b'd' => Command::Detach,
        b'Q' => Command::StopServer,
        _ => {
            let used = if rest[0] == 0x1b {
                csi_len(rest).unwrap_or(1)
            } else {
                utf8_len(rest[0])
            };
            return (AfterPrefix::Nothing, used.min(rest.len()));
        }
    };
    (AfterPrefix::Command(command), 1)
}

fn flush(plain: &mut Vec<u8>, modal: bool, routed: &mut Vec<Routed>) {
    if plain.is_empty() {
        return;
    }
    let bytes = std::mem::take(plain);
    routed.push(if modal {
        Routed::View(decode(&bytes))
    } else {
        Routed::Pane(bytes)
    });
}

/// The length of the CSI sequence at the start of `bytes`; None when it is cut off.
fn csi_len(bytes: &[u8]) -> Option<usize> {
    if !bytes.starts_with(b"\x1b[") {
        return Some(1);
    }
    bytes[2..]
        .iter()
        .position(|byte| (0x40..=0x7e).contains(byte))
        .map(|end| end + 3)
}

#[derive(Debug, Clone, Copy)]
struct MouseEvent {
    button: u16,
    col: u16,
    row: u16,
    press: bool,
}

/// An SGR mouse report, ESC [ < button ; col ; row M|m, with 1-based coordinates.
fn mouse(sequence: &[u8]) -> Option<MouseEvent> {
    let body = sequence.strip_prefix(b"\x1b[<")?;
    let (last, params) = body.split_last()?;
    if *last != b'M' && *last != b'm' {
        return None;
    }
    let params = std::str::from_utf8(params).ok()?;
    let mut parts = params.split(';').map(|part| part.parse::<u16>().ok());
    let (button, col, row) = (parts.next()??, parts.next()??, parts.next()??);
    Some(MouseEvent {
        button,
        col: col.saturating_sub(1),
        row: row.saturating_sub(1),
        press: *last == b'M',
    })
}

fn route_mouse(
    event: MouseEvent,
    pane: Rect,
    plain: &mut Vec<u8>,
    routed: &mut Vec<Routed>,
    modal: bool,
) {
    if pane.contains(Position::new(event.col, event.row)) && !modal {
        let _ = write!(
            plain,
            "\x1b[<{};{};{}{}",
            event.button,
            event.col - pane.x + 1,
            event.row - pane.y + 1,
            if event.press { 'M' } else { 'm' }
        );
        return;
    }
    let is_left_press = event.press && event.button == 0;
    if is_left_press {
        flush(plain, modal, routed);
        routed.push(Routed::Click {
            col: event.col,
            row: event.row,
        });
    }
}

fn utf8_len(first: u8) -> usize {
    match first {
        0xf0..=0xff => 4,
        0xe0..=0xef => 3,
        0xc0..=0xdf => 2,
        _ => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PANE: Rect = Rect {
        x: 0,
        y: 1,
        width: 80,
        height: 20,
    };

    fn route(router: &mut Router, bytes: &[u8]) -> Vec<Routed> {
        router.route(bytes, false, PANE)
    }

    #[test]
    fn passes_everything_but_the_prefix_through_unchanged() {
        let mut router = Router::default();

        assert_eq!(
            route(&mut router, "ls -la\r\x1b[A界\x1b".as_bytes()),
            vec![Routed::Pane("ls -la\r\x1b[A界\x1b".as_bytes().to_vec())]
        );
    }

    #[test]
    fn turns_the_key_after_c_space_into_a_command() {
        let mut router = Router::default();

        assert_eq!(
            route(&mut router, b"ab\x003cd\x00d"),
            vec![
                Routed::Pane(b"ab".to_vec()),
                Routed::Command(Command::Focus(3)),
                Routed::Pane(b"cd".to_vec()),
                Routed::Command(Command::Detach),
            ]
        );
        assert_eq!(
            route(&mut router, b"\x1b[32;5uQ"),
            vec![Routed::Command(Command::StopServer)]
        );
        assert_eq!(route(&mut router, b"\x00\x00"), vec![Routed::Pane(vec![0])]);
        assert_eq!(route(&mut router, b"\x00x"), vec![]);
    }

    #[test]
    fn remembers_a_prefix_whose_key_comes_in_the_next_read() {
        let mut router = Router::default();

        assert_eq!(route(&mut router, b"\x00"), vec![]);
        assert_eq!(
            route(&mut router, b"j"),
            vec![Routed::Command(Command::Cursor(Step::Next))]
        );
        assert_eq!(route(&mut router, b"j"), vec![Routed::Pane(b"j".to_vec())]);
    }

    #[test]
    fn moves_mouse_reports_into_the_pane_and_keeps_clicks_outside_for_the_ui() {
        let mut router = Router::default();

        assert_eq!(
            route(&mut router, b"\x1b[<0;10;5M\x1b[<64;3;2M"),
            vec![Routed::Pane(b"\x1b[<0;10;4M\x1b[<64;3;1M".to_vec())]
        );
        assert_eq!(
            route(&mut router, b"\x1b[<0;85;3M\x1b[<0;85;3m"),
            vec![Routed::Click { col: 84, row: 2 }]
        );
    }

    #[test]
    fn waits_for_the_rest_of_a_cut_off_sequence() {
        let mut router = Router::default();

        assert_eq!(
            route(&mut router, b"x\x1b[<0;10"),
            vec![Routed::Pane(b"x".to_vec())]
        );
        assert_eq!(
            route(&mut router, b";5M"),
            vec![Routed::Pane(b"\x1b[<0;10;4M".to_vec())]
        );
    }

    #[test]
    fn decodes_keys_for_a_modal_view() {
        let mut router = Router::default();

        assert_eq!(
            router.route("aé\r\x1b\x7f\t\x1b[B\x01".as_bytes(), true, PANE),
            vec![Routed::View(vec![
                Key::Char('a'),
                Key::Char('é'),
                Key::Enter,
                Key::Esc,
                Key::Backspace,
                Key::Tab,
                Key::Down,
                Key::Ctrl('a'),
            ])]
        );
    }
}
