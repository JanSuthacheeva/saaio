use ratatui::layout::Rect;

use crate::proto::Size;

/// Columns of the sidebar and of the rail, without the line that separates them from the pane.
pub const SIDEBAR_WIDTH: u16 = 40;
pub const RAIL_WIDTH: u16 = 6;

/// What surrounds the framed pane: a bottom bar while the master is alone, else a sidebar or a rail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Chrome {
    Solo,
    Sidebar,
    Rail,
}

/// Where each part of the screen goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Areas {
    /// The rounded border around the pane; its top edge names the session.
    pub frame: Rect,
    pub pane: Rect,
    pub side: Option<Rect>,
    pub bar: Option<Rect>,
}

impl Areas {
    pub fn pane_size(&self) -> Size {
        Size {
            cols: self.pane.width.max(1),
            rows: self.pane.height.max(1),
        }
    }
}

/// Solo: the framed pane, a blank row, the bottom bar; Sidebar: 40 columns on the right; Rail: 6.
pub fn areas(size: Size, chrome: Chrome) -> Areas {
    let (width, height) = (size.cols, size.rows);
    match chrome {
        Chrome::Solo => {
            let frame = Rect::new(0, 0, width, height.saturating_sub(2));
            Areas {
                frame,
                pane: inside(frame),
                side: None,
                bar: Some(Rect::new(0, height.saturating_sub(1), width, 1.min(height))),
            }
        }
        Chrome::Sidebar | Chrome::Rail => {
            let side_width = if chrome == Chrome::Sidebar {
                SIDEBAR_WIDTH
            } else {
                RAIL_WIDTH
            };
            let side_width = side_width.min(width.saturating_sub(2));
            let frame = Rect::new(0, 0, width.saturating_sub(side_width + 1), height);
            Areas {
                frame,
                pane: inside(frame),
                side: Some(Rect::new(width - side_width, 0, side_width, height)),
                bar: None,
            }
        }
    }
}

/// The cells within a one-cell border.
fn inside(frame: Rect) -> Rect {
    Rect::new(
        frame.x + 1.min(frame.width),
        frame.y + 1.min(frame.height),
        frame.width.saturating_sub(2),
        frame.height.saturating_sub(2),
    )
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

        assert_eq!(areas.frame, Rect::new(0, 0, 150, 42));
        assert_eq!(areas.pane, Rect::new(1, 1, 148, 40));
        assert_eq!(
            areas.bar,
            Some(Rect::new(0, 43, 150, 1)),
            "a blank row above it"
        );
        assert_eq!(areas.side, None);
        assert_eq!(
            areas.pane_size(),
            Size {
                cols: 148,
                rows: 40
            }
        );
    }

    #[test]
    fn puts_the_sidebar_or_the_rail_on_the_right_of_a_separator() {
        let sidebar = areas(SIZE, Chrome::Sidebar);
        let rail = areas(SIZE, Chrome::Rail);

        assert_eq!(sidebar.side, Some(Rect::new(110, 0, 40, 44)));
        assert_eq!(sidebar.frame, Rect::new(0, 0, 109, 44));
        assert_eq!(sidebar.pane, Rect::new(1, 1, 107, 42));
        assert_eq!(rail.side, Some(Rect::new(144, 0, 6, 44)));
        assert_eq!(rail.pane.width, 141);
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
