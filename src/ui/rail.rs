use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

use crate::proto::PaneKey;
use crate::ui::layout::RAIL_WIDTH;
use crate::ui::sidebar::{self, Sidebar};

/// The sidebar folded to a digit and a glyph per session, for more room in the pane.
#[derive(Debug)]
pub struct Rail<'a> {
    pub sidebar: Sidebar<'a>,
}

impl Rail<'_> {
    /// The pane a click on this row focuses.
    pub fn key_at(&self, row: u16, height: u16) -> Option<PaneKey> {
        self.rows(height).into_iter().nth(row as usize)?.1
    }

    fn rows(&self, height: u16) -> Vec<(Line<'static>, Option<PaneKey>, bool)> {
        let sidebar = &self.sidebar;
        let theme = sidebar.theme;
        let width = RAIL_WIDTH as usize;
        let mut rows = vec![
            (
                Line::from(vec![
                    Span::raw(" "),
                    Span::styled(theme.glyphs.mark, theme.ui.accent),
                ]),
                None,
                false,
            ),
            (Line::styled("─".repeat(width), theme.ui.rule), None, false),
            (
                Line::from(vec![Span::styled(" 0", theme.ui.accent)]),
                Some(PaneKey::Master),
                false,
            ),
        ];
        let mut group = None;
        for (index, flow) in sidebar::workers(sidebar.board).into_iter().enumerate() {
            let heading = flow.group();
            if group != Some(heading) {
                group = Some(heading);
                rows.push((Line::raw(""), None, false));
            }
            let key = PaneKey::Flow(flow.id.clone());
            let focused = *sidebar.focus == key;
            let tone = theme.state_style(flow.state);
            let digit = sidebar::digit(index);
            let lead = if focused {
                vec![
                    Span::styled(theme.glyphs.cursor, tone),
                    Span::styled(digit, theme.ui.strong),
                ]
            } else {
                vec![Span::raw(" "), Span::styled(digit, theme.ui.faint)]
            };
            let mut spans = lead;
            spans.push(Span::raw(" "));
            spans.push(Span::styled(sidebar.glyph(flow.state), tone));
            rows.push((Line::from(spans), Some(key), focused));
        }
        let room = height.saturating_sub(2) as usize;
        rows.truncate(room);
        rows.resize_with(room, || (Line::raw(""), None, false));
        rows.push((Line::styled("─".repeat(width), theme.ui.rule), None, false));
        rows.push((Line::styled(" C-␣", theme.ui.faint), None, false));
        rows
    }
}

impl Widget for Rail<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let selection = self.sidebar.theme.ui.selection;
        for ((line, _, focused), row) in self.rows(area.height).into_iter().zip(area.rows()) {
            if focused {
                buf.set_style(row, selection);
            }
            buf.set_line(row.x, row.y, &line, row.width);
        }
    }
}
