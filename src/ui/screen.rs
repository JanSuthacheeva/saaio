use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color as TuiColor, Modifier, Style};
use ratatui::widgets::Widget;

use crate::proto::{Cell, Color, Flags, Frame, Modes, PaneKey};

/// The client's copy of the focused pane, kept up to date by the server's frames.
#[derive(Debug, Default)]
pub struct Screen {
    pub key: Option<PaneKey>,
    pub rows: Vec<Vec<Cell>>,
    pub cursor: Option<(u16, u16)>,
    pub modes: Modes,
    pub history: Option<u32>,
}

impl Screen {
    pub fn apply(&mut self, frame: Frame) {
        if frame.full || self.key.as_ref() != Some(&frame.key) {
            self.rows.clear();
            self.key = Some(frame.key);
        }
        for (row, cells) in frame.rows {
            let row = row as usize;
            if self.rows.len() <= row {
                self.rows.resize(row + 1, Vec::new());
            }
            self.rows[row] = cells;
        }
        self.cursor = frame.cursor;
        self.modes = frame.modes;
        self.history = frame.history;
    }
}

/// The focused pane, its default-colored cells in the theme's text and background.
#[derive(Debug)]
pub struct ScreenView<'a> {
    pub screen: &'a Screen,
    pub text: TuiColor,
    pub background: TuiColor,
}

impl Widget for ScreenView<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        for (y, cells) in self
            .screen
            .rows
            .iter()
            .enumerate()
            .take(area.height as usize)
        {
            for (x, cell) in cells.iter().enumerate().take(area.width as usize) {
                let Some(target) = buf.cell_mut((area.x + x as u16, area.y + y as u16)) else {
                    continue;
                };
                if cell.flags.has(Flags::SPACER) {
                    // ratatui skips the column a wide character covers when it draws.
                    target.set_symbol("");
                    continue;
                }
                let symbol = if cell.text.is_empty() {
                    " "
                } else {
                    &cell.text
                };
                target
                    .set_symbol(symbol)
                    .set_style(style(cell, self.text, self.background));
            }
        }
    }
}

fn style(cell: &Cell, text: TuiColor, background: TuiColor) -> Style {
    let pairs = [
        (Flags::BOLD, Modifier::BOLD),
        (Flags::DIM, Modifier::DIM),
        (Flags::ITALIC, Modifier::ITALIC),
        (Flags::UNDERLINE, Modifier::UNDERLINED),
        (Flags::REVERSE, Modifier::REVERSED),
        (Flags::STRIKE, Modifier::CROSSED_OUT),
    ];
    let modifier = pairs
        .iter()
        .filter(|(flag, _)| cell.flags.has(*flag))
        .fold(Modifier::empty(), |all, (_, modifier)| all | *modifier);
    Style::default()
        .fg(color(cell.fg, text))
        .bg(color(cell.bg, background))
        .add_modifier(modifier)
}

fn color(color: Color, default: TuiColor) -> TuiColor {
    match color {
        Color::Default => default,
        Color::Indexed(index) => TuiColor::Indexed(index),
        Color::Rgb(r, g, b) => TuiColor::Rgb(r, g, b),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell(text: &str, flags: u8) -> Cell {
        Cell {
            text: text.into(),
            fg: Color::Indexed(2),
            bg: Color::Default,
            flags: Flags(flags),
        }
    }

    fn frame(key: PaneKey, full: bool, rows: Vec<(u16, Vec<Cell>)>) -> Frame {
        Frame {
            key,
            full,
            rows,
            cursor: Some((1, 0)),
            modes: Modes::default(),
            history: None,
        }
    }

    #[test]
    fn replaces_only_the_rows_a_partial_frame_carries() {
        let mut screen = Screen::default();
        screen.apply(frame(
            PaneKey::Master,
            true,
            vec![(0, vec![cell("a", 0)]), (1, vec![cell("b", 0)])],
        ));

        screen.apply(frame(PaneKey::Master, false, vec![(1, vec![cell("c", 0)])]));

        assert_eq!(screen.rows[0][0].text, "a");
        assert_eq!(screen.rows[1][0].text, "c");
        screen.apply(frame(
            PaneKey::Flow("x".into()),
            false,
            vec![(1, vec![cell("d", 0)])],
        ));
        assert!(screen.rows[0].is_empty(), "another pane starts afresh");
    }

    #[test]
    fn draws_cells_with_their_style_and_wide_characters_once() {
        let mut screen = Screen::default();
        screen.apply(frame(
            PaneKey::Master,
            true,
            vec![(
                0,
                vec![
                    cell("x", Flags::BOLD),
                    cell("界", Flags::WIDE),
                    cell("", Flags::SPACER),
                    cell("y", 0),
                ],
            )],
        ));
        let area = Rect::new(2, 1, 6, 2);
        let mut buf = Buffer::empty(Rect::new(0, 0, 10, 4));

        ScreenView {
            screen: &screen,
            text: TuiColor::Reset,
            background: TuiColor::Reset,
        }
        .render(area, &mut buf);

        assert_eq!(buf[(2, 1)].symbol(), "x");
        assert!(buf[(2, 1)].modifier.contains(Modifier::BOLD));
        assert_eq!(buf[(2, 1)].fg, TuiColor::Indexed(2));
        assert_eq!(buf[(3, 1)].symbol(), "界");
        assert_eq!(buf[(4, 1)].symbol(), "");
        assert_eq!(buf[(5, 1)].symbol(), "y");
    }

    #[test]
    fn draws_default_colored_cells_in_the_themes_colors() {
        let mut screen = Screen::default();
        let mut plain = cell("a", 0);
        plain.fg = Color::Default;
        screen.apply(frame(
            PaneKey::Master,
            true,
            vec![(0, vec![plain, cell("b", 0)])],
        ));
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 1));
        let (text, background) = (TuiColor::Rgb(200, 200, 208), TuiColor::Rgb(20, 20, 24));

        ScreenView {
            screen: &screen,
            text,
            background,
        }
        .render(Rect::new(0, 0, 4, 1), &mut buf);

        assert_eq!(buf[(0, 0)].fg, text);
        assert_eq!(buf[(0, 0)].bg, background);
        assert_eq!(
            buf[(1, 0)].fg,
            TuiColor::Indexed(2),
            "a color the program set stays"
        );
    }
}
