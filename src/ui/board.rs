use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Widget};

use crate::event::Mode;
use crate::event::Phase;
use crate::output::short_duration;
use crate::proto::{PaneKey, PaneStatus};
use crate::ui::sidebar::{self, Sidebar, clip, clip_place, split, wrap};
use crate::ui::theme::Theme;
use crate::view::{Board, FlowView, Project, Projects, State, StepStatus, Tone};

/// Cards per row.
const COLUMNS: usize = 3;
/// The longest task bar on a card.
const BAR_WIDTH: usize = 27;
/// A card's rows: its four lines inside a border.
const CARD_ROWS: u16 = 6;
/// Rows every project's cards leave for the selected flow's detail.
const DETAIL_ROWS: u16 = 10;

/// Which card the board's cursor is on: 0 is the master, n the nth worker; with `all`, the nth flow
/// of every project's, counted from 0.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BoardState {
    pub selected: usize,
    pub all: bool,
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
            ..self
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
    /// Every project's flows, this project first, once `flow status --all` answered.
    pub projects: Option<&'a Projects>,
    /// This project's store, to tell its flows from other projects'.
    pub here: Option<&'a str>,
}

/// Every project's flows in board order, each with its project.
pub fn every_flow(projects: &Projects) -> Vec<(&Project, &FlowView)> {
    projects
        .projects
        .iter()
        .flat_map(|project| project.board.flows.iter().map(move |flow| (project, flow)))
        .collect()
}

impl BoardView<'_> {
    fn theme(&self) -> &Theme {
        self.sidebar.theme
    }

    fn board(&self) -> &Board {
        self.sidebar.board
    }

    fn header(&self, width: usize) -> Line<'static> {
        if self.state.all {
            return self.all_header(width);
        }
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
            (count(&[State::Idle]), "idle", theme.ui.faint),
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

    fn all_header(&self, width: usize) -> Line<'static> {
        let theme = self.theme();
        let mut right = Vec::new();
        if let Some(projects) = self.projects {
            let flows = every_flow(projects);
            let needs_you = flows.iter().filter(|(_, flow)| flow.needs_you()).count();
            let count = projects.projects.len();
            right.push(Span::styled(
                format!(
                    "{count} project{} · {} flow{}",
                    plural(count),
                    flows.len(),
                    plural(flows.len())
                ),
                theme.ui.faint,
            ));
            if needs_you > 0 {
                right.push(Span::styled(" · ", theme.ui.faint));
                right.push(Span::styled(
                    format!("{needs_you} need you"),
                    theme.state.waiting,
                ));
            }
        }
        right.push(Span::raw(" "));
        split(
            vec![
                Span::raw(" "),
                Span::styled(theme.glyphs.mark, theme.ui.accent),
                Span::styled(" flow", theme.ui.strong),
                Span::styled(" board · every project", theme.ui.faint),
            ],
            right,
            width,
        )
    }

    /// A project's row above its cards: its name, how many flows and how many need you.
    fn project_line(&self, project: &Project, width: usize) -> Line<'static> {
        let theme = self.theme();
        let flows = project.board.flows.len();
        let mut left = vec![
            Span::raw(" "),
            Span::styled(project.name.clone(), theme.ui.strong),
            Span::styled(format!("  {flows} flow{}", plural(flows)), theme.ui.faint),
        ];
        if project.board.needs_you > 0 {
            left.push(Span::styled(" · ", theme.ui.faint));
            left.push(Span::styled(
                format!("{} need you", project.board.needs_you),
                theme.state.waiting,
            ));
        }
        let mut right = Vec::new();
        if self.here == Some(project.root.as_str()) {
            right.push(Span::styled("this project ", theme.ui.faint));
        }
        split(left, right, width)
    }

    fn master_card(&self, width: usize) -> [Line<'static>; 4] {
        let theme = self.theme();
        let workers = sidebar::workers(self.board()).len();
        let place = clip_place(self.sidebar.master.unwrap_or(""), width.saturating_sub(4));
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
            let progress = match tasks {
                // Nothing to measure yet: no bar, only the age.
                None => vec![Span::raw("   ")],
                Some((done, total)) => {
                    let count = format!("{done}/{total}");
                    let bar = width
                        .saturating_sub(3 + 2 + count.chars().count() + ago.chars().count() + 1)
                        .min(BAR_WIDTH);
                    let filled = (bar as u32 * done.min(total) / total) as usize;
                    vec![
                        Span::raw("   "),
                        Span::styled("━".repeat(filled), theme.ui.muted),
                        Span::styled("─".repeat(bar - filled), theme.ui.faint),
                        Span::styled(format!("  {count}"), theme.ui.text),
                    ]
                }
            };
            split(progress, vec![Span::styled(ago, theme.ui.faint)], width)
        };

        let (note, style) = match flow.state {
            State::Blocked | State::NotResponding => (flow.blocker.clone(), theme.state.blocked),
            State::Waiting => (flow.blocker.clone(), theme.ui.muted),
            State::Delegated => (
                flow.delegate.as_ref().map(|text| format!("agent: {text}")),
                theme.ui.faint,
            ),
            State::Working | State::Idle => (sidebar::last_note(flow), theme.ui.faint),
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
                .fg(theme.on_solid())
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
            State::Idle => ("idle".to_string(), soft(tone)),
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
            self.render_card(flow, index, index == self.state.selected, rect, buf);
            bottom = y + CARD_ROWS;
        }
        bottom
    }

    /// One card: the master's when `flow` is None; `index` picks its digit (1-9 for a worker).
    fn render_card(
        &self,
        flow: Option<&FlowView>,
        index: usize,
        selected: bool,
        rect: Rect,
        buf: &mut Buffer,
    ) {
        let theme = self.theme();
        let border = if selected {
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
            None => self.master_card(width),
            Some(flow) => self.flow_card(flow, index.wrapping_sub(1), width),
        };
        for (offset, line) in lines.iter().enumerate() {
            buf.set_line(body.x, body.y + offset as u16, line, body.width);
        }
        let pill = self.pill(flow);
        let pill_x = (body.x + body.width).saturating_sub(pill.width() as u16 + 1);
        buf.set_span(pill_x, body.y, &pill, pill.width() as u16);
    }

    /// Every project's row and cards, scrolled so the selected card shows; returns the last row used.
    fn render_projects(&self, projects: &Projects, area: Rect, buf: &mut Buffer) -> u16 {
        let theme = self.theme();
        let width = area.width as usize;
        if projects.projects.is_empty() {
            let line = Line::styled(" no flows in any project", theme.ui.faint);
            buf.set_line(area.x, area.y, &line, area.width);
            return area.y + 1;
        }
        let gap = 2;
        let inner = area.width.saturating_sub(2);
        let card_width = inner.saturating_sub(gap * (COLUMNS as u16 - 1)) / COLUMNS as u16;

        // Rows from the top of the first project: each project's line, then its rows of cards.
        enum Item<'p> {
            Project(&'p Project),
            Card(&'p FlowView, usize, u16),
        }
        let mut items = Vec::new();
        let mut y: u16 = 0;
        let mut number = 0;
        for project in &projects.projects {
            items.push((y, Item::Project(project)));
            y += 1;
            for (at, flow) in project.board.flows.iter().enumerate() {
                let row = (at / COLUMNS) as u16;
                let column = (at % COLUMNS) as u16;
                items.push((y + row * (CARD_ROWS + 1), Item::Card(flow, number, column)));
                number += 1;
            }
            let rows = project.board.flows.len().div_ceil(COLUMNS) as u16;
            y += rows * (CARD_ROWS + 1);
        }
        let total = y.saturating_sub(1);
        let room = if area.height > DETAIL_ROWS + CARD_ROWS + 2 {
            area.height - DETAIL_ROWS
        } else {
            area.height
        }
        .min(total);
        let selected_bottom = items
            .iter()
            .find_map(|(top, item)| match item {
                Item::Card(_, number, _) if *number == self.state.selected => Some(top + CARD_ROWS),
                _ => None,
            })
            .unwrap_or(0);
        let scroll = selected_bottom.saturating_sub(room);

        for (top, item) in &items {
            let height = match item {
                Item::Project(_) => 1,
                Item::Card(..) => CARD_ROWS,
            };
            if *top < scroll || top + height > scroll + room {
                continue;
            }
            let y = area.y + top - scroll;
            match item {
                Item::Project(project) => {
                    buf.set_line(area.x, y, &self.project_line(project, width), area.width);
                }
                Item::Card(flow, number, column) => {
                    let rect = Rect::new(
                        area.x + 1 + column * (card_width + gap),
                        y,
                        card_width,
                        CARD_ROWS,
                    );
                    self.render_card(Some(flow), 0, *number == self.state.selected, rect, buf);
                }
            }
        }
        area.y + room
    }

    /// The selected flow's request, mode, branch and age, then its timeline beside its decisions and steps.
    fn render_detail(&self, area: Rect, buf: &mut Buffer) {
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
        facts.push(flow.request.clone());
        facts.push(name_of_mode(flow.mode).to_string());
        if let Some(space) = &flow.workspace {
            facts.push(space.branch.clone());
            facts.push(format!("slot {}", space.slot));
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
        let mut facts = vec![format!(
            "{count} worker{}",
            if count == 1 { "" } else { "s" }
        )];
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
        let entry = |flow: &FlowView, text: String, style: Style| {
            let lead = format!("  {} {}  ", sidebar.glyph(flow.state), flow.name);
            Line::from(vec![
                Span::styled("  ", theme.ui.text),
                Span::styled(sidebar.glyph(flow.state), theme.state(flow.state)),
                Span::styled(format!(" {}  ", flow.name), theme.ui.strong),
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
        let rows = area.height.saturating_sub(2) as usize;
        for (offset, line) in lines.iter().take(rows).enumerate() {
            buf.set_line(area.x, area.y + 2 + offset as u16, line, area.width);
        }
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
        if self.state.all {
            return Line::from(vec![
                Span::raw(" "),
                key("h j k l"),
                hint(" move   ".into()),
                key("enter"),
                hint(" jump in   ".into()),
                key("n"),
                hint(" new flow   ".into()),
                key("x"),
                hint(" clear finished flow   ".into()),
                key("a"),
                hint(" this project   ".into()),
                key("esc"),
                hint(format!(" back to {}", self.back)),
            ]);
        }
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
            key("a"),
            hint(" every project   ".into()),
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
        let bottom = match (self.state.all, self.projects) {
            (false, _) => self.render_cards(cards, buf),
            (true, Some(projects)) => self.render_projects(projects, cards, buf),
            (true, None) => {
                let line = Line::styled(" reading every project…", theme.ui.faint);
                buf.set_line(cards.x, cards.y, &line, cards.width);
                return;
            }
        };
        let detail_y = bottom + 1;
        if detail_y + 3 < footer_y - 1 {
            let detail = Rect::new(area.x, detail_y, area.width, footer_y - 1 - detail_y);
            self.render_detail(detail, buf);
        }
    }
}

fn plural(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
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
        let state = BoardState {
            selected: 1,
            all: false,
        };

        assert_eq!(state.moved(1, 6).selected, 2);
        assert_eq!(state.moved(3, 6).selected, 4);
        assert_eq!(state.moved(-3, 6).selected, 1, "no row above");
        let last = BoardState {
            selected: 5,
            all: true,
        };
        assert_eq!(last.moved(1, 6), last);
        assert!(last.moved(-1, 6).all, "moving keeps every project in view");
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
