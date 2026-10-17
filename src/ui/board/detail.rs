use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};

use super::{BoardView, every_flow};
use crate::text::{clock, long_duration, plural, short_duration};
use crate::ui::fit::{clip, clip_place, wrap};
use crate::ui::sidebar;
use crate::view::{FlowView, State, StepStatus, Tone};

impl BoardView<'_> {
    /// Header (name and facts), the steps under it, a rule, then the timeline beside the answer.
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
        // A plain session has no phases to show.
        let steps = u16::from(!flow.steps.is_empty());
        if steps > 0 {
            let mut line = vec![Span::raw(" ".repeat(title.chars().count() + 2))];
            line.extend(self.steps(flow));
            buf.set_line(area.x, area.y + 1, &Line::from(line), area.width);
        }
        buf.set_line(
            area.x,
            area.y + 1 + steps,
            &Line::styled("─".repeat(width), theme.ui.rule),
            area.width,
        );

        let body = Rect::new(
            area.x + 1,
            area.y + 2 + steps,
            area.width.saturating_sub(2),
            area.height.saturating_sub(2 + steps),
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
        self.render_answer(flow, right, buf);
    }

    /// Under the master: where it runs, then each flow that needs you with what it asks, and each
    /// idle one with what is ready.
    fn render_master_detail(&self, area: Rect, buf: &mut Buffer) {
        let theme = self.theme();
        let width = area.width as usize;
        let flows = sidebar::workers(self.board());
        let count = flows.len();
        let place = clip_place(self.sidebar.master.unwrap_or(""), width.saturating_sub(24));
        let mut facts = vec![match count {
            0 => "no workers yet".to_string(),
            _ => format!("{count} worker{}", plural(count)),
        }];
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
                Span::styled(sidebar.glyph(flow.state), theme.flow_style(flow)),
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
            lines.push(Line::from(vec![Span::raw("  "), theme.heading(label)]));
            lines.extend(entries);
        }
        if lines.is_empty() && count == 0 {
            lines.push(Line::from(vec![
                Span::styled("  n", theme.ui.key),
                Span::styled(" starts a flow", theme.ui.faint),
            ]));
        } else if lines.is_empty() {
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
            &Line::from(theme.heading("timeline")),
            area.width,
        );
        for (line, row) in lines.iter().zip(area.rows().skip(1)) {
            buf.set_line(row.x, row.y, line, row.width);
        }
    }

    /// DECISIONS, then LAST ANSWER: the flow's closing message wrapped, its end kept when it is long.
    fn render_answer(&self, flow: &FlowView, area: Rect, buf: &mut Buffer) {
        let theme = self.theme();
        let width = area.width as usize;
        let mut lines = Vec::new();
        if !flow.decisions.is_empty() {
            lines.push(Line::from(theme.heading("decisions")));
            for decision in &flow.decisions {
                lines.push(Line::from(vec![
                    Span::styled("· ", theme.ui.faint),
                    Span::styled(clip(decision, width.saturating_sub(2)), theme.ui.text),
                ]));
            }
        }
        if let Some(answer) = flow
            .answer
            .as_deref()
            .filter(|text| !text.trim().is_empty())
        {
            if !lines.is_empty() {
                lines.push(Line::raw(""));
            }
            lines.push(Line::from(theme.heading("last answer")));
            let room = usize::from(area.height).saturating_sub(lines.len());
            let mut rows = wrap(answer, width);
            if rows.len() > room && room > 0 {
                // The end of an answer is what it asks or concludes.
                rows = rows.split_off(rows.len() - (room - 1));
                rows.insert(0, theme.glyphs.more.to_string());
            }
            lines.extend(rows.into_iter().map(|row| Line::styled(row, theme.ui.text)));
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
                StepStatus::Current => (glyphs.step_current, theme.step_style(flow)),
                StepStatus::Open => (glyphs.step_open, theme.ui.faint),
            };
            if !steps.is_empty() {
                steps.push(Span::raw("   "));
            }
            let name = if step.phase.name() == "merge-request" {
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
    use crate::ui::board::BoardState;
    use crate::ui::board::fixtures::{flow, rows, with_state, with_view};

    #[test]
    fn the_timeline_shows_the_latest_entries_with_their_clock_times() {
        let flow = flow(
            "shop-221",
            Mode::Auto,
            vec![
                EventKind::Phase {
                    phase: crate::event::Phase::of("implement"),
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
                "TIMELINE",
                "00:01  ● phase: implement",
                "00:02  ● blocked: migration needs a default",
            ]
        );
        assert_eq!(
            rows(&narrow_buf),
            [
                "TIMELINE",
                "00:02  ● blocked: migration needs a",
                "          default",
            ],
            "a long entry wraps under its text, and the oldest rows give way"
        );
    }

    #[test]
    fn lists_decisions_then_the_end_of_the_last_answer() {
        let mut flow = flow(
            "shop-221",
            Mode::Auto,
            vec![EventKind::Decision {
                text: "keep the old API".into(),
            }],
        );
        flow.answer = Some(
            "First I read the code.\n\nThen I wrote the tests and they pass. Should I open the MR?"
                .into(),
        );
        let area = Rect::new(0, 0, 30, 7);
        let mut buf = Buffer::empty(area);

        with_view(vec![flow.clone()], |view| {
            view.render_answer(&flow, area, &mut buf);
        });

        assert_eq!(
            rows(&buf),
            [
                "DECISIONS",
                "· keep the old API",
                "",
                "LAST ANSWER",
                "…",
                "they pass. Should I open the",
                "MR?",
            ],
            "the end is kept"
        );
    }

    #[test]
    fn shows_no_heading_without_an_answer_or_decisions() {
        let flow = flow("shop-221", Mode::Auto, vec![]);
        let area = Rect::new(0, 0, 30, 4);
        let mut buf = Buffer::empty(area);

        with_view(vec![flow.clone()], |view| {
            view.render_answer(&flow, area, &mut buf);
        });

        assert!(rows(&buf).iter().all(String::is_empty));
    }

    #[test]
    fn puts_every_step_with_its_duration_under_the_header() {
        let flow = flow(
            "shop-221",
            Mode::Auto,
            vec![
                EventKind::Phase {
                    phase: crate::event::Phase::of("design"),
                },
                EventKind::Phase {
                    phase: crate::event::Phase::of("implement"),
                },
            ],
        );
        let area = Rect::new(0, 0, 80, 6);
        let mut buf = Buffer::empty(area);

        let state = BoardState {
            selected: 1,
            all: false,
        };
        with_state(vec![flow], state, |view| view.render_detail(area, &mut buf));

        let rows = rows(&buf);
        assert!(rows[0].starts_with(" 1 shop-221  "), "{rows:#?}");
        assert_eq!(
            rows[1].trim_start(),
            "design ✓ 1m   implement ● 28m   review ○   MR ○"
        );
        assert!(rows[2].starts_with("────"), "{rows:#?}");
        assert_eq!(rows[3], " TIMELINE");
    }

    #[test]
    fn the_current_step_takes_the_color_of_its_phase_by_default() {
        let flow = flow(
            "shop-221",
            Mode::Auto,
            vec![EventKind::Phase {
                phase: crate::event::Phase::of("implement"),
            }],
        );
        let area = Rect::new(0, 0, 80, 6);
        let mut buf = Buffer::empty(area);
        let state = BoardState {
            selected: 1,
            all: false,
        };

        let (implement, working) = with_state(vec![flow], state, |view| {
            view.render_detail(area, &mut buf);
            let theme = view.theme();
            (theme.phase.implement.fg, theme.state.working.fg)
        });

        let row = rows(&buf)[1].clone();
        let x = row[..row.find('●').unwrap()].chars().count() as u16;
        assert_ne!(implement, working);
        assert_eq!(Some(buf[(x, 1)].fg), implement);
    }
}
