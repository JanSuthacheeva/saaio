//! The sidebar's rows: the master's, each flow's and the peek's

use ratatui::text::{Line, Span};

use super::{Mark, Row, Sidebar, WIDTH, digit, plain};
use crate::config::kind::DEFAULT_KIND;
use crate::proto::{PaneKey, PaneStatus};
use crate::text::short_duration;
use crate::ui::fit::{self, clip, clip_place, split};
use crate::ui::markdown;
use crate::ui::peek::PeekLine;
use crate::view::FlowView;

impl Sidebar<'_> {
    /// The header, its rule, the master and where it runs.
    pub(super) fn master_rows(&self) -> Vec<Row> {
        let theme = self.theme;
        let width = WIDTH;
        let mut rows = vec![
            plain(split(
                vec![
                    Span::raw(" "),
                    Span::styled(theme.glyphs.mark, theme.ui.accent),
                    Span::styled(" saaio", theme.ui.strong),
                ],
                vec![],
                width,
            )),
            plain(Line::styled("─".repeat(width), theme.ui.rule)),
        ];
        let master = *self.focus == PaneKey::Master;
        let (digit, name, mark) = if master {
            (theme.ui.strong, theme.ui.strong, Mark::Focus)
        } else if self.changed.contains_key(&PaneKey::Master) {
            (theme.ui.faint, theme.ui.text, Mark::Lit)
        } else {
            (theme.ui.faint, theme.ui.text, Mark::None)
        };
        let first = vec![
            Span::raw(" "),
            Span::styled("0 ", digit),
            self.master_glyph(),
            Span::styled(" master", name),
        ];
        rows.push(Row {
            line: split(first, vec![], width),
            key: Some(PaneKey::Master),
            mark,
        });
        if let Some(place) = self.master {
            rows.push(Row {
                line: Line::styled(
                    format!("     {}", clip_place(place, width - 6)),
                    theme.ui.faint,
                ),
                key: Some(PaneKey::Master),
                mark,
            });
        }
        rows.push(plain(Line::raw("")));
        rows
    }

    /// Row 0's glyph in its state's color, or a blank while the master needs nothing.
    pub fn master_glyph(&self) -> Span<'static> {
        match self.master_state {
            Some(state) => Span::styled(self.glyph(state), self.theme.state_style(state)),
            None => Span::raw(" "),
        }
    }

    pub(super) fn flow_rows(&self, flow: &FlowView, index: usize) -> Vec<Row> {
        let theme = self.theme;
        let width = WIDTH;
        let key = PaneKey::Flow(flow.id.clone());
        let focused = *self.focus == key;
        let selected = focused || self.cursor == Some(flow.id.as_str());
        let mark = self.mark(flow);
        let tone = theme.flow_style(flow);
        let status = self.status(flow);
        let lead = |rest: &str| vec![Span::styled(format!(" {rest}"), theme.ui.faint)];

        let mut first = lead(&format!("{} ", digit(index)));
        first.push(Span::styled(self.glyph(flow.state), tone));
        first.push(Span::styled(
            format!(" {}", flow.name),
            if selected {
                theme.ui.strong
            } else {
                theme.ui.text
            },
        ));
        let (age, age_style) = match status {
            Some(PaneStatus::Exited { .. }) => ("exited ".to_string(), theme.state.waiting),
            Some(PaneStatus::Failed { .. }) => ("failed ".to_string(), theme.state.blocked),
            _ => (
                format!(
                    "{} ",
                    short_duration(self.now.saturating_sub(flow.changed_at))
                ),
                theme.ui.faint,
            ),
        };
        // The agent, after the kind when it is not the default one: "fix claude".
        let agent = flow.workspace.as_ref().map(|space| {
            let room = width.saturating_sub(fit::spans_width(&first) + 2).max(1);
            let marker = match flow.kind.as_deref() {
                Some(kind) if kind != DEFAULT_KIND => format!("{kind} {}", space.agent),
                _ => space.agent.clone(),
            };
            fit::clip(&marker, room)
        });
        let marker = agent.as_ref().map(|agent| format!("{agent} "));
        let age_on_first = marker.as_ref().is_none_or(|marker| {
            fit::spans_width(&first) + fit::columns(marker) + fit::columns(&age) < width
        });
        let mut right = Vec::new();
        if let Some(marker) = marker {
            right.push(Span::styled(marker, theme.ui.faint));
        }
        if age_on_first {
            right.push(Span::styled(age.clone(), age_style));
        }

        let mut second = lead("    ");
        let steps = self.steps(flow);
        if !steps.is_empty() {
            second.extend(steps);
            second.push(Span::raw(" "));
        }
        second.push(self.label(flow));
        let detail = match flow.tasks.filter(|tasks| tasks.total > 0) {
            // A finished flow says how long it took.
            _ if flow.is_finished() => format!(
                "took {} ",
                short_duration(flow.changed_at.saturating_sub(flow.started_at))
            ),
            Some(tasks) => format!("{tasks} "),
            None => String::new(),
        };
        let mut second_right = vec![Span::styled(detail, theme.ui.faint)];
        if !age_on_first {
            second_right.push(Span::styled(age, age_style));
        }

        let mut rows = vec![
            Row {
                line: split(first, right, width),
                key: Some(key.clone()),
                mark,
            },
            Row {
                line: split(second, second_right, width),
                key: Some(key.clone()),
                mark,
            },
        ];
        if let Some((text, style)) = self.note(flow, status) {
            let mut third = lead("    ");
            // Notes stay behind the names; the row you are on reads a step brighter.
            let style = if selected && style == theme.ui.faint {
                theme.ui.muted
            } else {
                style
            };
            third.push(Span::styled(clip(&text, width - 6), style));
            rows.push(Row {
                line: Line::from(third),
                key: Some(key),
                mark,
            });
        }
        rows
    }

    /// The focused flow's rows get the focus, the peeked one's and one that just changed the light.
    fn mark(&self, flow: &FlowView) -> Mark {
        if *self.focus == PaneKey::Flow(flow.id.clone()) {
            Mark::Focus
        } else if self.cursor == Some(flow.id.as_str())
            || self.changed.contains_key(&PaneKey::Flow(flow.id.clone()))
        {
            Mark::Lit
        } else {
            Mark::None
        }
    }

    /// The peek in place of the flow's note, before its blank row.
    pub(super) fn insert_peek(&self, block: &mut Vec<Row>, flow: &FlowView, budget: usize) {
        let blank = block.pop();
        if self.note(flow, self.status(flow)).is_some() {
            block.pop();
        }
        block.extend(self.peek_rows(flow, budget));
        block.extend(blank);
    }

    /// What the flow under the cursor did lately, newest at the bottom, in at most `budget` rows:
    /// a dashed rule, the transcript lines, and how to jump in.
    #[expect(
        clippy::too_many_lines,
        reason = "the rule, the transcript and the jump hint, fitted to one budget"
    )]
    fn peek_rows(&self, flow: &FlowView, budget: usize) -> Vec<Row> {
        let theme = self.theme;
        let glyphs = &theme.glyphs;
        let tone = theme.flow_style(flow);
        let text_width = WIDTH - 4;
        let worktree = flow.workspace.as_ref().map(|space| space.path.as_str());
        let short = |text: &str| fit::shorten(text, worktree, self.home);
        let mut body: Vec<Vec<Span<'static>>> = Vec::new();
        for line in self.peek {
            match line {
                PeekLine::Tool { name, target } => body.push(vec![
                    Span::styled(format!(" {} ", glyphs.tool), theme.state.done),
                    Span::styled(name.clone(), theme.ui.strong),
                    Span::styled(
                        format!(
                            "({})",
                            clip(
                                &short(target),
                                text_width.saturating_sub(name.chars().count() + 3)
                            )
                        ),
                        theme.ui.text,
                    ),
                ]),
                PeekLine::Result(text) => body.push(vec![Span::styled(
                    format!(
                        "     {}  {}",
                        glyphs.result,
                        clip(&short(text), text_width - 6)
                    ),
                    theme.ui.faint,
                )]),
                PeekLine::Text(text) => {
                    for (index, row) in markdown::lines(&short(text), text_width, theme)
                        .into_iter()
                        .enumerate()
                    {
                        let mark = if index == 0 { glyphs.tool } else { " " };
                        let mut spans = vec![Span::styled(format!(" {mark} "), theme.ui.strong)];
                        spans.extend(row.spans);
                        body.push(spans);
                    }
                }
                PeekLine::Now(call) => body.push(vec![
                    Span::raw(" "),
                    Span::styled(self.spinner(), tone),
                    Span::styled(
                        format!(" {}", clip(&short(call), text_width)),
                        theme.ui.muted,
                    ),
                ]),
                PeekLine::Code {
                    branch,
                    worktree,
                    exists,
                    plain,
                } => {
                    if let Some(branch) = branch {
                        body.push(vec![
                            Span::styled(" branch   ", theme.ui.faint),
                            Span::styled(clip(branch, text_width - 6), theme.ui.muted),
                        ]);
                    }
                    let place = if *exists {
                        clip_place(&self.tilde(worktree), text_width - 6)
                    } else {
                        "gone".to_string()
                    };
                    // A plain session's code is in its folder.
                    let label = if *plain { " folder   " } else { " worktree " };
                    body.push(vec![
                        Span::styled(label, theme.ui.faint),
                        Span::styled(place, theme.ui.muted),
                    ]);
                }
            }
        }
        if body.is_empty() {
            body.push(vec![Span::styled(" no transcript yet", theme.ui.faint)]);
        }
        let keep = budget.saturating_sub(2);
        let body = body.split_off(body.len().saturating_sub(keep));

        let lead = || Span::raw(" ");
        let key = Some(PaneKey::Flow(flow.id.clone()));
        let mark = self.mark(flow);
        let row = |spans: Vec<Span<'static>>| Row {
            line: Line::from(spans),
            key: key.clone(),
            mark,
        };
        let mut rows = vec![row(vec![
            lead(),
            Span::styled(format!(" {}", glyphs.dash.repeat(WIDTH - 3)), theme.ui.rule),
        ])];
        for spans in body {
            let mut line = vec![lead()];
            line.extend(spans);
            rows.push(row(line));
        }
        rows.push(row(vec![
            lead(),
            Span::styled(format!(" {}", self.prefix.then("enter")), theme.ui.key),
            Span::styled(" jump in", theme.ui.faint),
        ]));
        rows
    }
}
