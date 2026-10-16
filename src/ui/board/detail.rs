use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};

use super::{BoardView, every_flow};
use crate::event::Phase;
use crate::text::{clock, long_duration, plural, short_duration};
use crate::ui::fit::{clip, clip_place, wrap};
use crate::ui::sidebar;
use crate::view::{FlowView, State, StepStatus, Tone};

impl BoardView<'_> {
    /// The selected flow's request, mode, branch and age, then its timeline beside its decisions and steps.
    pub(super) fn render_detail(&self, area: Rect, buf: &mut Buffer) {
        let theme = self.theme();
        let flows = sidebar::workers(self.board());
        let width = area.width as usize;
        let selected = if self.state.all {
            self.projects.and_then(|projects| {
                every_flow(projects)
                    .get(self.state.selected)
                    .map(|(project, flow)| (format!(" {}", flow.name), Some(&project.name), *flow))
            })
        } else {
            self.state
                .selected
                .checked_sub(1)
                .and_then(|at| flows.get(at).copied())
                .map(|flow| {
                    (
                        format!(" {} {}", self.state.selected, flow.name),
                        None,
                        flow,
                    )
                })
        };
        let Some((title, project, flow)) = selected else {
            if self.state.all {
                return;
            }
            self.render_master_detail(area, buf);
            return;
        };
        let mut facts: Vec<String> = project.into_iter().cloned().collect();
        if !flow.request.is_empty() {
            facts.push(flow.request.clone());
        }
        facts.push(flow.mode.name().to_string());
        if let Some(space) = flow.workspace.as_ref().filter(|_| flow.is_plain()) {
            facts.push(self.sidebar.tilde(&space.path));
        } else if let Some(space) = &flow.workspace {
            facts.extend(space.branch.clone());
            facts.extend(space.slot.map(|slot| format!("slot {slot}")));
        }
        facts.push(format!(
            "started {} ago",
            long_duration(self.sidebar.now.saturating_sub(flow.started_at))
        ));
        let header = Line::from(vec![
            Span::styled(title.clone(), theme.ui.strong),
            Span::styled(
                format!(
                    "  {}",
                    clip(
                        &facts.join(" · "),
                        width.saturating_sub(title.chars().count() + 3)
                    )
                ),
                theme.ui.faint,
            ),
        ]);
        buf.set_line(area.x, area.y, &header, area.width);
        buf.set_line(
            area.x,
            area.y + 1,
            &Line::styled("─".repeat(width), theme.ui.rule),
            area.width,
        );

        let body = Rect::new(
            area.x + 1,
            area.y + 2,
            area.width.saturating_sub(2),
            area.height.saturating_sub(2),
        );
        let left_width = body.width.saturating_sub(2) * 3 / 5;
        let left = Rect::new(body.x, body.y, left_width, body.height);
        let right = Rect::new(
            body.x + left_width + 2,
            body.y,
            body.width.saturating_sub(left_width + 2),
            body.height,
        );
        self.render_timeline(flow, left, buf);
        self.render_facts(flow, right, buf);
    }

    /// Under the master: where it runs, then each flow that needs you with what it asks, and each
    /// idle one with what is ready.
    fn render_master_detail(&self, area: Rect, buf: &mut Buffer) {
        let theme = self.theme();
        let width = area.width as usize;
        let flows = sidebar::workers(self.board());
        let count = flows.len();
        let place = clip_place(self.sidebar.master.unwrap_or(""), width.saturating_sub(24));
        let mut facts = vec![format!("{count} worker{}", plural(count))];
        if !place.is_empty() {
            facts.insert(0, place);
        }
        let header = Line::from(vec![
            Span::styled(" 0 master", theme.ui.strong),
            Span::styled(format!("  {}", facts.join(" · ")), theme.ui.faint),
        ]);
        buf.set_line(area.x, area.y, &header, area.width);
        buf.set_line(
            area.x,
            area.y + 1,
            &Line::styled("─".repeat(width), theme.ui.rule),
            area.width,
        );

        let sidebar = &self.sidebar;
        // Names padded to the longest, so what each flow asks starts in one column.
        let name_width = flows
            .iter()
            .filter(|flow| flow.needs_you() || flow.state == State::Idle)
            .map(|flow| flow.name.chars().count())
            .max()
            .unwrap_or(0);
        let entry = |flow: &FlowView, text: String, style: Style| {
            let lead = format!(
                "  {} {:name_width$}  ",
                sidebar.glyph(flow.state),
                flow.name
            );
            Line::from(vec![
                Span::styled("  ", theme.ui.text),
                Span::styled(sidebar.glyph(flow.state), theme.state_style(flow.state)),
                Span::styled(format!(" {:name_width$}  ", flow.name), theme.ui.strong),
                Span::styled(
                    clip(&text, width.saturating_sub(lead.chars().count() + 1)),
                    style,
                ),
            ])
        };
        let needing: Vec<Line> = flows
            .iter()
            .filter(|flow| flow.needs_you())
            .map(|flow| {
                let style = match flow.state {
                    State::Waiting => theme.ui.muted,
                    _ => theme.state.blocked,
                };
                entry(flow, flow.blocker.clone().unwrap_or_default(), style)
            })
            .collect();
        let ready: Vec<Line> = flows
            .iter()
            .filter(|flow| flow.state == State::Idle)
            .map(|flow| {
                entry(
                    flow,
                    sidebar::last_note(flow).unwrap_or_default(),
                    theme.ui.muted,
                )
            })
            .collect();
        let mut lines = Vec::new();
        for (label, entries) in [("needs you", needing), ("ready for you", ready)] {
            if entries.is_empty() {
                continue;
            }
            if !lines.is_empty() {
                lines.push(Line::raw(""));
            }
            lines.push(Line::styled(format!("  {label}"), theme.ui.faint));
            lines.extend(entries);
        }
        if lines.is_empty() {
            lines.push(Line::styled("  nothing needs you", theme.ui.faint));
        }
        for (line, row) in lines.iter().zip(area.rows().skip(2)) {
            buf.set_line(row.x, row.y, line, row.width);
        }
    }

    fn render_timeline(&self, flow: &FlowView, area: Rect, buf: &mut Buffer) {
        let theme = self.theme();
        let mut lines = Vec::new();
        let text_width = (area.width as usize).saturating_sub(10);
        for entry in &flow.timeline {
            let tone = theme.tone_style(entry.tone);
            let text_style = if entry.tone == Tone::Problem {
                tone
            } else {
                theme.ui.text
            };
            for (index, row) in wrap(&entry.text, text_width).into_iter().enumerate() {
                let lead = if index == 0 {
                    vec![
                        Span::styled(clock(entry.at, self.offset_secs), theme.ui.faint),
                        Span::raw("  "),
                        Span::styled("●", tone),
                        Span::raw(" "),
                    ]
                } else {
                    vec![Span::raw(" ".repeat(10))]
                };
                let mut spans = lead;
                spans.push(Span::styled(row, text_style));
                lines.push(Line::from(spans));
            }
        }
        let room = area.height.saturating_sub(1) as usize;
        let lines = lines.split_off(lines.len().saturating_sub(room));
        buf.set_line(
            area.x,
            area.y,
            &Line::styled("timeline", theme.ui.faint),
            area.width,
        );
        for (line, row) in lines.iter().zip(area.rows().skip(1)) {
            buf.set_line(row.x, row.y, line, row.width);
        }
    }

    fn render_facts(&self, flow: &FlowView, area: Rect, buf: &mut Buffer) {
        let theme = self.theme();
        let width = area.width as usize;
        let mut lines = Vec::new();
        if !flow.decisions.is_empty() {
            lines.push(Line::styled("decisions", theme.ui.faint));
            for decision in &flow.decisions {
                lines.push(Line::from(vec![
                    Span::styled("· ", theme.ui.faint),
                    Span::styled(clip(decision, width.saturating_sub(2)), theme.ui.text),
                ]));
            }
            lines.push(Line::raw(""));
        }
        // A plain session has no phases to show.
        if !flow.steps.is_empty() {
            lines.push(Line::styled("steps", theme.ui.faint));
            lines.push(Line::from(self.steps(flow)));
        }
        for (line, row) in lines.iter().zip(area.rows()) {
            buf.set_line(row.x, row.y, line, row.width);
        }
    }

    /// Each phase with its glyph and the time spent in it.
    fn steps(&self, flow: &FlowView) -> Vec<Span<'static>> {
        let theme = self.theme();
        let glyphs = &theme.glyphs;
        let mut steps = Vec::new();
        for step in &flow.steps {
            let (glyph, style) = match step.status {
                StepStatus::Done => (glyphs.done, theme.state.done),
                StepStatus::Current => (glyphs.step_current, theme.state_style(flow.state)),
                StepStatus::Open => (glyphs.step_open, theme.ui.faint),
            };
            if !steps.is_empty() {
                steps.push(Span::raw("   "));
            }
            let name = if step.phase == Phase::MergeRequest {
                "MR"
            } else {
                step.phase.name()
            };
            steps.push(Span::styled(format!("{name} "), theme.ui.text));
            steps.push(Span::styled(glyph, style));
            if let Some(duration) = step.duration_ms {
                steps.push(Span::styled(
                    format!(" {}", short_duration(duration)),
                    theme.ui.faint,
                ));
            }
        }
        steps
    }
}

#[cfg(test)]
mod tests {
    use ratatui::layout::Rect;

    use super::*;
    use crate::event::{EventKind, Mode};
    use crate::ui::board::fixtures::{flow, rows, with_view};

    #[test]
    fn the_timeline_shows_the_latest_entries_with_their_clock_times() {
        let flow = flow(
            "shop-221",
            Mode::Auto,
            vec![
                EventKind::Phase {
                    phase: Phase::Implement,
                },
                EventKind::Blocked {
                    reason: "migration needs a default".into(),
                },
            ],
        );
        let wide = Rect::new(0, 0, 60, 3);
        let narrow = Rect::new(0, 0, 36, 3);
        let mut wide_buf = Buffer::empty(wide);
        let mut narrow_buf = Buffer::empty(narrow);

        with_view(vec![flow.clone()], |view| {
            view.render_timeline(&flow, wide, &mut wide_buf);
            view.render_timeline(&flow, narrow, &mut narrow_buf);
        });

        assert_eq!(
            rows(&wide_buf),
            [
                "timeline",
                "00:01  ● phase: implement",
                "00:02  ● blocked: migration needs a default",
            ]
        );
        assert_eq!(
            rows(&narrow_buf),
            [
                "timeline",
                "00:02  ● blocked: migration needs a",
                "          default",
            ],
            "a long entry wraps under its text, and the oldest rows give way"
        );
    }

    #[test]
    fn the_facts_list_decisions_then_every_step_with_its_duration() {
        let flow = flow(
            "shop-221",
            Mode::Auto,
            vec![
                EventKind::Phase {
                    phase: Phase::Design,
                },
                EventKind::Decision {
                    text: "keep the old API".into(),
                },
                EventKind::Phase {
                    phase: Phase::Implement,
                },
            ],
        );
        let area = Rect::new(0, 0, 60, 5);
        let mut buf = Buffer::empty(area);

        with_view(vec![flow.clone()], |view| {
            view.render_facts(&flow, area, &mut buf);
        });

        let rows = rows(&buf);
        assert_eq!(rows[0], "decisions");
        assert_eq!(rows[1], "· keep the old API");
        assert_eq!(rows[2], "");
        assert_eq!(rows[3], "steps");
        assert_eq!(rows[4], "design ✓ 2m   implement ● 27m   review ○   MR ○");
    }
}
