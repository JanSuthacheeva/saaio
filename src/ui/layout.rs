use ratatui::layout::Rect;

use crate::proto::Size;

/// Columns of the sidebar and of the rail, without the line that separates them from the pane.
pub const SIDEBAR_WIDTH: u16 = 40;
pub const RAIL_WIDTH: u16 = 6;
/// Cells left blank on every side, so nothing touches the terminal's edge.
pub const MARGIN: u16 = 1;
/// Below this size the margin is left out: every cell is needed.
const MARGIN_FROM: Size = Size { cols: 40, rows: 10 };

/// What surrounds the pane: a bottom bar while the master is alone, else a sidebar or a rail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Chrome {
    Solo,
    Sidebar,
    Rail,
}

/// Where each part of the screen goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Areas {
    /// The row above the pane that names the session.
    pub title: Rect,
    pub pane: Rect,
    /// The line between the pane and the sidebar or rail.
    pub rule: Option<Rect>,
    pub side: Option<Rect>,
    pub bar: Option<Rect>,
}

impl Areas {
    /// The session's whole column: its title and its pane, where dialogs center.
    pub fn session(&self) -> Rect {
        Rect {
            height: self.title.height + self.pane.height,
            ..self.title
        }
    }

    pub fn pane_size(&self) -> Size {
        Size {
            cols: self.pane.width.max(1),
            rows: self.pane.height.max(1),
        }
    }
}

/// The part of the terminal flow draws in: all of it but MARGIN on every side.
pub fn screen(size: Size) -> Rect {
    if size.cols < MARGIN_FROM.cols || size.rows < MARGIN_FROM.rows {
        return Rect::new(0, 0, size.cols, size.rows);
    }
    Rect::new(
        MARGIN,
        MARGIN,
        size.cols - 2 * MARGIN,
        size.rows - 2 * MARGIN,
    )
}

/// Solo: the titled pane, a blank row, the bottom bar; Sidebar: 40 columns on the right of a rule;
/// Rail: 6. All of it within the screen's margin.
pub fn areas(size: Size, chrome: Chrome) -> Areas {
    let screen = screen(size);
    let (x, y, width, height) = (screen.x, screen.y, screen.width, screen.height);
    let title = |width: u16| Rect::new(x, y, width, 1.min(height));
    let pane = |width: u16, below: u16| {
        Rect::new(
            x,
            y + 1.min(height),
            width,
            height.saturating_sub(1 + below),
        )
    };
    match chrome {
        Chrome::Solo => Areas {
            title: title(width),
            pane: pane(width, 2),
            rule: None,
            side: None,
            bar: Some(Rect::new(
                x,
                y + height.saturating_sub(1),
                width,
                1.min(height),
            )),
        },
        Chrome::Sidebar | Chrome::Rail => {
            let side_width = if chrome == Chrome::Sidebar {
                SIDEBAR_WIDTH
            } else {
                RAIL_WIDTH
            };
            let side_width = side_width.min(width.saturating_sub(2));
            let left = width.saturating_sub(side_width + 1);
            Areas {
                title: title(left),
                pane: pane(left, 0),
                rule: Some(Rect::new(x + left, y, 1.min(width), height)),
                side: Some(Rect::new(x + width - side_width, y, side_width, height)),
                bar: None,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIZE: Size = Size {
        cols: 150,
        rows: 44,
    };

    #[test]
    fn gives_a_lone_master_everything_but_the_bottom_bar() {
        let areas = areas(SIZE, Chrome::Solo);

        assert_eq!(areas.title, Rect::new(1, 1, 148, 1));
        assert_eq!(areas.pane, Rect::new(1, 2, 148, 39));
        assert_eq!(
            areas.bar,
            Some(Rect::new(1, 42, 148, 1)),
            "a blank row above it"
        );
        assert_eq!(areas.side, None);
        assert_eq!(areas.rule, None);
        assert_eq!(areas.session(), Rect::new(1, 1, 148, 40));
        assert_eq!(
            areas.pane_size(),
            Size {
                cols: 148,
                rows: 39
            }
        );
    }

    #[test]
    fn puts_the_sidebar_or_the_rail_on_the_right_of_a_separator() {
        let sidebar = areas(SIZE, Chrome::Sidebar);
        let rail = areas(SIZE, Chrome::Rail);

        assert_eq!(sidebar.side, Some(Rect::new(109, 1, 40, 42)));
        assert_eq!(sidebar.title, Rect::new(1, 1, 107, 1));
        assert_eq!(sidebar.pane, Rect::new(1, 2, 107, 41));
        assert_eq!(sidebar.rule, Some(Rect::new(108, 1, 1, 42)));
        assert_eq!(rail.side, Some(Rect::new(143, 1, 6, 42)));
        assert_eq!(rail.pane.width, 141);
    }

    #[test]
    fn keeps_a_margin_around_everything_but_a_tiny_terminal() {
        assert_eq!(screen(SIZE), Rect::new(1, 1, 148, 42));
        assert_eq!(
            screen(Size { cols: 30, rows: 8 }),
            Rect::new(0, 0, 30, 8),
            "every cell is needed"
        );
    }

    #[test]
    fn never_overflows_a_tiny_terminal() {
        for chrome in [Chrome::Solo, Chrome::Sidebar, Chrome::Rail] {
            let areas = areas(Size { cols: 3, rows: 1 }, chrome);
            let pane = areas.pane;
            assert!(
                pane.x + pane.width <= 3 && pane.y + pane.height <= 1,
                "{chrome:?}"
            );
            assert!(areas.pane_size().cols >= 1 && areas.pane_size().rows >= 1);
        }
    }
}
