use ratatui::text::{Line, Span};

use super::{Mark, Row, Sidebar, WIDTH, digit, plain};
use crate::proto::{PaneKey, PaneStatus};
use crate::text::short_duration;
use crate::ui::fit::{clip, clip_place, split, wrap};
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
                    Span::styled(" flow", theme.ui.strong),
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
        rows.push(Row {
            line: split(
                vec![
                    Span::raw(" "),
                    Span::styled("0 ", digit),
                    self.master_glyph(),
                    Span::styled(" master", name),
                ],
                vec![],
                width,
            ),
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
        let right = match status {
            Some(PaneStatus::Exited { .. }) => Span::styled("exited ", theme.state.waiting),
            Some(PaneStatus::Failed { .. }) => Span::styled("failed ", theme.state.blocked),
            _ => Span::styled(
                format!(
                    "{} ",
                    short_duration(self.now.saturating_sub(flow.changed_at))
                ),
                theme.ui.faint,
            ),
        };

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

        let mut rows = vec![
            Row {
                line: split(first, vec![right], width),
                key: Some(key.clone()),
                mark,
            },
            Row {
                line: split(second, vec![Span::styled(detail, theme.ui.faint)], width),
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
    fn peek_rows(&self, flow: &FlowView, budget: usize) -> Vec<Row> {
        let theme = self.theme;
        let glyphs = &theme.glyphs;
        let tone = theme.flow_style(flow);
        let text_width = WIDTH - 4;
        let mut body: Vec<Vec<Span<'static>>> = Vec::new();
        for line in self.peek {
            match line {
                PeekLine::Tool { name, target } => body.push(vec![
                    Span::styled(format!(" {} ", glyphs.tool), theme.state.done),
                    Span::styled(name.clone(), theme.ui.strong),
                    Span::styled(
                        format!(
                            "({})",
                            clip(target, text_width.saturating_sub(name.chars().count() + 3))
                        ),
                        theme.ui.text,
                    ),
                ]),
                PeekLine::Result(text) => body.push(vec![Span::styled(
                    format!("     {}  {}", glyphs.result, clip(text, text_width - 6)),
                    theme.ui.faint,
                )]),
                PeekLine::Text(text) => {
                    for (index, row) in wrap(text, text_width).into_iter().enumerate() {
                        let mark = if index == 0 { glyphs.tool } else { " " };
                        body.push(vec![
                            Span::styled(format!(" {mark} "), theme.ui.strong),
                            Span::styled(row, theme.ui.text),
                        ]);
                    }
                }
                PeekLine::Now(call) => body.push(vec![
                    Span::raw(" "),
                    Span::styled(self.spinner(), tone),
                    Span::styled(format!(" {}", clip(call, text_width)), theme.ui.muted),
                ]),
                PeekLine::Code {
                    branch,
                    worktree,
                    exists,
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
                    let label = if branch.is_some() {
                        " worktree "
                    } else {
                        " folder   "
                    };
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
            Span::styled(" C-space enter", theme.ui.key),
            Span::styled(" jump in", theme.ui.faint),
        ]));
        rows
    }
}
