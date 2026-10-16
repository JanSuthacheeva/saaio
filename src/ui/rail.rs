use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

use crate::proto::PaneKey;
use crate::ui::layout::RAIL_WIDTH;
use crate::ui::sidebar::{self, Mark, Row, Sidebar, plain};

/// The sidebar folded to a digit and a glyph per session, for more room in the pane.
#[derive(Debug)]
pub struct Rail<'a> {
    pub sidebar: Sidebar<'a>,
}

impl Rail<'_> {
    /// The pane a click on this row focuses.
    pub fn key_at(&self, row: u16, height: u16) -> Option<PaneKey> {
        self.rows(height).into_iter().nth(usize::from(row))?.key
    }

    fn rows(&self, height: u16) -> Vec<Row> {
        let sidebar = &self.sidebar;
        let theme = sidebar.theme;
        let width = RAIL_WIDTH as usize;
        let master = *sidebar.focus == PaneKey::Master;
        let digit = |focused: bool| {
            if focused {
                theme.ui.strong
            } else {
                theme.ui.faint
            }
        };
        let mark = |focused: bool| if focused { Mark::Focus } else { Mark::None };
        let mut rows = vec![
            plain(Line::from(vec![
                Span::raw(" "),
                Span::styled(theme.glyphs.mark, theme.ui.accent),
            ])),
            plain(Line::styled("─".repeat(width), theme.ui.rule)),
            Row {
                line: Line::from(vec![Span::styled(" 0", digit(master))]),
                key: Some(PaneKey::Master),
                mark: mark(master),
            },
        ];
        let mut group = None;
        for (index, flow) in sidebar::workers(sidebar.board).into_iter().enumerate() {
            let heading = flow.group();
            if group != Some(heading) {
                group = Some(heading);
                rows.push(plain(Line::raw("")));
            }
            let key = PaneKey::Flow(flow.id.clone());
            let focused = *sidebar.focus == key;
            let tone = theme.flow_style(flow);
            let spans = vec![
                Span::raw(" "),
                Span::styled(sidebar::digit(index), digit(focused)),
                Span::raw(" "),
                Span::styled(sidebar.glyph(flow.state), tone),
            ];
            rows.push(Row {
                line: Line::from(spans),
                key: Some(key),
                mark: mark(focused),
            });
        }
        let room = usize::from(height.saturating_sub(2));
        rows.truncate(room);
        rows.resize_with(room, || plain(Line::raw("")));
        rows.push(plain(Line::styled("─".repeat(width), theme.ui.rule)));
        rows.push(plain(Line::styled(" C-␣", theme.ui.faint)));
        rows
    }
}

impl Widget for Rail<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        for (row, area) in self.rows(area.height).into_iter().zip(area.rows()) {
            row.render(area, buf, self.sidebar.theme);
        }
    }
}
