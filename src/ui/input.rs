//! `Router`: raw input split at the prefix, mouse events to the pane or saaio

use ratatui::layout::{Position, Rect};

use crate::config::prefix::Prefix;
use crate::proto::Mouse;

mod keys;
mod mouse;

pub use keys::Key;
use keys::{csi_len, decode, utf8_len};
use mouse::{MouseEvent, encode, mouse, wanted};

/// What raw keyboard and mouse bytes from the real terminal turn into.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Routed {
    /// Bytes for the focused pane, unchanged except for mouse coordinates.
    Pane(Vec<u8>),
    /// The key after the prefix.
    Command(Command),
    /// Keys for a modal view (a dialog, the board).
    View(Vec<Key>),
    /// A mouse event for saaio.
    Pointer(Pointer),
}

/// A mouse event saaio handles itself, in terminal cells from 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pointer {
    pub kind: PointerKind,
    pub at: Position,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointerKind {
    Press,
    Drag,
    Release,
    WheelUp,
    WheelDown,
}

/// Where mouse events can go: the focused pane, its mouse modes, and whether a view is open over it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Target {
    pub pane: Rect,
    pub modal: bool,
    pub mouse: Mouse,
    pub sgr: bool,
}

/// Which way the prefix and j / k move the peek cursor.
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

/// Splits raw input at the prefix; remembers a prefix whose key has not arrived yet.
#[derive(Debug)]
pub struct Router {
    /// The prefix as a terminal without the kitty keyboard protocol sends it (C-space is NUL).
    prefix: u8,
    /// The prefix as a terminal with the kitty keyboard protocol sends it.
    kitty: Vec<u8>,
    prefixed: bool,
    pending: Vec<u8>,
    /// Who gets the drag and release of the button held down.
    held: Option<Holder>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Holder {
    Pane,
    Saaio,
}

impl Default for Router {
    fn default() -> Router {
        Router::new(Prefix::default())
    }
}

impl Router {
    pub fn new(prefix: Prefix) -> Router {
        Router {
            prefix: prefix.byte(),
            kitty: prefix.kitty(),
            prefixed: false,
            pending: Vec::new(),
            held: None,
        }
    }

    /// Takes `prefix` from now on; a half-pressed prefix is forgotten.
    pub fn set_prefix(&mut self, prefix: Prefix) {
        *self = Router {
            pending: std::mem::take(&mut self.pending),
            held: self.held,
            ..Router::new(prefix)
        };
    }

    /// Splits bytes at the prefix (its byte or its kitty form): the next key is a Command, the
    /// prefix twice sends one.
    /// In a modal view every key is decoded for the view instead. A mouse event goes to a pane
    /// that uses the mouse when it asked for it, in pane coordinates; saaio takes the rest.
    pub fn route(&mut self, bytes: &[u8], target: Target) -> Vec<Routed> {
        let modal = target.modal;
        let mut input = std::mem::take(&mut self.pending);
        input.extend_from_slice(bytes);
        let mut routed = Vec::new();
        let mut plain = Vec::new();
        let mut at = 0;
        while at < input.len() {
            let rest = &input[at..];
            if self.prefixed {
                self.prefixed = false;
                let (command, used) = self.command_after_prefix(rest);
                at += used;
                match command {
                    AfterPrefix::Command(command) => {
                        flush(&mut plain, modal, &mut routed);
                        routed.push(Routed::Command(command));
                    }
                    AfterPrefix::Literal => plain.push(self.prefix),
                    AfterPrefix::Nothing => {}
                }
                continue;
            }
            if rest[0] == self.prefix {
                self.prefixed = true;
                at += 1;
                continue;
            }
            if rest.starts_with(&self.kitty) {
                self.prefixed = true;
                at += self.kitty.len();
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
                            Some(event) => self.route_mouse(event, target, &mut plain, &mut routed),
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

    /// The command a key after the prefix means, and how many bytes the key took.
    fn command_after_prefix(&self, rest: &[u8]) -> (AfterPrefix, usize) {
        if rest[0] == self.prefix {
            return (AfterPrefix::Literal, 1);
        }
        if rest.starts_with(&self.kitty) {
            return (AfterPrefix::Literal, self.kitty.len());
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

    /// Hands a mouse event to the pane or to saaio. A press decides who gets the drag and the
    /// release after it, even when they leave the pane or come back into it.
    fn route_mouse(
        &mut self,
        event: MouseEvent,
        target: Target,
        plain: &mut Vec<u8>,
        routed: &mut Vec<Routed>,
    ) {
        let mut to_saaio = |kind| {
            flush(plain, target.modal, routed);
            routed.push(Routed::Pointer(Pointer {
                kind,
                at: event.at(),
            }));
        };
        if target.modal {
            if let Some(kind) = event.pointer() {
                to_saaio(kind);
            }
            return;
        }
        if event.is_press() {
            self.held = None;
        }
        match self.held {
            Some(Holder::Pane) => {
                if !event.press {
                    self.held = None;
                }
                if wanted(target.mouse, event)
                    && let Some(bytes) = encode(event, target.pane, target.sgr)
                {
                    plain.extend_from_slice(&bytes);
                }
            }
            Some(Holder::Saaio) => {
                if !event.press {
                    self.held = None;
                    to_saaio(PointerKind::Release);
                } else if event.pointer() == Some(PointerKind::Drag) {
                    to_saaio(PointerKind::Drag);
                }
            }
            None if target.pane.contains(event.at()) && target.mouse != Mouse::Off => {
                if event.is_press() {
                    self.held = Some(Holder::Pane);
                }
                if wanted(target.mouse, event)
                    && let Some(bytes) = encode(event, target.pane, target.sgr)
                {
                    plain.extend_from_slice(&bytes);
                }
            }
            None => match event.pointer() {
                Some(PointerKind::Press) => {
                    self.held = Some(Holder::Saaio);
                    to_saaio(PointerKind::Press);
                }
                Some(wheel @ (PointerKind::WheelUp | PointerKind::WheelDown)) => to_saaio(wheel),
                _ => {}
            },
        }
    }
}

#[derive(Debug)]
enum AfterPrefix {
    Command(Command),
    Literal,
    Nothing,
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

#[cfg(test)]
mod tests {
    use super::*;

    const PANE: Rect = Rect {
        x: 0,
        y: 1,
        width: 80,
        height: 20,
    };

    const MOUSE_PANE: Target = Target {
        pane: PANE,
        modal: false,
        mouse: Mouse::Motion,
        sgr: true,
    };

    const SHELL: Target = Target {
        mouse: Mouse::Off,
        sgr: false,
        ..MOUSE_PANE
    };

    fn route(router: &mut Router, bytes: &[u8]) -> Vec<Routed> {
        router.route(bytes, MOUSE_PANE)
    }

    fn pointer(kind: PointerKind, x: u16, y: u16) -> Routed {
        Routed::Pointer(Pointer {
            kind,
            at: Position::new(x, y),
        })
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
    fn takes_a_new_prefix_and_forgets_a_half_pressed_one() {
        let mut router = Router::default();
        assert_eq!(route(&mut router, b"\x00"), vec![]);

        router.set_prefix(Prefix::try_from("C-b".to_string()).unwrap());

        assert_eq!(route(&mut router, b"d"), vec![Routed::Pane(b"d".to_vec())]);
        assert_eq!(
            route(&mut router, b"\x02d"),
            vec![Routed::Command(Command::Detach)]
        );
        assert_eq!(route(&mut router, b"\x00"), vec![Routed::Pane(vec![0])]);
    }

    #[test]
    fn splits_at_the_configured_prefix_and_passes_c_space_through() {
        let mut router = Router::new(Prefix::try_from("C-b".to_string()).unwrap());

        assert_eq!(
            route(&mut router, b"a\x02d\x00"),
            vec![
                Routed::Pane(b"a".to_vec()),
                Routed::Command(Command::Detach),
                Routed::Pane(vec![0]),
            ]
        );
        assert_eq!(
            route(&mut router, b"\x1b[98;5u?"),
            vec![Routed::Command(Command::Help)]
        );
        assert_eq!(
            route(&mut router, b"\x1b[32;5u"),
            vec![Routed::Pane(b"\x1b[32;5u".to_vec())]
        );
        assert_eq!(route(&mut router, b"\x02\x02"), vec![Routed::Pane(vec![2])]);
        assert_eq!(
            route(&mut router, b"\x02\x1b[98;5u"),
            vec![Routed::Pane(vec![2])]
        );
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
    fn moves_mouse_reports_into_the_pane_and_keeps_presses_outside_for_the_ui() {
        let mut router = Router::default();

        assert_eq!(
            route(&mut router, b"\x1b[<0;10;5M\x1b[<0;10;5m\x1b[<64;3;2M"),
            vec![Routed::Pane(
                b"\x1b[<0;10;4M\x1b[<0;10;4m\x1b[<64;3;1M".to_vec()
            )]
        );
        assert_eq!(
            route(&mut router, b"\x1b[<0;85;3M\x1b[<0;85;3m"),
            vec![
                pointer(PointerKind::Press, 84, 2),
                pointer(PointerKind::Release, 84, 2)
            ]
        );
    }

    #[test]
    fn a_drag_saaio_started_stays_with_saaio_across_the_pane() {
        let mut router = Router::default();

        assert_eq!(
            route(&mut router, b"\x1b[<0;85;3M\x1b[<32;10;5M\x1b[<0;10;5m"),
            vec![
                pointer(PointerKind::Press, 84, 2),
                pointer(PointerKind::Drag, 9, 4),
                pointer(PointerKind::Release, 9, 4),
            ]
        );
        assert_eq!(
            route(&mut router, b"\x1b[<35;10;5M"),
            vec![Routed::Pane(b"\x1b[<35;10;4M".to_vec())]
        );
    }

    #[test]
    fn a_drag_the_pane_started_stays_with_it_clamped_to_its_edge() {
        let mut router = Router::default();

        assert_eq!(
            route(&mut router, b"\x1b[<0;10;5M\x1b[<32;90;30M\x1b[<0;90;30m"),
            vec![Routed::Pane(
                b"\x1b[<0;10;4M\x1b[<32;80;20M\x1b[<0;80;20m".to_vec()
            )]
        );
        assert_eq!(
            route(&mut router, b"\x1b[<0;85;3M"),
            vec![pointer(PointerKind::Press, 84, 2)]
        );
    }

    #[test]
    fn a_pane_gets_only_the_events_its_mode_asked_for() {
        let mut router = Router::default();
        let press = Target {
            mouse: Mouse::Press,
            ..MOUSE_PANE
        };
        let drag = Target {
            mouse: Mouse::Drag,
            ..MOUSE_PANE
        };
        let events = b"\x1b[<35;5;5M\x1b[<0;5;5M\x1b[<32;6;5M\x1b[<0;6;5m";

        assert_eq!(
            router.route(events, press),
            vec![Routed::Pane(b"\x1b[<0;5;4M\x1b[<0;6;4m".to_vec())]
        );
        assert_eq!(
            router.route(events, drag),
            vec![Routed::Pane(
                b"\x1b[<0;5;4M\x1b[<32;6;4M\x1b[<0;6;4m".to_vec()
            )]
        );
    }

    #[test]
    fn a_pane_without_the_mouse_leaves_drags_and_the_wheel_to_saaio() {
        let mut router = Router::default();

        assert_eq!(
            router.route(b"\x1b[<35;5;5M\x1b[<0;5;5M\x1b[<32;7;5M\x1b[<0;7;5m", SHELL),
            vec![
                pointer(PointerKind::Press, 4, 4),
                pointer(PointerKind::Drag, 6, 4),
                pointer(PointerKind::Release, 6, 4),
            ]
        );
        assert_eq!(
            router.route(b"\x1b[<64;5;5M\x1b[<65;85;5M\x1b[<2;5;5M", SHELL),
            vec![
                pointer(PointerKind::WheelUp, 4, 4),
                pointer(PointerKind::WheelDown, 84, 4),
            ]
        );
    }

    #[test]
    fn encodes_x10_for_a_pane_that_did_not_ask_for_sgr() {
        let x10 = Target {
            sgr: false,
            ..MOUSE_PANE
        };
        let mut router = Router::default();

        assert_eq!(
            router.route(b"\x1b[<0;10;5M\x1b[<0;10;5m\x1b[<65;3;2M", x10),
            vec![Routed::Pane(b"\x1b[M *$\x1b[M#*$\x1b[Ma#!".to_vec())]
        );
        let wide = Rect { width: 300, ..PANE };
        assert_eq!(
            router.route(b"\x1b[<0;250;5M", Target { pane: wide, ..x10 }),
            vec![]
        );
    }

    #[test]
    fn a_modal_view_gets_every_left_button_event_and_the_wheel() {
        let mut router = Router::default();
        let modal = Target {
            modal: true,
            ..MOUSE_PANE
        };

        assert_eq!(
            router.route(
                b"\x1b[<0;10;5M\x1b[<32;11;5M\x1b[<35;12;5M\x1b[<0;11;5m\x1b[<65;3;3M\x1b[<2;3;3M",
                modal
            ),
            vec![
                pointer(PointerKind::Press, 9, 4),
                pointer(PointerKind::Drag, 10, 4),
                pointer(PointerKind::Release, 10, 4),
                pointer(PointerKind::WheelDown, 2, 2),
            ]
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
            router.route(
                "aé\r\x1b\x7f\t\x1b[B\x01".as_bytes(),
                Target {
                    modal: true,
                    ..MOUSE_PANE
                }
            ),
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
