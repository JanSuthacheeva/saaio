use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::ui::theme::Theme;

/// Marks `area` as where the keys go: theme.ui.selection over it and theme.glyphs.cursor in
/// theme.ui.accent down its first column. Drawn after the content, which leaves that column blank.
pub fn mark(buf: &mut Buffer, area: Rect, theme: &Theme) {
    let area = area.intersection(buf.area);
    if area.is_empty() {
        return;
    }
    light(buf, area, theme);
    for y in area.top()..area.bottom() {
        buf[(area.x, y)]
            .set_symbol(theme.glyphs.cursor)
            .set_style(theme.ui.accent);
    }
}

/// Lights `area` without claiming the keys: the fill alone, for the peek and a row that just changed.
pub fn light(buf: &mut Buffer, area: Rect, theme: &Theme) {
    buf.set_style(area.intersection(buf.area), theme.ui.selection);
}

#[cfg(test)]
mod tests {
    use ratatui::style::Color;

    use super::*;
    use crate::ui::theme::Palette;

    fn filled(text: &str) -> Buffer {
        let area = Rect::new(0, 0, 6, 3);
        let mut buf = Buffer::empty(area);
        for y in 0..3 {
            buf.set_string(0, y, text, ratatui::style::Style::default());
        }
        buf
    }

    #[test]
    fn fills_the_area_and_draws_the_bar_down_its_first_column() {
        let theme = Theme::default();
        let mut buf = filled(" abcde");

        mark(&mut buf, Rect::new(0, 1, 6, 2), &theme);

        for x in 0..6 {
            assert_eq!(buf[(x, 0)].bg, Color::Reset, "the row above is left alone");
            for y in 1..3 {
                assert_eq!(buf[(x, y)].bg, Color::Indexed(236), "({x}, {y})");
            }
        }
        for y in 1..3 {
            assert_eq!(buf[(0, y)].symbol(), "▌");
            assert_eq!(buf[(0, y)].fg, Color::Indexed(173));
            assert_eq!(buf[(1, y)].symbol(), "a", "the content keeps its symbols");
            assert_eq!(buf[(1, y)].fg, Color::Reset, "and its colors");
        }
        assert_eq!(buf[(0, 0)].symbol(), " ");
    }

    #[test]
    fn lights_without_a_bar() {
        let theme = Theme::default();
        let mut buf = filled(" abcde");

        light(&mut buf, Rect::new(0, 0, 6, 1), &theme);

        assert_eq!(buf[(0, 0)].symbol(), " ");
        assert_eq!(buf[(0, 0)].bg, Color::Indexed(236));
        assert_eq!(buf[(5, 0)].bg, Color::Indexed(236));
    }

    #[test]
    fn takes_its_colors_from_the_theme() {
        for palette in [Palette::TERMINAL, Palette::FLOW, Palette::FLOW_LIGHT] {
            let theme = Theme::from_palette(palette);
            let mut buf = filled(" abcde");

            mark(&mut buf, Rect::new(0, 0, 6, 1), &theme);

            assert_eq!(buf[(0, 0)].fg, palette.accent);
            assert_eq!(buf[(0, 0)].bg, palette.selection);
            assert_eq!(buf[(3, 0)].bg, palette.selection);
        }
    }

    #[test]
    fn stays_inside_the_buffer() {
        let theme = Theme::default();
        let mut buf = filled(" abcde");

        mark(&mut buf, Rect::new(4, 2, 10, 10), &theme);
        mark(&mut buf, Rect::new(2, 0, 0, 3), &theme);

        assert_eq!(buf[(4, 2)].symbol(), "▌");
        assert_eq!(buf[(2, 0)].symbol(), "b", "an empty area marks nothing");
    }
}
