//! `Key`: raw bytes as keys for a modal view, and how long a sequence is

/// A key as a modal view reads it.
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

/// Decodes raw bytes into keys for a modal view.
pub(super) fn decode(bytes: &[u8]) -> Vec<Key> {
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

/// The length of the CSI sequence at the start of `bytes`; None when it is cut off.
pub(super) fn csi_len(bytes: &[u8]) -> Option<usize> {
    if !bytes.starts_with(b"\x1b[") {
        return Some(1);
    }
    bytes[2..]
        .iter()
        .position(|byte| (0x40..=0x7e).contains(byte))
        .map(|end| end + 3)
}

pub(super) fn utf8_len(first: u8) -> usize {
    match first {
        0xf0..=0xff => 4,
        0xe0..=0xef => 3,
        0xc0..=0xdf => 2,
        _ => 1,
    }
}
