use ratatui::Frame;
use ratatui::layout::{Alignment, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};

use super::App;
use crate::proto::{PaneKey, PaneStatus};
use crate::ui::fit::{clip, clip_place, columns, spans_width};
use crate::ui::theme::Theme;
use crate::view::State;

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
    /// branch; a worker that needs you leads with a bar in its state's color.
    fn title_line(&self, theme: &Theme, width: u16, right: usize) -> Line<'static> {
        let room = |used: usize| (width as usize).saturating_sub(right + 2 + used);
        let PaneKey::Flow(id) = &self.focus else {
            let mut title = vec![Span::styled(" master", theme.ui.strong)];
            // The sidebar says where the master runs; alone, the title says it.
            let place = match (&self.master, self.areas().side) {
                (Some(place), Some(_)) => place
                    .rsplit_once(" · ")
                    .map(|(_, branch)| branch.to_string()),
                (Some(place), None) => Some(place.clone()),
                (None, _) => None,
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
        let lead = if flow.needs_you() {
            Span::styled(theme.glyphs.cursor, theme.state_style(flow.state))
        } else {
            Span::raw(" ")
        };
        let mut title = vec![
            lead,
            Span::styled(sidebar.glyph(flow.state), theme.state_style(flow.state)),
            Span::styled(format!(" {}  ", flow.name), theme.ui.strong),
        ];
        title.extend(sidebar.steps(flow));
        title.push(Span::raw(" "));
        title.push(sidebar.label(flow));
        if let Some(space) = &flow.workspace {
            let place = format!("{} · slot {}", space.branch, space.slot);
            let lead = "  · ";
            let fitted = clip(&place, room(spans_width(&title) + columns(lead)));
            if !fitted.is_empty() {
                title.push(Span::styled(format!("{lead}{fitted}"), theme.ui.faint));
            }
        }
        Line::from(title)
    }

    /// The right of the pane's top edge: with the rail, the next flow that needs you and the way back
    /// to the sidebar; with the sidebar, nothing.
    fn border_hint(&self, theme: &Theme) -> Option<Line<'static>> {
        // With the sidebar in view its key hint says the rest.
        if !self.rail {
            return None;
        }
        if self.workers().is_empty() {
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
                theme.state_style(flow.state),
            ));
            spans.push(Span::styled(
                format!("  {}  ", theme.glyphs.separator),
                theme.ui.rule,
            ));
        } else {
            spans.push(Span::raw(" "));
        }
        spans.push(Span::styled("C-space b", theme.ui.key));
        spans.push(Span::styled(" sidebar ", theme.ui.faint));
        Some(Line::from(spans))
    }

    pub(super) fn draw_bar(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let glyphs = &theme.glyphs;
        let mut left = vec![
            Span::raw(" "),
            Span::styled(glyphs.mark, theme.ui.accent),
            Span::styled(" flow ", theme.ui.strong),
            Span::styled(format!(" {} ", glyphs.separator), theme.ui.rule),
            Span::styled("master", theme.ui.text),
        ];
        if let Some(place) = &self.master {
            let room = usize::from(area.width).saturating_sub(spans_width(&left) + 3);
            left.push(Span::styled(
                format!("  {}", clip_place(place, room)),
                theme.ui.faint,
            ));
        }
        match self.status(&PaneKey::Master) {
            Some(PaneStatus::Exited { .. }) => left.push(Span::styled(
                "  exited - C-space r resumes it",
                theme.state.waiting,
            )),
            Some(PaneStatus::Failed { reason }) => {
                left.push(Span::styled(format!("  {reason}"), theme.state.blocked));
            }
            _ => {}
        }
        let keys = vec![
            Span::styled("C-space", theme.ui.key),
            Span::styled(" then  ", theme.ui.faint),
            Span::styled("n", theme.ui.key),
            Span::styled(" new flow   ", theme.ui.muted),
            Span::styled("?", theme.ui.key),
            Span::styled(" keys ", theme.ui.muted),
        ];
        let mut full = vec![
            Span::styled("no workers yet", theme.ui.faint),
            Span::styled(format!("   {}  ", glyphs.separator), theme.ui.rule),
        ];
        full.extend(keys.clone());
        let left = Line::from(left);
        let room = (area.width as usize).saturating_sub(left.width() + 1);
        let right = [Line::from(full), Line::from(keys)]
            .into_iter()
            .find(|line| line.width() <= room);
        frame.render_widget(Block::default().style(theme.ui.bar), area);
        frame.render_widget(Paragraph::new(left), area);
        if let Some(right) = right {
            frame.render_widget(Paragraph::new(right).alignment(Alignment::Right), area);
        }
    }
}
