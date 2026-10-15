use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Widget};

use crate::event::Mode;
use crate::event::Phase;
use crate::output::short_duration;
use crate::proto::{PaneKey, PaneStatus};
use crate::ui::sidebar::{self, Sidebar, clip, split, wrap};
use crate::ui::theme::Theme;
use crate::view::{Board, FlowView, State, StepStatus, Tone};

/// Cards per row.
const COLUMNS: usize = 3;
/// The longest task bar on a card.
const BAR_WIDTH: usize = 27;
/// A card's rows: its four lines inside a border.
const CARD_ROWS: u16 = 6;

/// Which card the board's cursor is on: 0 is the master, n the nth worker.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BoardState {
    pub selected: usize,
}

impl BoardState {
    /// h j k l and the arrows move over the grid; a digit picks a card.
    pub fn moved(self, step: isize, cards: usize) -> BoardState {
        let next = self.selected as isize + step;
        if next < 0 || next as usize >= cards {
            return self;
        }
        BoardState {
            selected: next as usize,
        }
    }

    pub fn step(key: char) -> Option<isize> {
        match key {
            'h' => Some(-1),
            'l' => Some(1),
            'j' => Some(COLUMNS as isize),
            'k' => Some(-(COLUMNS as isize)),
            _ => None,
        }
    }
}

/// Every session as a card, and the timeline, decisions and steps of the selected one.
pub struct BoardView<'a> {
    pub sidebar: Sidebar<'a>,
    pub state: BoardState,
    /// The pane esc goes back to.
    pub back: &'a str,
    /// The local time zone's offset from UTC, for the timeline's clock times.
    pub offset_secs: i64,
    /// Shown in place of the keys, such as why clearing a flow failed.
    pub notice: Option<&'a str>,
}

impl BoardView<'_> {
    fn theme(&self) -> &Theme {
        self.sidebar.theme
    }

    fn board(&self) -> &Board {
        self.sidebar.board
    }

    fn header(&self, width: usize) -> Line<'static> {
        let theme = self.theme();
        let flows = sidebar::workers(self.board());
        let count = |states: &[State]| {
            flows
                .iter()
                .filter(|flow| states.contains(&flow.state))
                .count()
        };
        let mut right = vec![Span::styled(
            format!("{} sessions", flows.len() + 1),
            theme.ui.faint,
        )];
        let parts = [
            (
                count(&[State::Waiting, State::Blocked, State::NotResponding]),
                "need you",
                theme.state.waiting,
            ),
            (count(&[State::Working]), "running", theme.ui.faint),
            (count(&[State::Delegated]), "delegated", theme.ui.faint),
            (count(&[State::Done]), "done", theme.ui.faint),
            (count(&[State::Ended]), "ended", theme.ui.faint),
        ];
        for (number, label, style) in parts {
            if number > 0 {
                right.push(Span::styled(" · ", theme.ui.faint));
                right.push(Span::styled(format!("{number} {label}"), style));
            }
        }
        right.push(Span::raw(" "));
        split(
            vec![
                Span::raw(" "),
                Span::styled(theme.glyphs.mark, theme.ui.accent),
                Span::styled(" flow", theme.ui.strong),
                Span::styled(" board", theme.ui.faint),
            ],
            right,
            width,
        )
    }

    fn master_card(&self) -> [Line<'static>; 4] {
        let theme = self.theme();
        let workers = sidebar::workers(self.board()).len();
        let place = self.sidebar.master.unwrap_or("").to_string();
        let status = match self.status(&PaneKey::Master) {
            Some(PaneStatus::Exited { .. }) => "exited - C-space r resumes it",
            Some(PaneStatus::Failed { .. }) => "could not start",
            _ => "",
        };
        [
            Line::from(vec![
                Span::styled(" 0 ", theme.ui.faint),
                Span::styled(theme.glyphs.master, theme.ui.accent),
                Span::styled(" master", theme.ui.strong),
            ]),
            Line::styled(format!("   {place}"), theme.ui.faint),
            Line::styled(
                format!("   {workers} worker{}", if workers == 1 { "" } else { "s" }),
                theme.ui.faint,
            ),
            Line::styled(format!("   {status}"), theme.state.waiting),
        ]
    }

    fn flow_card(&self, flow: &FlowView, index: usize, width: usize) -> [Line<'static>; 4] {
        let theme = self.theme();
        let tone = theme.state(flow.state);
        let digit = if index < 9 {
            (index + 1).to_string()
        } else {
            " ".to_string()
        };

        let first = Line::from(vec![
            Span::styled(format!(" {digit} "), theme.ui.faint),
            Span::styled(self.sidebar.glyph(flow.state), tone),
            Span::styled(format!(" {}", flow.name), theme.ui.strong),
        ]);

        let mut second = vec![Span::raw("   ")];
        second.extend(self.sidebar.steps(flow));
        second.push(Span::raw("  "));
        second.push(self.sidebar.label(flow));
        if flow.mode == Mode::Guided && !flow.is_finished() {
            second.push(Span::styled("  · guided", theme.ui.faint));
        }

        let ago = format!(
            "{} ago ",
            short_duration(self.sidebar.now.saturating_sub(flow.changed_at))
        );
        let third = if flow.is_finished() {
            let text = match &flow.workspace {
                Some(space) => space.branch.clone(),
                None => String::new(),
            };
            split(
                vec![Span::styled(format!("   {text}"), theme.ui.faint)],
                vec![Span::styled(ago, theme.ui.faint)],
                width,
            )
        } else {
            let tasks = flow
                .tasks
                .map(|tasks| (tasks.done, tasks.total))
                .filter(|(_, total)| *total > 0);
            let count = tasks.map_or("-".to_string(), |(done, total)| format!("{done}/{total}"));
            let bar = width
                .saturating_sub(3 + 2 + count.chars().count() + ago.chars().count() + 1)
                .min(BAR_WIDTH);
            let filled = tasks.map_or(0, |(done, total)| {
                (bar as u32 * done.min(total) / total) as usize
            });
            split(
                vec![
                    Span::raw("   "),
                    Span::styled("━".repeat(filled), theme.ui.muted),
                    Span::styled("─".repeat(bar - filled), theme.ui.faint),
                    Span::styled(
                        format!("  {count}"),
                        if tasks.is_some() {
                            theme.ui.text
                        } else {
                            theme.ui.faint
                        },
                    ),
                ],
                vec![Span::styled(ago, theme.ui.faint)],
                width,
            )
        };

        let (note, style) = match flow.state {
            State::Blocked | State::NotResponding => (flow.blocker.clone(), theme.state.blocked),
            State::Waiting => (flow.blocker.clone(), theme.ui.muted),
            State::Delegated => (
                flow.delegate.as_ref().map(|text| format!("agent: {text}")),
                theme.ui.faint,
            ),
            State::Working => (
                flow.timeline
                    .iter()
                    .skip(1)
                    .last()
                    .map(|entry| entry.text.clone()),
                theme.ui.faint,
            ),
            State::Done | State::Ended => (Some(flow.request.clone()), theme.ui.faint),
        };
        let fourth = Line::styled(
            format!(
                "   {}",
                clip(note.as_deref().unwrap_or(""), width.saturating_sub(4))
            ),
            style,
        );
        [first, Line::from(second), third, fourth]
    }

    /// The label at a card's top right: a solid pill when the flow needs you, a soft one otherwise.
    fn pill(&self, flow: Option<&FlowView>) -> Span<'static> {
        let theme = self.theme();
        let soft = |style: Style| style.bg(theme.ui.selection.bg.unwrap_or(Color::Reset));
        let solid = |style: Style| {
            Style::default()
                .fg(Color::Black)
                .bg(style.fg.unwrap_or(Color::Reset))
        };
        let Some(flow) = flow else {
            return Span::styled(" you ", soft(theme.ui.accent));
        };
        let tone = theme.state(flow.state);
        let (text, style) = match flow.state {
            State::Blocked => ("blocked".to_string(), solid(tone)),
            State::NotResponding => ("not responding".to_string(), solid(tone)),
            State::Waiting => ("waiting for you".to_string(), solid(tone)),
            State::Working => ("working".to_string(), soft(tone)),
            State::Delegated => (
                format!("{} · agent", flow.phase.map_or("working", gerund)),
                soft(tone),
            ),
            State::Done if flow.merge_request.is_some() => ("MR open".to_string(), soft(tone)),
            State::Done => ("done".to_string(), soft(tone)),
            State::Ended => ("ended".to_string(), soft(tone)),
        };
        Span::styled(format!(" {text} "), style)
    }

    fn status(&self, key: &PaneKey) -> Option<&PaneStatus> {
        self.sidebar
            .panes
            .iter()
            .find(|pane| &pane.key == key)
            .map(|pane| &pane.status)
    }

    fn render_cards(&self, area: Rect, buf: &mut Buffer) -> u16 {
        let theme = self.theme();
        let flows = sidebar::workers(self.board());
        let gap = 2;
        let inner = area.width.saturating_sub(2);
        let card_width = inner.saturating_sub(gap * (COLUMNS as u16 - 1)) / COLUMNS as u16;
        let mut bottom = area.y;
        for index in 0..=flows.len() {
            let column = (index % COLUMNS) as u16;
            let row = (index / COLUMNS) as u16;
            let y = area.y + row * (CARD_ROWS + 1);
            if y + CARD_ROWS > area.y + area.height {
                break;
            }
            let rect = Rect::new(
                area.x + 1 + column * (card_width + gap),
                y,
                card_width,
                CARD_ROWS,
            );
            let flow = index.checked_sub(1).map(|at| flows[at]);
            let border = if index == self.state.selected {
                theme.ui.accent
            } else if flow
                .is_some_and(|flow| matches!(flow.state, State::Blocked | State::NotResponding))
            {
                theme.state.blocked
            } else {
                theme.ui.rule
            };
            let block = Block::bordered()
                .border_type(BorderType::Rounded)
                .border_style(border);
            let body = block.inner(rect);
            block.render(rect, buf);
            let width = body.width as usize;
            let lines = match flow {
                None => self.master_card(),
                Some(flow) => self.flow_card(flow, index - 1, width),
            };
            for (offset, line) in lines.iter().enumerate() {
                buf.set_line(body.x, body.y + offset as u16, line, body.width);
            }
            let pill = self.pill(flow);
            let pill_x = (body.x + body.width).saturating_sub(pill.width() as u16 + 1);
            buf.set_span(pill_x, body.y, &pill, pill.width() as u16);
            bottom = y + CARD_ROWS;
        }
        bottom
    }

    /// The selected flow's request, mode, branch and age, then its timeline beside its decisions and steps.
    fn render_detail(&self, area: Rect, buf: &mut Buffer) {
        let theme = self.theme();
        let flows = sidebar::workers(self.board());
        let width = area.width as usize;
        let Some(flow) = self
            .state
            .selected
            .checked_sub(1)
            .and_then(|at| flows.get(at).copied())
        else {
            let line = Line::from(vec![
                Span::styled(" 0 master", theme.ui.strong),
                Span::styled(
                    format!("  {}", self.sidebar.master.unwrap_or("")),
                    theme.ui.faint,
                ),
            ]);
            buf.set_line(area.x, area.y, &line, area.width);
            return;
        };
        let mut facts = vec![flow.request.clone(), name_of_mode(flow.mode).to_string()];
        if let Some(space) = &flow.workspace {
            facts.push(space.branch.clone());
            facts.push(format!("slot {}", space.slot));
        }
        facts.push(format!(
            "started {} ago",
            long_duration(self.sidebar.now.saturating_sub(flow.started_at))
        ));
        let header = Line::from(vec![
            Span::styled(
                format!(" {} {}", self.state.selected, flow.name),
                theme.ui.strong,
            ),
            Span::styled(
                format!(
                    "  {}",
                    clip(
                        &facts.join(" · "),
                        width.saturating_sub(flow.name.len() + 6)
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

    fn render_timeline(&self, flow: &FlowView, area: Rect, buf: &mut Buffer) {
        let theme = self.theme();
        let mut lines = Vec::new();
        let text_width = (area.width as usize).saturating_sub(10);
        for entry in &flow.timeline {
            let tone = match entry.tone {
                Tone::Neutral => theme.ui.muted,
                Tone::Progress => theme.state.working,
                Tone::Attention => theme.state.waiting,
                Tone::Problem => theme.state.blocked,
                Tone::Success => theme.state.done,
            };
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
        for (offset, line) in lines.iter().enumerate() {
            buf.set_line(area.x, area.y + 1 + offset as u16, line, area.width);
        }
    }

    fn render_facts(&self, flow: &FlowView, area: Rect, buf: &mut Buffer) {
        let theme = self.theme();
        let glyphs = &theme.glyphs;
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
        lines.push(Line::styled("steps", theme.ui.faint));
        let mut steps = Vec::new();
        for step in &flow.steps {
            let (glyph, style) = match step.status {
                StepStatus::Done => (glyphs.done, theme.state.done),
                StepStatus::Current => (glyphs.step_current, theme.state(flow.state)),
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
        lines.push(Line::from(steps));
        for (offset, line) in lines.iter().take(area.height as usize).enumerate() {
            buf.set_line(area.x, area.y + offset as u16, line, area.width);
        }
    }

    fn footer(&self) -> Line<'static> {
        let theme = self.theme();
        if let Some(notice) = self.notice {
            return Line::styled(format!(" {notice}"), theme.state.waiting);
        }
        let key = |text: &'static str| Span::styled(text, theme.ui.key);
        let hint = |text: String| Span::styled(text, theme.ui.faint);
        Line::from(vec![
            Span::raw(" "),
            key("0-9"),
            hint(" select   ".into()),
            key("h j k l"),
            hint(" move   ".into()),
            key("enter"),
            hint(" jump in   ".into()),
            key("n"),
            hint(" new flow   ".into()),
            key("x"),
            hint(" clear finished flow   ".into()),
            key("esc"),
            hint(format!(" back to {}", self.back)),
        ])
    }
}

impl Widget for BoardView<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let theme = self.theme();
        let width = area.width as usize;
        if area.height < 4 {
            return;
        }
        buf.set_line(area.x, area.y, &self.header(width), area.width);
        let rule = Line::styled("─".repeat(width), theme.ui.rule);
        buf.set_line(area.x, area.y + 1, &rule, area.width);
        let footer_y = area.y + area.height - 1;
        buf.set_line(area.x, footer_y - 1, &rule, area.width);
        buf.set_line(area.x, footer_y, &self.footer(), area.width);

        let cards = Rect::new(
            area.x,
            area.y + 2,
            area.width,
            footer_y.saturating_sub(area.y + 3),
        );
        let bottom = self.render_cards(cards, buf);
        let detail_y = bottom + 1;
        if detail_y + 3 < footer_y - 1 {
            let detail = Rect::new(area.x, detail_y, area.width, footer_y - 1 - detail_y);
            self.render_detail(detail, buf);
        }
    }
}

fn gerund(phase: Phase) -> &'static str {
    match phase {
        Phase::Design => "designing",
        Phase::Implement => "implementing",
        Phase::Review => "reviewing",
        Phase::MergeRequest => "opening the MR",
    }
}

fn name_of_mode(mode: Mode) -> &'static str {
    match mode {
        Mode::Auto => "auto",
        Mode::Guided => "guided",
    }
}

/// 41m, 1h 12m, 2d 3h: two units, so a long-running flow still shows its minutes.
pub fn long_duration(ms: u64) -> String {
    let minutes = ms / 60_000;
    match minutes {
        0 => short_duration(ms),
        1..60 => format!("{minutes}m"),
        60..1_440 => format!("{}h {}m", minutes / 60, minutes % 60),
        _ => format!("{}d {}h", minutes / 1_440, minutes % 1_440 / 60),
    }
}

/// HH:MM of a Unix ms time in a zone `offset_secs` east of UTC.
pub fn clock(ms: u64, offset_secs: i64) -> String {
    let secs = (ms / 1_000) as i64 + offset_secs;
    let minute_of_day = secs.rem_euclid(86_400) / 60;
    format!("{:02}:{:02}", minute_of_day / 60, minute_of_day % 60)
}

/// This machine's offset from UTC right now, in seconds.
pub fn local_offset_secs() -> i64 {
    // SAFETY: time and localtime_r only write into the tm we pass; a zeroed tm is a valid start.
    unsafe {
        let now = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&now, &mut tm).is_null() {
            return 0;
        }
        tm.tm_gmtoff
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moves_over_the_grid_but_not_off_it() {
        let state = BoardState { selected: 1 };

        assert_eq!(state.moved(1, 6).selected, 2);
        assert_eq!(state.moved(3, 6).selected, 4);
        assert_eq!(state.moved(-3, 6).selected, 1, "no row above");
        assert_eq!(BoardState { selected: 5 }.moved(1, 6).selected, 5);
    }

    #[test]
    fn writes_durations_and_clock_times() {
        assert_eq!(long_duration(41 * 60_000), "41m");
        assert_eq!(long_duration(72 * 60_000), "1h 12m");
        assert_eq!(long_duration(51 * 3_600_000), "2d 3h");
        assert_eq!(long_duration(30_000), "30s");
        assert_eq!(clock(12 * 3_600_000 + 4 * 60_000, 0), "12:04");
        assert_eq!(clock(23 * 3_600_000, 2 * 3_600), "01:00");
    }
}
