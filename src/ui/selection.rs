//! `Selection`: text selected with the mouse, read and highlighted

use ratatui::buffer::Buffer;
use ratatui::layout::{Position, Rect};
use ratatui::style::Modifier;
use unicode_width::UnicodeWidthStr;

/// How far a selection reaches past the cells under it: a drag, a double-click, a triple-click.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    Cell,
    Word,
    Line,
}

/// Text selected with the mouse on one surface, in screen cells, read like a terminal reads it:
/// from the first cell to the end of its row, whole rows between, the last row up to its cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    bounds: Rect,
    anchor: Position,
    head: Position,
    unit: Unit,
}

impl Selection {
    pub fn new(bounds: Rect, at: Position, unit: Unit) -> Selection {
        let at = clamp(bounds, at);
        Selection {
            bounds,
            anchor: at,
            head: at,
            unit,
        }
    }

    /// Moves the free end to `to`, clamped into bounds.
    pub fn extend(&mut self, to: Position) {
        self.head = clamp(self.bounds, to);
    }

    /// A press and release on one cell with Unit::Cell: a click, nothing selected.
    pub fn is_click(&self) -> bool {
        self.unit == Unit::Cell && self.anchor == self.head
    }

    /// Rows in reading order, each with trailing blanks trimmed, joined by newlines; Word and Line
    /// widen the ends in `buf`. Wide-char spacer cells are skipped.
    pub fn text(&self, buf: &Buffer) -> String {
        let Some((start, end)) = self.span(buf) else {
            return String::new();
        };
        let mut rows = Vec::new();
        for y in start.y..=end.y {
            let (from, to) = self.columns(y, start, end);
            let mut row = String::new();
            let mut x = from;
            while x <= to {
                let symbol = symbol(buf, Position { x, y });
                row.push_str(symbol);
                x += u16::try_from(symbol.width().max(1)).unwrap_or(1);
            }
            rows.push(row.trim_end().to_string());
        }
        rows.join("\n")
    }

    /// Reverses the selected cells.
    pub fn render(&self, buf: &mut Buffer) {
        let Some((start, end)) = self.span(buf) else {
            return;
        };
        for y in start.y..=end.y {
            let (from, to) = self.columns(y, start, end);
            for x in from..=to {
                if let Some(cell) = buf.cell_mut(Position { x, y }) {
                    cell.modifier.toggle(Modifier::REVERSED);
                }
            }
        }
    }

    /// The first and last selected cell, widened to words or lines; None for a word on a blank.
    fn span(&self, buf: &Buffer) -> Option<(Position, Position)> {
        let (mut start, mut end) = if (self.anchor.y, self.anchor.x) <= (self.head.y, self.head.x) {
            (self.anchor, self.head)
        } else {
            (self.head, self.anchor)
        };
        let left = self.bounds.x;
        let right = self.bounds.right() - 1;
        match self.unit {
            Unit::Cell => {}
            Unit::Line => {
                start.x = left;
                end.x = right;
            }
            Unit::Word => {
                if start == end && !is_word(symbol(buf, start)) {
                    return None;
                }
                while start.x > left
                    && is_word(symbol(buf, start))
                    && is_word(symbol(
                        buf,
                        Position {
                            x: start.x - 1,
                            ..start
                        },
                    ))
                {
                    start.x -= 1;
                }
                while end.x < right
                    && is_word(symbol(buf, end))
                    && is_word(symbol(
                        buf,
                        Position {
                            x: end.x + 1,
                            ..end
                        },
                    ))
                {
                    end.x += 1;
                }
            }
        }
        Some((start, end))
    }

    /// The columns selected on row `y`, inclusive.
    fn columns(&self, y: u16, start: Position, end: Position) -> (u16, u16) {
        let from = if y == start.y { start.x } else { self.bounds.x };
        let to = if y == end.y {
            end.x
        } else {
            self.bounds.right() - 1
        };
        (from, to)
    }
}

fn clamp(bounds: Rect, at: Position) -> Position {
    Position {
        x: at.x.clamp(bounds.x, bounds.right().saturating_sub(1)),
        y: at.y.clamp(bounds.y, bounds.bottom().saturating_sub(1)),
    }
}

fn symbol(buf: &Buffer, at: Position) -> &str {
    buf.cell(at).map_or(" ", |cell| cell.symbol())
}

/// Part of a word: neither blank, a wide character's spacer nor box drawing.
fn is_word(symbol: &str) -> bool {
    !symbol.trim().is_empty()
        && !symbol
            .chars()
            .all(|c| ('\u{2500}'..='\u{257f}').contains(&c))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buffer(lines: &[&str]) -> Buffer {
        Buffer::with_lines(lines.iter().copied())
    }

    fn at(x: u16, y: u16) -> Position {
        Position { x, y }
    }

    const ALL: Rect = Rect::new(0, 0, 10, 3);

    #[test]
    fn a_drag_reads_rows_like_a_terminal_and_trims_trailing_blanks() {
        let buf = buffer(&["alpha beta", "gamma     ", "delta eps "]);
        let mut sel = Selection::new(ALL, at(6, 0), Unit::Cell);

        sel.extend(at(2, 2));

        assert_eq!(sel.text(&buf), "beta\ngamma\ndel");
    }

    #[test]
    fn a_backward_drag_reads_the_same_text() {
        let buf = buffer(&["alpha beta", "gamma     ", "delta eps "]);
        let mut sel = Selection::new(ALL, at(2, 2), Unit::Cell);

        sel.extend(at(6, 0));

        assert_eq!(sel.text(&buf), "beta\ngamma\ndel");
    }

    #[test]
    fn a_drag_stays_inside_its_bounds() {
        let buf = buffer(&["ab|cd|ef  ", "gh|ij|kl  ", "          "]);
        let side = Rect::new(3, 0, 2, 2);
        let mut sel = Selection::new(side, at(3, 0), Unit::Cell);

        sel.extend(at(9, 2));

        assert_eq!(sel.text(&buf), "cd\nij");
    }

    #[test]
    fn a_press_and_release_on_one_cell_is_a_click() {
        let mut sel = Selection::new(ALL, at(1, 1), Unit::Cell);
        assert!(sel.is_click());

        sel.extend(at(2, 1));
        assert!(!sel.is_click());
        assert!(!Selection::new(ALL, at(1, 1), Unit::Word).is_click());
    }

    #[test]
    fn a_wide_character_is_read_once() {
        let mut buf = Buffer::empty(ALL);
        buf.set_string(0, 0, "a界b", ratatui::style::Style::default());
        let mut sel = Selection::new(ALL, at(0, 0), Unit::Cell);

        sel.extend(at(9, 0));

        assert_eq!(sel.text(&buf), "a界b");
    }

    #[test]
    fn a_double_click_selects_the_word_under_it() {
        let buf = buffer(&["│ foo-bar │", "          ", "          "]);

        let word = Selection::new(Rect::new(0, 0, 11, 3), at(4, 0), Unit::Word);

        assert_eq!(word.text(&buf), "foo-bar");
    }

    #[test]
    fn a_double_click_on_a_blank_selects_nothing() {
        let buf = buffer(&["foo   bar ", "          ", "          "]);
        let mut out = buf.clone();

        let blank = Selection::new(ALL, at(4, 0), Unit::Word);
        blank.render(&mut out);

        assert_eq!(blank.text(&buf), "");
        assert_eq!(out, buf);
    }

    #[test]
    fn a_triple_click_selects_the_whole_row_of_its_bounds() {
        let buf = buffer(&["ab|cd ef  ", "          ", "          "]);

        let line = Selection::new(Rect::new(3, 0, 5, 1), at(4, 0), Unit::Line);

        assert_eq!(line.text(&buf), "cd ef");
    }

    #[test]
    fn render_reverses_exactly_the_selected_cells() {
        let mut buf = buffer(&["alpha beta", "gamma     ", "delta eps "]);
        let mut sel = Selection::new(ALL, at(8, 0), Unit::Cell);
        sel.extend(at(1, 1));

        sel.render(&mut buf);

        let reversed: Vec<(u16, u16)> = buf
            .content
            .iter()
            .enumerate()
            .filter(|(_, cell)| cell.modifier.contains(Modifier::REVERSED))
            .map(|(i, _)| buf.pos_of(i))
            .collect();
        assert_eq!(reversed, vec![(8, 0), (9, 0), (0, 1), (1, 1)]);
    }
}
