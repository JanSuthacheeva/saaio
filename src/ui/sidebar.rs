use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

use crate::output::short_duration;
use crate::proto::{PaneInfo, PaneKey, PaneStatus};
use crate::ui::layout::SIDEBAR_WIDTH;
use crate::ui::theme::Theme;
use crate::view::{Board, FlowView, State, StepStatus};

const WIDTH: usize = SIDEBAR_WIDTH as usize;
/// Rows the key hints take at the bottom, with the rule above them.
const FOOTER_ROWS: u16 = 3;

/// The right-hand overview: the master, then every worker grouped by what it needs.
pub struct Sidebar<'a> {
    pub board: &'a Board,
    pub panes: &'a [PaneInfo],
    pub focus: &'a PaneKey,
    pub now: u64,
    pub spin: usize,
    pub theme: &'a Theme,
}

/// One row of the sidebar and the pane a click on it focuses.
struct Row {
    line: Line<'static>,
    key: Option<PaneKey>,
    selected: bool,
}

/// The flows that get a pane, in sidebar order (needs you, running, done); digit n focuses the nth.
pub fn workers(board: &Board) -> Vec<&FlowView> {
    board
        .flows
        .iter()
        .filter(|flow| flow.workspace.is_some())
        .collect()
}

impl Sidebar<'_> {
    /// The pane a click on this row of the sidebar focuses.
    pub fn key_at(&self, row: u16, height: u16) -> Option<PaneKey> {
        self.rows(height).into_iter().nth(row as usize)?.key
    }

    fn rows(&self, height: u16) -> Vec<Row> {
        let theme = self.theme;
        let width = WIDTH;
        let mut rows = vec![
            plain(split(
                vec![
                    Span::raw(" "),
                    Span::styled(theme.glyphs.mark, theme.ui.accent),
                    Span::styled(" flow", theme.ui.strong),
                ],
                match self.board.needs_you {
                    0 => vec![],
                    count => vec![Span::styled(
                        format!("{count} need you "),
                        theme.state.waiting,
                    )],
                },
                width,
            )),
            plain(Line::styled("─".repeat(width), theme.ui.rule)),
        ];
        let master = *self.focus == PaneKey::Master;
        rows.push(Row {
            line: split(
                vec![
                    Span::styled(" 0 ", theme.ui.faint),
                    Span::styled(theme.glyphs.master, theme.ui.accent),
                    Span::styled(
                        " master",
                        if master {
                            theme.ui.strong
                        } else {
                            theme.ui.text
                        },
                    ),
                ],
                if master {
                    vec![Span::styled(
                        format!("{} in view ", theme.glyphs.in_view),
                        theme.ui.accent,
                    )]
                } else {
                    vec![Span::styled("you ", theme.ui.faint)]
                },
                width,
            ),
            key: Some(PaneKey::Master),
            selected: false,
        });
        rows.push(plain(Line::raw("")));

        let mut group = "";
        let mut flows = Vec::new();
        for (index, flow) in workers(self.board).into_iter().enumerate() {
            let heading = if flow.needs_you() {
                "NEEDS YOU"
            } else if flow.is_finished() {
                "DONE"
            } else {
                "RUNNING"
            };
            let mut block = Vec::new();
            if heading != group {
                group = heading;
                block.push(plain(Line::styled(format!(" {heading}"), theme.ui.faint)));
            }
            block.extend(self.flow_rows(flow, index));
            block.push(plain(Line::raw("")));
            flows.push(block);
        }

        let room = height.saturating_sub(FOOTER_ROWS) as usize;
        let mut shown = 0;
        for block in &flows {
            if rows.len() + block.len() > room {
                break;
            }
            shown += 1;
        }
        let hidden = flows.len() - shown;
        for block in flows.into_iter().take(shown) {
            rows.extend(block);
        }
        if hidden > 0 && rows.len() < room {
            rows.push(plain(Line::styled(
                format!(" {} {hidden} more", theme.glyphs.more),
                theme.ui.faint,
            )));
        }
        rows.truncate(room);
        rows.resize_with(room, || plain(Line::raw("")));
        rows.extend(self.footer(width));
        rows
    }

    fn flow_rows(&self, flow: &FlowView, index: usize) -> Vec<Row> {
        let theme = self.theme;
        let width = WIDTH;
        let key = PaneKey::Flow(flow.id.clone());
        let selected = *self.focus == key;
        let tone = theme.state(flow.state);
        let status = self
            .panes
            .iter()
            .find(|pane| pane.key == key)
            .map(|pane| &pane.status);
        let lead = |rest: &str| -> Vec<Span<'static>> {
            if selected {
                vec![
                    Span::styled(theme.glyphs.cursor, tone),
                    Span::styled(rest.to_string(), theme.ui.faint),
                ]
            } else {
                vec![Span::styled(format!(" {rest}"), theme.ui.faint)]
            }
        };

        let digit = if index < 9 {
            (index + 1).to_string()
        } else {
            " ".to_string()
        };
        let mut first = lead(&format!("{digit} "));
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
        second.extend(self.steps(flow));
        second.push(Span::raw(" "));
        second.push(self.label(flow));
        let detail = match flow.tasks {
            Some(tasks) => format!("{}/{} ", tasks.done, tasks.total),
            None if flow.mode == crate::event::Mode::Guided && !flow.is_finished() => {
                "guided ".to_string()
            }
            None => String::new(),
        };

        let mut rows = vec![
            Row {
                line: split(first, vec![right], width),
                key: Some(key.clone()),
                selected,
            },
            Row {
                line: split(second, vec![Span::styled(detail, theme.ui.faint)], width),
                key: Some(key.clone()),
                selected,
            },
        ];
        if let Some((text, style)) = self.note(flow, status) {
            let mut third = lead("    ");
            third.push(Span::styled(clip(&text, width - 6), style));
            rows.push(Row {
                line: Line::from(third),
                key: Some(key),
                selected,
            });
        }
        rows
    }

    pub fn glyph(&self, state: State) -> &'static str {
        let glyphs = &self.theme.glyphs;
        match state {
            State::Working | State::Delegated => glyphs.spinner[self.spin % glyphs.spinner.len()],
            State::Waiting => glyphs.waiting,
            State::Blocked | State::NotResponding => glyphs.blocked,
            State::Done => glyphs.done,
            State::Ended => glyphs.ended,
        }
    }

    /// ●─◉─○─○: what is done in muted, the current step in the state's color, the rest faint;
    /// all of it in the state's color once the flow is finished.
    pub fn steps(&self, flow: &FlowView) -> Vec<Span<'static>> {
        let theme = self.theme;
        let glyphs = &theme.glyphs;
        let tone = theme.state(flow.state);
        let mut spans = Vec::new();
        for (index, step) in flow.steps.iter().enumerate() {
            let (glyph, style) = match step.status {
                _ if flow.is_finished() => (glyphs.step_done, tone),
                StepStatus::Done => (glyphs.step_done, theme.ui.muted),
                StepStatus::Current => (glyphs.step_current, tone),
                StepStatus::Open => (glyphs.step_open, theme.ui.faint),
            };
            spans.push(Span::styled(glyph, style));
            if let Some(next) = flow.steps.get(index + 1) {
                let join = if flow.is_finished() {
                    tone
                } else if step.status == StepStatus::Done && next.status != StepStatus::Open {
                    theme.ui.muted
                } else {
                    theme.ui.faint
                };
                spans.push(Span::styled(glyphs.step_join, join));
            }
        }
        spans
    }

    pub fn label(&self, flow: &FlowView) -> Span<'static> {
        let theme = self.theme;
        match flow.state {
            State::Done | State::Ended if flow.merge_request.is_some() => Span::styled(
                merge_request(flow.merge_request.as_deref().unwrap_or_default()),
                theme.ui.faint,
            ),
            State::Done => Span::styled("done", theme.state.done),
            State::Ended => Span::styled("ended", theme.state.ended),
            State::NotResponding => Span::styled("not responding", theme.state.blocked),
            _ => match (flow.phase, flow.state) {
                (Some(phase), state) => Span::styled(phase.name(), theme.state(state)),
                (None, State::Working) => Span::styled("starting", theme.ui.faint),
                (None, State::Waiting) => Span::styled("waiting", theme.state.waiting),
                (None, State::Delegated) => Span::styled("delegated", theme.state.working),
                (None, _) => Span::styled("blocked", theme.state.blocked),
            },
        }
    }

    /// The third row: why the pane is not running, what the flow asks, or what it does.
    fn note(&self, flow: &FlowView, status: Option<&PaneStatus>) -> Option<(String, Style)> {
        let theme = self.theme;
        if let Some(PaneStatus::Failed { reason }) = status {
            return Some((reason.clone(), theme.state.blocked));
        }
        if flow.is_finished() {
            return None;
        }
        if let Some(blocker) = &flow.blocker {
            return Some((blocker.clone(), theme.ui.muted));
        }
        if let Some(delegate) = &flow.delegate {
            return Some((format!("agent: {delegate}"), theme.ui.faint));
        }
        // The first entry only says the flow started, which the label already does.
        flow.timeline
            .iter()
            .skip(1)
            .last()
            .map(|entry| (entry.text.clone(), theme.ui.faint))
    }

    fn footer(&self, width: usize) -> Vec<Row> {
        let theme = self.theme;
        let key = |text: &'static str| Span::styled(text, theme.ui.key);
        let hint = |text: &'static str| Span::styled(text, theme.ui.faint);
        vec![
            plain(Line::styled("─".repeat(width), theme.ui.rule)),
            plain(Line::from(vec![
                Span::raw(" "),
                key("C-space"),
                Span::raw(" "),
                key("0-9"),
                hint(" jump  "),
                key("r"),
                hint(" resume"),
            ])),
            plain(Line::from(vec![
                Span::raw("         "),
                key("d"),
                hint(" detach  "),
                key("Q"),
                hint(" stop"),
            ])),
        ]
    }
}

impl Widget for Sidebar<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        for (y, row) in self.rows(area.height).into_iter().enumerate() {
            let line = Rect::new(area.x, area.y + y as u16, area.width, 1);
            if row.selected {
                buf.set_style(line, self.theme.ui.selection);
            }
            buf.set_line(line.x, line.y, &row.line, line.width);
        }
    }
}

fn plain(line: Line<'static>) -> Row {
    Row {
        line,
        key: None,
        selected: false,
    }
}

/// `left` and `right` on one line of `width`, the left clipped when both do not fit.
fn split(left: Vec<Span<'static>>, right: Vec<Span<'static>>, width: usize) -> Line<'static> {
    let right_width: usize = right.iter().map(Span::width).sum();
    let room = width.saturating_sub(right_width);
    let mut spans = Vec::new();
    let mut used = 0;
    for span in left {
        let left_room = room.saturating_sub(used);
        if span.width() > left_room {
            let style = span.style;
            let cut = clip(&span.content, left_room.saturating_sub(1));
            used += Span::raw(cut.as_str()).width();
            spans.push(Span::styled(cut, style));
            break;
        }
        used += span.width();
        spans.push(span);
    }
    spans.push(Span::raw(" ".repeat(room.saturating_sub(used))));
    spans.extend(right);
    Line::from(spans)
}

/// The text cut to `width` columns, ending in … when it was longer.
fn clip(text: &str, width: usize) -> String {
    if Span::raw(text).width() <= width {
        return text.to_string();
    }
    let mut kept = String::new();
    let mut used = 0;
    for c in text.chars() {
        let char_width = Span::raw(c.to_string()).width();
        if used + char_width + 1 > width {
            break;
        }
        kept.push(c);
        used += char_width;
    }
    kept.push('…');
    kept
}

/// !418 for a GitLab merge request, #12 for a GitHub pull request, else MR.
fn merge_request(url: &str) -> String {
    let number = url.trim_end_matches('/').rsplit('/').next().unwrap_or("");
    let numeric = !number.is_empty() && number.chars().all(|c| c.is_ascii_digit());
    if numeric && url.contains("merge_requests/") {
        format!("MR !{number}")
    } else if numeric && url.contains("/pull/") {
        format!("PR #{number}")
    } else {
        "MR".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clips_long_text_with_an_ellipsis() {
        assert_eq!(clip("short", 10), "short");
        assert_eq!(clip("migration needs a default", 12), "migration n…");
        assert_eq!(Span::raw(clip("界界界界", 5)).width(), 5);
    }

    #[test]
    fn names_merge_requests_by_their_number() {
        assert_eq!(
            merge_request("https://gitlab.com/shop/-/merge_requests/418"),
            "MR !418"
        );
        assert_eq!(merge_request("https://github.com/a/b/pull/12"), "PR #12");
        assert_eq!(merge_request("somewhere"), "MR");
    }

    #[test]
    fn puts_the_right_part_flush_with_the_edge() {
        let line = split(
            vec![Span::raw(" a very long name that will not fit")],
            vec![Span::raw("29m ")],
            20,
        );

        let text: String = line
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect();
        assert_eq!(text, " a very long n… 29m ");
        assert_eq!(line.width(), 20);
    }
}
