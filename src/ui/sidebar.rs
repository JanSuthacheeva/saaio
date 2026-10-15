use std::collections::HashMap;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

use crate::output::short_duration;
use crate::proto::{PaneInfo, PaneKey, PaneStatus};
use crate::ui::layout::SIDEBAR_WIDTH;
use crate::ui::peek::PeekLine;
use crate::ui::theme::Theme;
use crate::view::{Board, FlowView, State, StepStatus};

const WIDTH: usize = SIDEBAR_WIDTH as usize;
/// Rows the key hint takes at the bottom.
const FOOTER_ROWS: u16 = 1;
/// The most rows a peek takes, with its rule and its hint.
const PEEK_ROWS: usize = 14;
/// Fewer rows than this and the peek is left out.
const PEEK_MIN_ROWS: usize = 4;

/// The right-hand overview: the master, then every worker grouped by what it needs.
pub struct Sidebar<'a> {
    pub board: &'a Board,
    pub panes: &'a [PaneInfo],
    pub focus: &'a PaneKey,
    /// The flow whose peek shows (C-space j/k).
    pub cursor: Option<&'a str>,
    pub peek: &'a [PeekLine],
    /// Where the master runs: ~/projects/shop · main.
    pub master: Option<&'a str>,
    pub now: u64,
    pub spin: usize,
    /// Flows whose state changed a moment ago, by id; their rows stand out.
    pub changed: &'a HashMap<String, u64>,
    pub theme: &'a Theme,
}

/// One row of the sidebar and the pane a click on it focuses.
struct Row {
    line: Line<'static>,
    key: Option<PaneKey>,
    selected: bool,
}

/// The flows that get a pane, in sidebar order (needs you, ready, running, done); digit n focuses the nth.
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
        if let Some(place) = self.master {
            rows.push(Row {
                line: Line::styled(
                    format!("     {}", clip_place(place, width - 6)),
                    theme.ui.faint,
                ),
                key: Some(PaneKey::Master),
                selected: false,
            });
        }
        rows.push(plain(Line::raw("")));

        let listed = workers(self.board);
        let mut group = "";
        let mut flows = Vec::new();
        for (index, flow) in listed.iter().copied().enumerate() {
            let heading = group_of(flow);
            let mut block = Vec::new();
            if heading != group {
                group = heading;
                let count = listed
                    .iter()
                    .filter(|other| group_of(other) == heading)
                    .count();
                let tone = if heading == "NEEDS YOU" {
                    theme.state.waiting
                } else {
                    theme.ui.faint
                };
                block.push(plain(Line::from(vec![
                    Span::styled(format!(" {heading} "), theme.ui.faint),
                    Span::styled(count.to_string(), tone),
                ])));
            }
            block.extend(self.flow_rows(flow, index));
            block.push(plain(Line::raw("")));
            flows.push(block);
        }

        let room = height.saturating_sub(FOOTER_ROWS) as usize;
        let used = rows.len() + flows.iter().map(Vec::len).sum::<usize>();
        let budget = room.saturating_sub(used).min(PEEK_ROWS);
        let peeked = self.cursor.and_then(|id| {
            workers(self.board)
                .into_iter()
                .position(|flow| flow.id == id)
        });
        if let Some(index) = peeked
            && budget >= PEEK_MIN_ROWS
        {
            let flow = workers(self.board)[index];
            let block = &mut flows[index];
            let blank = block.pop();
            if self.note(flow, self.status(flow)).is_some() {
                block.pop();
            }
            block.extend(self.peek_rows(flow, budget));
            block.extend(blank);
        }
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
        rows.extend(self.footer());
        rows
    }

    fn flow_rows(&self, flow: &FlowView, index: usize) -> Vec<Row> {
        let theme = self.theme;
        let width = WIDTH;
        let key = PaneKey::Flow(flow.id.clone());
        let selected = *self.focus == key || self.cursor == Some(flow.id.as_str());
        let changed = self.changed.contains_key(&flow.id);
        let tone = theme.state(flow.state);
        let status = self.status(flow);
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
            // A finished flow says how long it took.
            _ if flow.is_finished() => format!(
                "in {} ",
                short_duration(flow.changed_at.saturating_sub(flow.started_at))
            ),
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
                selected: selected || changed,
            },
            Row {
                line: split(second, vec![Span::styled(detail, theme.ui.faint)], width),
                key: Some(key.clone()),
                selected: selected || changed,
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
                selected: selected || changed,
            });
        }
        rows
    }

    fn status(&self, flow: &FlowView) -> Option<&PaneStatus> {
        self.panes
            .iter()
            .find(|pane| pane.key == PaneKey::Flow(flow.id.clone()))
            .map(|pane| &pane.status)
    }

    /// What the flow under the cursor did lately, newest at the bottom, in at most `budget` rows:
    /// a dashed rule, the transcript lines, and how to jump in.
    fn peek_rows(&self, flow: &FlowView, budget: usize) -> Vec<Row> {
        let theme = self.theme;
        let glyphs = &theme.glyphs;
        let tone = theme.state(flow.state);
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
                    Span::styled(glyphs.spinner[self.spin % glyphs.spinner.len()], tone),
                    Span::styled(format!(" {}", clip(call, text_width)), theme.ui.muted),
                ]),
                PeekLine::Code {
                    branch,
                    worktree,
                    exists,
                } => {
                    body.push(vec![
                        Span::styled(" branch   ", theme.ui.faint),
                        Span::styled(clip(branch, text_width - 6), theme.ui.muted),
                    ]);
                    let place = if *exists {
                        clip(worktree, text_width - 6)
                    } else {
                        "gone".to_string()
                    };
                    body.push(vec![
                        Span::styled(" worktree ", theme.ui.faint),
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

        let lead = || Span::styled(glyphs.cursor, tone);
        let key = Some(PaneKey::Flow(flow.id.clone()));
        let row = |spans: Vec<Span<'static>>| Row {
            line: Line::from(spans),
            key: key.clone(),
            selected: true,
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
            Span::styled(" enter", theme.ui.key),
            Span::styled(" jump in", theme.ui.faint),
        ]));
        rows
    }

    pub fn glyph(&self, state: State) -> &'static str {
        let glyphs = &self.theme.glyphs;
        match state {
            State::Working | State::Delegated => glyphs.spinner[self.spin % glyphs.spinner.len()],
            State::Waiting => glyphs.waiting,
            State::Idle => glyphs.idle,
            State::Blocked | State::NotResponding => glyphs.blocked,
            State::Done => glyphs.done,
            State::Ended => glyphs.ended,
        }
    }

    /// ●───●───○───○: what is done and the current step in the state's color, the rest faint;
    /// all of it in the state's color once the flow is finished.
    pub fn steps(&self, flow: &FlowView) -> Vec<Span<'static>> {
        let theme = self.theme;
        let glyphs = &theme.glyphs;
        let tone = theme.state(flow.state);
        let mut spans = Vec::new();
        for (index, step) in flow.steps.iter().enumerate() {
            let (glyph, style) = match step.status {
                _ if flow.is_finished() => (glyphs.step_done, tone),
                StepStatus::Done => (glyphs.step_done, tone),
                StepStatus::Current => (glyphs.step_current, tone),
                StepStatus::Open => (glyphs.step_open, theme.ui.faint),
            };
            spans.push(Span::styled(glyph, style));
            if let Some(next) = flow.steps.get(index + 1) {
                let reached = step.status == StepStatus::Done && next.status != StepStatus::Open;
                let join = if flow.is_finished() || reached {
                    tone
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
                (None, State::Idle) => Span::styled("idle", theme.ui.muted),
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
            return Some((blocker.clone(), theme.ui.faint));
        }
        if let Some(delegate) = &flow.delegate {
            return Some((format!("agent: {delegate}"), theme.ui.faint));
        }
        last_note(flow).map(|note| (note, theme.ui.faint))
    }

    /// One quiet line at the foot: the prefix and the key that lists the rest.
    fn footer(&self) -> Vec<Row> {
        let theme = self.theme;
        vec![plain(Line::from(vec![
            Span::raw(" "),
            Span::styled("C-space", theme.ui.key),
            Span::raw(" "),
            Span::styled("?", theme.ui.key),
            Span::styled(" keys", theme.ui.faint),
        ]))]
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
pub fn split(left: Vec<Span<'static>>, right: Vec<Span<'static>>, width: usize) -> Line<'static> {
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

/// Words of `text` on rows of at most `width` columns; a word longer than a row is clipped.
pub fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut rows = Vec::new();
    for paragraph in text.lines().filter(|line| !line.trim().is_empty()) {
        let mut row = String::new();
        for word in paragraph.split_whitespace() {
            let next = if row.is_empty() {
                word.to_string()
            } else {
                format!("{row} {word}")
            };
            if Span::raw(next.as_str()).width() <= width {
                row = next;
                continue;
            }
            if !row.is_empty() {
                rows.push(row);
            }
            row = clip(word, width);
        }
        if !row.is_empty() {
            rows.push(row);
        }
    }
    rows
}

/// The text cut to `width` columns, ending in … when it was longer.
pub fn clip(text: &str, width: usize) -> String {
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

/// The sidebar section a flow is listed under.
fn group_of(flow: &FlowView) -> &'static str {
    if flow.needs_you() {
        "NEEDS YOU"
    } else if flow.state == State::Idle {
        "READY"
    } else if flow.is_finished() {
        "DONE"
    } else {
        "RUNNING"
    }
}

/// What a flow last said that its row does not already show: its last timeline entry, without
/// "started", phase changes and "answered" (the steps and the glyph say those), and without the
/// "idle: " the glyph says too.
pub fn last_note(flow: &FlowView) -> Option<String> {
    let text = flow
        .timeline
        .iter()
        .skip(1)
        .rev()
        .map(|entry| entry.text.as_str())
        .find(|text| !text.starts_with("phase: "))?;
    // An answered question is over; what came before it is no news.
    (text != "answered").then(|| text.strip_prefix("idle: ").unwrap_or(text).to_string())
}

/// A place (`path · branch`) in `width` columns: the path loses folders from its middle first
/// (/private/…/ui-look/repo · main), then its first folder; the branch goes only when the last
/// folder alone does not fit beside it.
pub fn clip_place(place: &str, width: usize) -> String {
    let fits = |text: &str| Span::raw(text).width() <= width;
    if fits(place) {
        return place.to_string();
    }
    let (path, suffix) = match place.rsplit_once(" · ") {
        Some((path, branch)) => (path, format!(" · {branch}")),
        None => (place, String::new()),
    };
    let parts: Vec<&str> = path.split('/').collect();
    let head_len = if parts.first() == Some(&"") { 2 } else { 1 };
    let head = parts[..head_len.min(parts.len())].join("/");
    let tail = |keep: usize| parts[parts.len() - keep..].join("/");
    let removable = parts.len().saturating_sub(head_len + 1);
    let with_head = (1..=removable)
        .rev()
        .map(|keep| format!("{head}/…/{}{suffix}", tail(keep)));
    let without_head = (1..parts.len())
        .rev()
        .map(|keep| format!("…/{}{suffix}", tail(keep)));
    let last = format!("…/{}", tail(1));
    with_head
        .chain(without_head)
        .chain(std::iter::once(last.clone()))
        .find(|candidate| fits(candidate))
        .unwrap_or_else(|| clip(&last, width))
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
    fn a_note_is_what_the_flow_last_said_not_what_the_row_shows() {
        use crate::event::{Event, EventKind, Mode, Phase};
        let flow = |kinds: Vec<EventKind>| {
            let mut events = vec![Event::new(
                0,
                EventKind::Started {
                    name: "shop".into(),
                    request: "x".into(),
                    mode: Mode::Auto,
                },
            )];
            events.extend(kinds.into_iter().map(|kind| Event::new(1, kind)));
            crate::fold::fold("id", &events, None, 2).unwrap()
        };
        let phase = |phase| EventKind::Phase { phase };

        let ready = flow(vec![
            phase(Phase::Design),
            EventKind::Idle {
                note: "The design is ready for review.".into(),
            },
        ]);
        let working = flow(vec![phase(Phase::Design), phase(Phase::Implement)]);
        let told = flow(vec![
            EventKind::Message {
                text: "Semicolons.".into(),
            },
            EventKind::Working,
            phase(Phase::Implement),
        ]);

        assert_eq!(
            last_note(&ready).as_deref(),
            Some("The design is ready for review.")
        );
        assert_eq!(last_note(&working), None);
        assert_eq!(last_note(&told).as_deref(), Some("master: Semicolons."));
        let answered = flow(vec![
            EventKind::Waiting {
                question: "Which?".into(),
            },
            EventKind::Working,
        ]);
        assert_eq!(last_note(&answered), None, "not the old question");
    }

    #[test]
    fn shortens_a_place_in_the_middle_and_keeps_its_branch() {
        let place = "/private/tmp/claude-501/x/scratchpad/ui-look/repo · main";

        assert_eq!(
            clip_place("~/projects/shop · main", 30),
            "~/projects/shop · main"
        );
        assert_eq!(clip_place(place, 34), "/private/…/ui-look/repo · main");
        assert_eq!(clip_place(place, 24), "/private/…/repo · main");
        assert_eq!(clip_place(place, 15), "…/repo · main");
        assert_eq!(clip_place(place, 9), "…/repo", "too narrow for the branch");
        assert_eq!(clip_place("~/a/b/c/d", 8), "~/…/c/d");
        for width in 1..60 {
            assert!(
                Span::raw(clip_place(place, width)).width() <= width,
                "{width}"
            );
        }
    }

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
