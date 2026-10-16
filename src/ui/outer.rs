use std::fmt::Write as _;
use std::io::{self, Write};
use std::sync::Once;

use anyhow::{Context, Result};
use ratatui::crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::crossterm::{cursor, execute};

use crate::proto::{Modes, Mouse, Size};

/// Turns off every mode a pane may have turned on, so the shell gets back a plain terminal.
const RESET_MODES: &str =
    "\x1b[?2004l\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1006l\x1b[?1l\x1b[?1004l";

/// The real terminal flow ui runs in.
#[derive(Debug)]
pub struct Outer {
    mirrored: Modes,
}

impl Outer {
    /// Raw mode and the alternate screen; restored on Drop, also on panic.
    pub fn enter() -> Result<Outer> {
        install_panic_restore();
        terminal::enable_raw_mode().context("could not put the terminal in raw mode")?;
        execute!(io::stdout(), EnterAlternateScreen).context("could not open the screen")?;
        Ok(Outer {
            mirrored: Modes::default(),
        })
    }

    /// Turns paste, mouse, cursor-key and focus modes on or off to match the focused pane.
    pub fn mirror(&mut self, modes: Modes) -> Result<()> {
        let sequence = mode_changes(self.mirrored, modes);
        if sequence.is_empty() {
            return Ok(());
        }
        let mut stdout = io::stdout();
        stdout.write_all(sequence.as_bytes())?;
        stdout.flush()?;
        self.mirrored = modes;
        Ok(())
    }

    pub fn size(&self) -> Result<Size> {
        let (cols, rows) = terminal::size().context("could not read the terminal size")?;
        Ok(Size { cols, rows })
    }
}

impl Drop for Outer {
    fn drop(&mut self) {
        restore();
    }
}

fn restore() {
    let mut stdout = io::stdout();
    let _ = stdout.write_all(RESET_MODES.as_bytes());
    let _ = execute!(stdout, LeaveAlternateScreen, cursor::Show);
    let _ = terminal::disable_raw_mode();
}

/// A panic restores the terminal before its message prints; the server and its panes keep running.
fn install_panic_restore() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            restore();
            previous(info);
        }));
    });
}

/// The escape sequences that take the real terminal from `from` to `to`.
fn mode_changes(from: Modes, to: Modes) -> String {
    let mut out = String::new();
    let mut set = |on: bool, was: bool, code: &str| {
        if on != was {
            let _ = write!(out, "\x1b[?{code}{}", if on { 'h' } else { 'l' });
        }
    };
    set(to.paste, from.paste, "2004");
    set(to.app_cursor, from.app_cursor, "1");
    set(to.focus, from.focus, "1004");
    set(to.sgr_mouse, from.sgr_mouse, "1006");
    if to.mouse != from.mouse {
        let code = |mouse: Mouse| match mouse {
            Mouse::Off => None,
            Mouse::Press => Some("1000"),
            Mouse::Drag => Some("1002"),
            Mouse::Motion => Some("1003"),
        };
        if let Some(old) = code(from.mouse) {
            let _ = write!(out, "\x1b[?{old}l");
        }
        if let Some(new) = code(to.mouse) {
            let _ = write!(out, "\x1b[?{new}h");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_only_the_modes_that_changed() {
        let off = Modes::default();
        let on = Modes {
            paste: true,
            mouse: Mouse::Motion,
            sgr_mouse: true,
            app_cursor: false,
            focus: true,
        };

        assert_eq!(
            mode_changes(off, on),
            "\x1b[?2004h\x1b[?1004h\x1b[?1006h\x1b[?1003h"
        );
        assert_eq!(mode_changes(on, on), "");
        let drag = Modes {
            mouse: Mouse::Drag,
            ..on
        };
        assert_eq!(mode_changes(on, drag), "\x1b[?1003l\x1b[?1002h");
        assert_eq!(
            mode_changes(on, off),
            "\x1b[?2004l\x1b[?1004l\x1b[?1006l\x1b[?1003l"
        );
    }
}
