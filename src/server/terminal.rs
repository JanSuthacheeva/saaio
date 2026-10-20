use std::fmt;
use std::sync::{Arc, Mutex};

use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::cell::Flags as CellFlags;
use alacritty_terminal::term::{Config, Term, TermDamage, TermMode};
use alacritty_terminal::vte::ansi::{Color as AnsiColor, NamedColor, Processor, Rgb};

use crate::proto::{Cell, Color, Flags, Modes, Mouse, Size};

/// Foreground and background a program is told when it asks (OSC 10/11).
const ASKED_FOREGROUND: Rgb = Rgb {
    r: 0xd0,
    g: 0xd0,
    b: 0xd0,
};
const ASKED_BACKGROUND: Rgb = Rgb {
    r: 0x14,
    g: 0x14,
    b: 0x18,
};

/// One pane's screen: the emulator fed with what its program writes.
pub struct Terminal {
    term: Term<Listener>,
    parser: Processor,
    listener: Listener,
}

impl fmt::Debug for Terminal {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Terminal")
            .field("listener", &self.listener)
            .finish_non_exhaustive()
    }
}

/// Collects what the emulator wants written back to the program, and what it copied.
#[derive(Debug, Clone)]
struct Listener {
    replies: Arc<Mutex<Vec<u8>>>,
    size: Arc<Mutex<Size>>,
    copied: Arc<Mutex<Option<String>>>,
}

impl EventListener for Listener {
    fn send_event(&self, event: Event) {
        let reply = match event {
            Event::PtyWrite(text) => text,
            Event::ColorRequest(index, format) => format(asked_color(index)),
            Event::TextAreaSizeRequest(format) => {
                let size = *self.size.lock().expect("size lock");
                format(alacritty_terminal::event::WindowSize {
                    num_lines: size.rows,
                    num_cols: size.cols,
                    cell_width: 8,
                    cell_height: 16,
                })
            }
            Event::ClipboardStore(_, text) => {
                *self.copied.lock().expect("copied lock") = Some(text);
                return;
            }
            _ => return,
        };
        self.replies
            .lock()
            .expect("replies lock")
            .extend_from_slice(reply.as_bytes());
    }
}

#[derive(Debug)]
struct Dims {
    cols: usize,
    rows: usize,
}

impl Dimensions for Dims {
    fn total_lines(&self) -> usize {
        self.rows
    }

    fn screen_lines(&self) -> usize {
        self.rows
    }

    fn columns(&self) -> usize {
        self.cols
    }
}

impl Terminal {
    pub fn new(size: Size, history: usize) -> Terminal {
        let listener = Listener {
            replies: Arc::default(),
            size: Arc::new(Mutex::new(size)),
            copied: Arc::default(),
        };
        let config = Config {
            scrolling_history: history,
            kitty_keyboard: false,
            ..Config::default()
        };
        let term = Term::new(config, &dims(size), listener.clone());
        Terminal {
            term,
            parser: Processor::new(),
            listener,
        }
    }

    pub fn feed(&mut self, bytes: &[u8]) {
        self.parser.advance(&mut self.term, bytes);
    }

    /// Ends a synchronized update the program left open past its timeout, so its screen shows.
    pub fn expire_sync(&mut self) {
        let expired = self
            .parser
            .sync_timeout()
            .sync_timeout()
            .is_some_and(|deadline| deadline <= std::time::Instant::now());
        if expired {
            self.parser.stop_sync(&mut self.term);
        }
    }

    /// Answers the program asked for (device attributes, cursor position), drained.
    pub fn replies(&mut self) -> Vec<u8> {
        std::mem::take(&mut *self.listener.replies.lock().expect("replies lock"))
    }

    /// The text the program last put on the clipboard (OSC 52), drained.
    pub fn copied(&mut self) -> Option<String> {
        self.listener.copied.lock().expect("copied lock").take()
    }

    pub fn resize(&mut self, size: Size) {
        *self.listener.size.lock().expect("size lock") = size;
        self.term.resize(dims(size));
    }

    /// Screen rows changed since the last call; the cursor's row is always among them,
    /// so ask only after new output.
    pub fn damaged(&mut self) -> Vec<u16> {
        let rows = match self.term.damage() {
            TermDamage::Full => (0..self.term.screen_lines() as u16).collect(),
            TermDamage::Partial(lines) => lines.map(|bounds| bounds.line as u16).collect(),
        };
        self.term.reset_damage();
        rows
    }

    /// One screen row, `history` lines back from the live screen.
    pub fn row(&self, row: u16, history: u32) -> Vec<Cell> {
        let grid = self.term.grid();
        let back = history.min(grid.history_size() as u32) as i32;
        let line = &grid[Line(i32::from(row) - back)];
        (0..grid.columns())
            .map(|col| {
                let cell = &line[Column(col)];
                let mut text = String::new();
                if !cell.flags.intersects(
                    CellFlags::WIDE_CHAR_SPACER
                        | CellFlags::LEADING_WIDE_CHAR_SPACER
                        | CellFlags::HIDDEN,
                ) {
                    text.push(cell.c);
                    if let Some(extra) = cell.zerowidth() {
                        text.extend(extra);
                    }
                }
                Cell {
                    text,
                    fg: color(cell.fg),
                    bg: color(cell.bg),
                    flags: flags(cell.flags),
                }
            })
            .collect()
    }

    /// A small plain-text tail of the visible screen; never reads scrollback.
    pub fn output_tail(&self, max_lines: usize, max_chars: usize) -> String {
        if max_lines == 0 || max_chars == 0 {
            return String::new();
        }
        let mut lines = Vec::new();
        for row in (0..self.term.screen_lines() as u16).rev() {
            let line = self
                .row(row, 0)
                .into_iter()
                .map(|cell| cell.text)
                .collect::<String>();
            let line = line.trim();
            if !line.is_empty() {
                lines.push(line.to_string());
                if lines.len() == max_lines {
                    break;
                }
            }
        }
        lines.reverse();
        let tail = lines.join("\n");
        if tail.chars().count() <= max_chars {
            return tail;
        }
        tail.chars()
            .rev()
            .take(max_chars)
            .collect::<String>()
            .chars()
            .rev()
            .collect::<String>()
            .trim_start_matches('\n')
            .to_string()
    }

    /// Column and row of the cursor; None while the program hides it.
    pub fn cursor(&self) -> Option<(u16, u16)> {
        if !self.term.mode().contains(TermMode::SHOW_CURSOR) {
            return None;
        }
        let point = self.term.grid().cursor.point;
        Some((point.column.0 as u16, point.line.0 as u16))
    }

    pub fn modes(&self) -> Modes {
        let mode = self.term.mode();
        let mouse = if mode.contains(TermMode::MOUSE_MOTION) {
            Mouse::Motion
        } else if mode.contains(TermMode::MOUSE_DRAG) {
            Mouse::Drag
        } else if mode.contains(TermMode::MOUSE_REPORT_CLICK) {
            Mouse::Press
        } else {
            Mouse::Off
        };
        Modes {
            paste: mode.contains(TermMode::BRACKETED_PASTE),
            mouse,
            sgr_mouse: mode.contains(TermMode::SGR_MOUSE),
            app_cursor: mode.contains(TermMode::APP_CURSOR),
            focus: mode.contains(TermMode::FOCUS_IN_OUT),
        }
    }

    pub fn history_len(&self) -> u32 {
        self.term.grid().history_size() as u32
    }
}

fn dims(size: Size) -> Dims {
    Dims {
        cols: size.cols.max(1) as usize,
        rows: size.rows.max(1) as usize,
    }
}

fn color(color: AnsiColor) -> Color {
    match color {
        AnsiColor::Spec(rgb) => Color::Rgb(rgb.r, rgb.g, rgb.b),
        AnsiColor::Indexed(index) => Color::Indexed(index),
        AnsiColor::Named(named) => match named as usize {
            index @ 0..=15 => Color::Indexed(index as u8),
            _ => match named {
                NamedColor::DimBlack => Color::Indexed(0),
                NamedColor::DimRed => Color::Indexed(1),
                NamedColor::DimGreen => Color::Indexed(2),
                NamedColor::DimYellow => Color::Indexed(3),
                NamedColor::DimBlue => Color::Indexed(4),
                NamedColor::DimMagenta => Color::Indexed(5),
                NamedColor::DimCyan => Color::Indexed(6),
                NamedColor::DimWhite => Color::Indexed(7),
                _ => Color::Default,
            },
        },
    }
}

fn flags(cell: CellFlags) -> Flags {
    let pairs = [
        (CellFlags::BOLD, Flags::BOLD),
        (CellFlags::DIM, Flags::DIM),
        (CellFlags::ITALIC, Flags::ITALIC),
        (CellFlags::ALL_UNDERLINES, Flags::UNDERLINE),
        (CellFlags::INVERSE, Flags::REVERSE),
        (CellFlags::STRIKEOUT, Flags::STRIKE),
        (CellFlags::WIDE_CHAR, Flags::WIDE),
        (CellFlags::WIDE_CHAR_SPACER, Flags::SPACER),
    ];
    pairs
        .iter()
        .filter(|(theirs, _)| cell.intersects(*theirs))
        .fold(Flags::default(), |flags, (_, ours)| flags | *ours)
}

/// What a program asking for a palette color is told: the xterm defaults.
fn asked_color(index: usize) -> Rgb {
    const BASE: [(u8, u8, u8); 16] = [
        (0x00, 0x00, 0x00),
        (0xcd, 0x00, 0x00),
        (0x00, 0xcd, 0x00),
        (0xcd, 0xcd, 0x00),
        (0x00, 0x00, 0xee),
        (0xcd, 0x00, 0xcd),
        (0x00, 0xcd, 0xcd),
        (0xe5, 0xe5, 0xe5),
        (0x7f, 0x7f, 0x7f),
        (0xff, 0x00, 0x00),
        (0x00, 0xff, 0x00),
        (0xff, 0xff, 0x00),
        (0x5c, 0x5c, 0xff),
        (0xff, 0x00, 0xff),
        (0x00, 0xff, 0xff),
        (0xff, 0xff, 0xff),
    ];
    match index {
        0..=15 => {
            let (r, g, b) = BASE[index];
            Rgb { r, g, b }
        }
        _ if index == NamedColor::Background as usize => ASKED_BACKGROUND,
        _ => ASKED_FOREGROUND,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn term(cols: u16, rows: u16) -> Terminal {
        Terminal::new(Size { cols, rows }, 100)
    }

    fn text(cells: &[Cell]) -> String {
        cells
            .iter()
            .map(|cell| cell.text.as_str())
            .collect::<String>()
            .trim_end()
            .to_string()
    }

    #[test]
    fn shows_what_the_program_wrote_and_where_the_cursor_is() {
        let mut term = term(20, 4);

        term.feed(b"hello\r\nworld");

        assert_eq!(text(&term.row(0, 0)), "hello");
        assert_eq!(text(&term.row(1, 0)), "world");
        assert_eq!(term.cursor(), Some((5, 1)));
        term.feed(b"\x1b[?25l");
        assert_eq!(term.cursor(), None);
    }

    #[test]
    fn keeps_colors_attributes_and_double_width_characters() {
        let mut term = term(20, 2);

        term.feed("\x1b[1;31mR\x1b[0m\x1b[38;2;1;2;3;48;5;200mx\x1b[0m界!".as_bytes());

        let row = term.row(0, 0);
        assert_eq!(row[0].fg, Color::Indexed(1));
        assert!(row[0].flags.has(Flags::BOLD));
        assert_eq!(row[1].fg, Color::Rgb(1, 2, 3));
        assert_eq!(row[1].bg, Color::Indexed(200));
        assert_eq!(row[2].text, "界");
        assert!(row[2].flags.has(Flags::WIDE));
        assert_eq!(row[3].text, "");
        assert!(row[3].flags.has(Flags::SPACER));
        assert_eq!(row[4].text, "!");
        assert_eq!(row[5].fg, Color::Default);
    }

    #[test]
    fn answers_the_queries_claude_code_sends() {
        let mut term = term(20, 2);

        term.feed(b"\x1b[c\x1b[?2026$p\x1b[6n");

        assert_eq!(
            String::from_utf8(term.replies()).unwrap(),
            "\x1b[?6c\x1b[?2026;2$y\x1b[1;1R"
        );
        assert!(term.replies().is_empty());
    }

    #[test]
    fn keeps_what_the_program_copied_and_never_gives_the_clipboard_away() {
        let mut term = term(20, 2);

        term.feed(b"\x1b]52;c;aGk=\x07");

        assert_eq!(term.copied(), Some("hi".to_string()));
        assert_eq!(term.copied(), None);
        term.feed(b"\x1b]52;p;Ynll\x1b\\");
        assert_eq!(term.copied(), Some("bye".to_string()));
        term.feed(b"\x1b]52;c;?\x07");
        assert!(term.replies().is_empty());
        assert_eq!(term.copied(), None);
    }

    #[test]
    fn reports_the_modes_the_program_turned_on() {
        let mut term = term(20, 2);
        assert_eq!(term.modes(), Modes::default());

        term.feed(b"\x1b[?2004h\x1b[?1002h\x1b[?1006h\x1b[?1h\x1b[?1004h");

        assert_eq!(
            term.modes(),
            Modes {
                paste: true,
                mouse: Mouse::Drag,
                sgr_mouse: true,
                app_cursor: true,
                focus: true,
            }
        );
        term.feed(b"\x1b[?1003h");
        assert_eq!(term.modes().mouse, Mouse::Motion);
    }

    #[test]
    fn names_only_the_rows_that_changed() {
        let mut term = term(10, 5);
        term.damaged();

        term.feed(b"\x1b[3;1Hthird");

        let damaged = term.damaged();
        assert!(damaged.contains(&2), "{damaged:?}");
        assert!(!damaged.contains(&1), "{damaged:?}");
        assert!(!damaged.contains(&4), "{damaged:?}");
        assert_eq!(
            term.damaged(),
            vec![2],
            "only the cursor's row stays marked"
        );
    }

    #[test]
    fn scrolls_old_lines_into_history() {
        let mut term = term(10, 4);

        for line in 0..10 {
            term.feed(format!("line {line}\r\n").as_bytes());
        }

        assert_eq!(term.history_len(), 7);
        assert_eq!(text(&term.row(0, 0)), "line 7");
        assert_eq!(text(&term.row(0, 3)), "line 4");
        assert_eq!(text(&term.row(0, 99)), "line 0");
    }

    #[test]
    fn follows_a_resize() {
        let mut term = term(10, 4);

        term.resize(Size { cols: 30, rows: 2 });
        term.feed(&[b'x'; 25]);

        assert_eq!(term.row(0, 0).len(), 30);
        assert_eq!(text(&term.row(0, 0)), "x".repeat(25));
        term.feed(b"\x1b[18t");
        assert_eq!(String::from_utf8(term.replies()).unwrap(), "\x1b[8;2;30t");
    }

    #[test]
    fn output_tail_uses_rendered_nonempty_visible_rows() {
        let mut term = term(30, 7);
        term.feed(b"old\r\n\x1b[31mred\x1b[0m\r\n\r\nthird\r\nfourth\r\nfifth");

        assert_eq!(term.output_tail(3, 100), "third\nfourth\nfifth");
        assert_eq!(term.output_tail(0, 100), "");
    }

    #[test]
    fn output_tail_bounds_unicode_characters_without_scrollback() {
        let mut term = term(20, 3);
        term.feed("gone\r\n界界\r\nlast".as_bytes());

        assert_eq!(term.output_tail(2, 5), "last");
        assert_eq!(term.output_tail(2, 0), "");
    }
}
