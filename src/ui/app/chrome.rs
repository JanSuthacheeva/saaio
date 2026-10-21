//! The title row above the pane: the session in view, a notice or the rail's hint

use ratatui::Frame;
use ratatui::layout::{Alignment, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};

use super::App;
use crate::proto::PaneKey;
use crate::ui::fit::{clip, clip_place, columns, spans_width};
use crate::ui::theme::Theme;
use crate::view::State;

/// The fewest columns a cut branch keeps in the title before it goes.
const BRANCH_KEPT: usize = 12;

impl App {
    /// The row above the pane that names the session, on the panel color: what is in view on the
    /// left, a notice or the rail's hint on the right. The left is fitted so the right stays.
    pub(super) fn draw_title(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let right = match &self.notice {
            Some(notice) => Some(Line::styled(format!(" {notice} "), theme.state.waiting)),
            None => self.border_hint(theme),
        };
        let left = self.title_line(theme, area.width, right.as_ref().map_or(0, Line::width));
        frame.render_widget(Block::default().style(theme.ui.bar), area);
        frame.render_widget(Paragraph::new(left), area);
        if let Some(right) = right {
            frame.render_widget(Paragraph::new(right).alignment(Alignment::Right), area);
        }
    }

    /// The session in view: the master and its branch, or a worker's glyph, name, steps, phase and
    /// branch.
    fn title_line(&self, theme: &Theme, width: u16, right: usize) -> Line<'static> {
        let room = |used: usize| (width as usize).saturating_sub(right + 2 + used);
        let PaneKey::Flow(id) = &self.focus else {
            let mut title = vec![Span::styled(" master", theme.ui.strong)];
            // The sidebar says where the master runs and the title its branch; the rail says neither.
            let place = match &self.master.place {
                Some(place) if self.folded() => Some(place.clone()),
                Some(place) => place
                    .rsplit_once(" · ")
                    .map(|(_, branch)| branch.to_string()),
                None => None,
            };
            if let Some(place) = place {
                let fitted = clip_place(&place, room(spans_width(&title) + 2));
                if !fitted.is_empty() {
                    title.push(Span::styled(format!("  {fitted}"), theme.ui.faint));
                }
            }
            return Line::from(title);
        };
        let Some(flow) = self.worker(id) else {
            return Line::styled(format!(" {id}"), theme.ui.strong);
        };
        let sidebar = self.sidebar(theme, &[]);
        let mut title = vec![
            Span::raw(" "),
            Span::styled(sidebar.glyph(flow.state), theme.flow_style(flow)),
            Span::styled(format!(" {}  ", flow.name), theme.ui.strong),
        ];
        let steps = sidebar.steps(flow);
        if !steps.is_empty() {
            title.extend(steps);
            title.push(Span::raw(" "));
        }
        title.push(sidebar.label(flow));
        if let Some(space) = flow.workspace.as_ref().filter(|_| flow.is_plain()) {
            let lead = "  · ";
            let fitted = clip_place(
                &self.tilde(&space.path),
                room(spans_width(&title) + columns(lead)),
            );
            if !fitted.is_empty() {
                title.push(Span::styled(format!("{lead}{fitted}"), theme.ui.faint));
            }
        } else if let Some(space) = &flow.workspace
            && let (Some(branch), Some(slot)) = (&space.branch, space.slot)
        {
            let place = format!("{branch} · slot {slot}");
            let lead = "  · ";
            let fitted = clip(&place, room(spans_width(&title) + columns(lead)));
            // A branch cut to a few letters says nothing; it goes whole.
            if fitted == place || columns(&fitted) >= BRANCH_KEPT {
                title.push(Span::styled(format!("{lead}{fitted}"), theme.ui.faint));
            }
        }
        Line::from(title)
    }

    /// The right of the pane's top edge: with the rail, the next flow that needs you and the way back
    /// to the sidebar; with the sidebar, nothing.
    fn border_hint(&self, theme: &Theme) -> Option<Line<'static>> {
        // With the sidebar in view its key hint says the rest.
        if !self.folded() {
            return None;
        }
        let sidebar = self.sidebar(theme, &[]);
        let mut spans = Vec::new();
        let next = self
            .workers()
            .into_iter()
            .enumerate()
            .find(|(_, flow)| flow.needs_you() && !self.in_view(&flow.id));
        if let Some((index, flow)) = next {
            let what = match flow.state {
                State::Waiting => "is waiting",
                State::NotResponding => "is not responding",
                _ => "is blocked",
            };
            spans.push(Span::styled(
                format!(
                    " {} {} {} {what}",
                    sidebar.glyph(flow.state),
                    index + 1,
                    flow.name
                ),
                theme.flow_style(flow),
            ));
            spans.push(Span::styled(
                format!("  {}  ", theme.glyphs.separator),
                theme.ui.rule,
            ));
        } else {
            spans.push(Span::raw(" "));
        }
        spans.push(Span::styled(self.prefix.then("b"), theme.ui.key));
        spans.push(Span::styled(" sidebar ", theme.ui.faint));
        Some(Line::from(spans))
    }
}
