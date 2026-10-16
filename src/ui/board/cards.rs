use std::borrow::Cow;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Widget};

use super::{BoardView, CARD_ROWS, COLUMNS, DETAIL_ROWS, Item, card_rect, every_flow};
use crate::event::{Mode, Phase};
use crate::proto::{PaneKey, PaneStatus};
use crate::text::{plural, short_duration};
use crate::ui::fit::{clip, clip_place, split};
use crate::ui::sidebar;
use crate::view::{FlowView, Project, Projects, State};

impl BoardView<'_> {
    pub(super) fn all_header(&self, width: usize) -> Line<'static> {
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
                Span::styled(" 0   ", theme.ui.faint),
                Span::styled("master", theme.ui.strong),
            ]),
            Line::styled(format!("   {place}"), theme.ui.faint),
            Line::styled(
                format!("   {workers} worker{}", plural(workers)),
                theme.ui.faint,
            ),
            Line::styled(format!("   {status}"), theme.state.waiting),
        ]
    }

    /// `number` is the card's place among the workers, from 1; 1-9 show as the digit that picks it.
    fn flow_card(
        &self,
        flow: &FlowView,
        number: Option<usize>,
        width: usize,
    ) -> [Line<'static>; 4] {
        let theme = self.theme();
        let tone = theme.state_style(flow.state);
        let digit = match number {
            Some(number @ 1..=9) => number.to_string(),
            _ => " ".to_string(),
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
                Some(space) => space.branch.clone().unwrap_or_default(),
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
                // Nothing to measure yet: only the age.
                None => vec![Span::raw("   ")],
                Some((done, total)) => {
                    vec![
                        Span::raw("   "),
                        Span::styled(format!("{done}/{total} tasks"), theme.ui.faint),
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

    /// The label at a card's top right: its state in words, in the state's color, with no fill, as
    /// Claude Code says a state in colored text.
    fn state_label(&self, flow: Option<&FlowView>) -> Span<'static> {
        let theme = self.theme();
        let Some(flow) = flow else {
            return Span::styled("you", theme.ui.accent);
        };
        let text: Cow<'static, str> = match flow.state {
            State::Blocked => "blocked".into(),
            State::NotResponding => "not responding".into(),
            State::Waiting => "waiting for you".into(),
            State::Working => "working".into(),
            State::Idle => "ready".into(),
            State::Delegated => format!("{} · agent", flow.phase.map_or("working", gerund)).into(),
            State::Done if flow.merge_request.is_some() => "MR open".into(),
            State::Done => "done".into(),
            State::Ended => "ended".into(),
        };
        let style = match flow.state {
            State::Idle => theme.ui.muted,
            state => theme.state_style(state),
        };
        Span::styled(text, style)
    }

    pub(super) fn render_cards(&self, area: Rect, buf: &mut Buffer) -> u16 {
        let flows = sidebar::workers(self.board());
        let mut bottom = area.y;
        for index in 0..=flows.len() {
            let row = (index / COLUMNS) as u16;
            let y = area.y + row * (CARD_ROWS + 1);
            if y + CARD_ROWS > area.y + area.height {
                break;
            }
            let rect = card_rect(area, index % COLUMNS, y);
            let flow = index.checked_sub(1).map(|at| flows[at]);
            self.render_card(flow, Some(index), index == self.state.selected, rect, buf);
            bottom = y + CARD_ROWS;
        }
        bottom
    }

    /// One card: the master's when `flow` is None; `number` picks a worker's digit.
    fn render_card(
        &self,
        flow: Option<&FlowView>,
        number: Option<usize>,
        selected: bool,
        rect: Rect,
        buf: &mut Buffer,
    ) {
        let theme = self.theme();
        // Only the selected card is outlined; the glyph and the state's words say the rest.
        let block = Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(theme.ui.accent);
        let body = block.inner(rect);
        if selected {
            block.render(rect, buf);
        }
        let width = body.width as usize;
        let lines = match flow {
            None => self.master_card(width),
            Some(flow) => self.flow_card(flow, number, width),
        };
        for (line, row) in lines.iter().zip(body.rows()) {
            buf.set_line(row.x, row.y, line, row.width);
        }
        let label = self.state_label(flow);
        let width = u16::try_from(label.width()).unwrap_or(u16::MAX);
        let x = (body.x + body.width).saturating_sub(width.saturating_add(1));
        buf.set_span(x, body.y, &label, width);
    }

    /// Every project's row and cards, scrolled so the selected card shows; returns the last row used.
    pub(super) fn render_projects(&self, projects: &Projects, area: Rect, buf: &mut Buffer) -> u16 {
        let theme = self.theme();
        let width = area.width as usize;
        if projects.projects.is_empty() {
            let line = Line::styled(" no flows in any project", theme.ui.faint);
            buf.set_line(area.x, area.y, &line, area.width);
            return area.y + 1;
        }
        // Rows from the top of the first project: each project's line, then its rows of cards.
        let mut items = Vec::new();
        let mut y: u16 = 0;
        let mut number = 0;
        for project in &projects.projects {
            items.push((y, Item::Project(project)));
            y += 1;
            for (at, flow) in project.board.flows.iter().enumerate() {
                let row = (at / COLUMNS) as u16;
                let card = Item::Card {
                    flow,
                    number,
                    column: at % COLUMNS,
                };
                items.push((y + row * (CARD_ROWS + 1), card));
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
                Item::Card { number, .. } if *number == self.state.selected => {
                    Some(top + CARD_ROWS)
                }
                _ => None,
            })
            .unwrap_or(0);
        let scroll = selected_bottom.saturating_sub(room);

        for (top, item) in &items {
            let height = match item {
                Item::Project(_) => 1,
                Item::Card { .. } => CARD_ROWS,
            };
            if *top < scroll || top + height > scroll + room {
                continue;
            }
            let y = area.y + top - scroll;
            match item {
                Item::Project(project) => {
                    buf.set_line(area.x, y, &self.project_line(project, width), area.width);
                }
                Item::Card {
                    flow,
                    number,
                    column,
                } => {
                    let rect = card_rect(area, *column, y);
                    self.render_card(Some(flow), None, *number == self.state.selected, rect, buf);
                }
            }
        }
        area.y + room
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::EventKind;
    use crate::ui::board::fixtures::{flow, text, with_view};

    #[test]
    fn the_master_card_says_where_it_runs_and_how_many_workers_it_has() {
        let flows = vec![
            flow("shop-221", Mode::Auto, vec![]),
            flow("shop-230", Mode::Auto, vec![]),
        ];

        let card = with_view(flows, |view| view.master_card(30).map(|line| text(&line)));

        assert_eq!(card[0], " 0   master");
        assert_eq!(card[1], "   ~/shop · main");
        assert_eq!(card[2], "   2 workers");
        assert_eq!(card[3], "   ", "no notice while the master runs");
    }

    #[test]
    fn a_flow_card_shows_its_progress_and_what_holds_it() {
        let blocked = flow(
            "shop-221",
            Mode::Guided,
            vec![
                EventKind::Phase {
                    phase: Phase::Implement,
                },
                EventKind::Tasks { done: 4, total: 9 },
                EventKind::Blocked {
                    reason: "migration needs a default".into(),
                },
            ],
        );

        let card = with_view(vec![blocked.clone()], |view| {
            view.flow_card(&blocked, Some(1), 40)
                .map(|line| text(&line))
        });

        assert!(card[0].starts_with(" 1 "), "{card:#?}");
        assert!(card[0].ends_with(" shop-221"), "{card:#?}");
        assert!(card[1].ends_with("  · guided"), "{card:#?}");
        assert!(card[2].starts_with("   4/9 tasks"), "{card:#?}");
        assert!(card[2].ends_with("27m ago "), "{card:#?}");
        assert_eq!(card[3], "   migration needs a default");
    }

    #[test]
    fn a_finished_flow_card_shows_its_branch_and_its_request() {
        let done = flow(
            "shop-240",
            Mode::Guided,
            vec![EventKind::MergeRequest {
                url: "https://x/88".into(),
            }],
        );
        let ended = flow("shop-241", Mode::Guided, vec![EventKind::Ended]);

        let cards = with_view(vec![done.clone(), ended.clone()], |view| {
            [&done, &ended].map(|flow| view.flow_card(flow, Some(2), 40).map(|line| text(&line)))
        });

        for (card, name) in cards.iter().zip(["shop-240", "shop-241"]) {
            assert!(!card[1].contains("guided"), "{card:#?}");
            assert!(card[2].starts_with(&format!("   flow/{name}")), "{card:#?}");
            assert_eq!(
                card[3],
                format!("   {} add CSV export", name.to_uppercase())
            );
        }
    }

    #[test]
    fn only_the_first_nine_cards_get_a_digit() {
        let one = flow("shop-221", Mode::Auto, vec![]);

        let firsts = with_view(vec![one.clone()], |view| {
            [Some(9), Some(10), None].map(|number| text(&view.flow_card(&one, number, 40)[0]))
        });

        assert!(firsts[0].starts_with(" 9 "), "{firsts:?}");
        assert!(firsts[1].starts_with("   "), "{firsts:?}");
        assert!(firsts[2].starts_with("   "), "{firsts:?}");
    }
}
