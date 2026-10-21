//! SGR mouse reports read, matched to a pane's mouse mode and re-encoded for the pane

use ratatui::layout::{Position, Rect};

use super::PointerKind;
use crate::proto::Mouse;

#[derive(Debug, Clone, Copy)]
pub(super) struct MouseEvent {
    /// The SGR button code: the button in the low two bits (3 none), 4 shift, 8 alt, 16 control.
    pub(super) button: u16,
    pub(super) col: u16,
    pub(super) row: u16,
    pub(super) press: bool,
    /// The pointer moved (bit 32), with a button held or, in any-motion mode, without one.
    pub(super) motion: bool,
}

impl MouseEvent {
    const WHEEL: u16 = 64;

    pub(super) fn at(self) -> Position {
        Position::new(self.col, self.row)
    }

    fn is_wheel(self) -> bool {
        self.button & Self::WHEEL != 0
    }

    /// Which button, without modifiers: 0 left, 1 middle, 2 right, 3 none.
    fn base(self) -> u16 {
        self.button & 0b11
    }

    /// A button went down: not motion, not the wheel.
    pub(super) fn is_press(self) -> bool {
        self.press && !self.motion && !self.is_wheel()
    }

    /// What the event means to saaio: the left button and the wheel; None for the rest.
    pub(super) fn pointer(self) -> Option<PointerKind> {
        if self.is_wheel() {
            return Some(if self.base() & 1 == 0 {
                PointerKind::WheelUp
            } else {
                PointerKind::WheelDown
            });
        }
        if self.base() != 0 {
            return None;
        }
        Some(match (self.press, self.motion) {
            (false, _) => PointerKind::Release,
            (true, true) => PointerKind::Drag,
            (true, false) => PointerKind::Press,
        })
    }
}

/// An SGR mouse report, ESC [ < button ; col ; row M|m, with 1-based coordinates.
pub(super) fn mouse(sequence: &[u8]) -> Option<MouseEvent> {
    let body = sequence.strip_prefix(b"\x1b[<")?;
    let (last, params) = body.split_last()?;
    if *last != b'M' && *last != b'm' {
        return None;
    }
    let params = std::str::from_utf8(params).ok()?;
    let mut parts = params.split(';').map(|part| part.parse::<u16>().ok());
    let (button, col, row) = (parts.next()??, parts.next()??, parts.next()??);
    Some(MouseEvent {
        button: button & !32,
        col: col.saturating_sub(1),
        row: row.saturating_sub(1),
        press: *last == b'M',
        motion: button & 32 != 0,
    })
}

/// The report in pane coordinates, SGR or X10 as the pane asked; None past X10's 223 columns.
pub(super) fn encode(event: MouseEvent, pane: Rect, sgr: bool) -> Option<Vec<u8>> {
    let col = event.col.clamp(pane.x, pane.right().saturating_sub(1)) - pane.x + 1;
    let row = event.row.clamp(pane.y, pane.bottom().saturating_sub(1)) - pane.y + 1;
    let button = event.button | if event.motion { 32 } else { 0 };
    if sgr {
        let end = if event.press { 'M' } else { 'm' };
        return Some(format!("\x1b[<{button};{col};{row}{end}").into_bytes());
    }
    let button = if event.press || event.is_wheel() {
        button
    } else {
        button | 3
    };
    let byte = |value: u16| u8::try_from(value + 32).ok();
    Some(vec![
        0x1b,
        b'[',
        b'M',
        byte(button)?,
        byte(col)?,
        byte(row)?,
    ])
}

/// Whether a program in `mouse` mode asked for `event`: Press press/release/wheel, Drag also
/// motion with a button, Motion all motion.
pub(super) fn wanted(mouse: Mouse, event: MouseEvent) -> bool {
    match mouse {
        Mouse::Off => false,
        Mouse::Press => !event.motion,
        Mouse::Drag => !event.motion || event.base() != 3,
        Mouse::Motion => true,
    }
}
