//! The board's cards: the master's, each flow's, project headings and state labels

use std::borrow::Cow;

use ratatui::buffer::Buffer;
use ratatui::layout::{Margin, Rect};
use ratatui::text::{Line, Span};

use super::{BoardView, CARD_ROWS, COLUMNS, DETAIL_ROWS, Item, card_rect, every_flow};
use crate::event::{Mode, ServiceStatus};
use crate::proto::{PaneKey, PaneStatus};
use crate::text::{plural, short_duration};
use crate::ui::fit::{clip, clip_place, columns, spans_width, split};
use crate::ui::focus;
use crate::ui::sidebar;
use crate::view::{FlowView, Project, Projects, State};
use crate::workflow::Phase;

/// The columns of a name a card keeps before its state label says less.
const NAME_KEPT: usize = 10;

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
                Span::styled(" saaio", theme.ui.strong),
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
            Some(PaneStatus::Exited { .. }) => {
                format!("exited - {} resumes it", self.sidebar.prefix.then("r"))
            }
            Some(PaneStatus::Failed { .. }) => "could not start".to_string(),
            _ => String::new(),
        };
        [
            self.first_row(
                vec![
                    Span::styled(" 0   ", theme.ui.faint),
                    Span::styled("master", theme.ui.strong),
                ],
                None,
                width,
            ),
            Line::styled(format!("   {place}"), theme.ui.faint),
            Line::styled(
                match workers {
                    0 => "   no workers yet".to_string(),
                    _ => format!("   {workers} worker{}", plural(workers)),
                },
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
        let tone = theme.flow_style(flow);
        let digit = match number {
            Some(number @ 1..=9) => number.to_string(),
            _ => " ".to_string(),
        };

        let first = self.first_row(
            vec![
                Span::styled(format!(" {digit} "), theme.ui.faint),
                Span::styled(self.sidebar.glyph(flow.state), tone),
                Span::styled(format!(" {}", flow.name), theme.ui.strong),
            ],
            Some(flow),
            width,
        );

        let mut second = vec![Span::raw("   ")];
        let steps = self.sidebar.steps(flow);
        if !steps.is_empty() {
            second.extend(steps);
            second.push(Span::raw("  "));
        }
        second.push(self.sidebar.label(flow));
        let mode = format!("  · {}", flow.mode.name());
        if matches!(flow.mode, Mode::Guided | Mode::Plain)
            && !flow.is_finished()
            && spans_width(&second) + columns(&mode) < width
        {
            second.push(Span::styled(mode, theme.ui.faint));
        }

        let ago = format!(
            "{} ",
            short_duration(self.sidebar.now.saturating_sub(flow.changed_at))
        );
        let third = if flow.is_finished() {
            let text = match &flow.workspace {
                Some(space) if flow.is_plain() => self.sidebar.tilde(&space.path),
                Some(space) => space.branch.clone().unwrap_or_default(),
                None => String::new(),
            };
            let left = vec![Span::styled(format!("   {text}"), theme.ui.faint)];
            let right = self.agent_and_age(flow, &left, ago, width);
            split(left, right, width)
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
            let right = self.agent_and_age(flow, &progress, ago, width);
            split(progress, right, width)
        };

        let (note, style) = match flow.state {
            // The glyph and the label say how it stands; what it asks reads as text.
            State::Blocked | State::NotResponding | State::Waiting => {
                (flow.blocker.clone(), theme.ui.text)
            }
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

    /// The right of a card's third row: the worker's agent and `ago`, as the sidebar shows them, or
    /// only `ago` where both do not fit beside `left`.
    fn agent_and_age(
        &self,
        flow: &FlowView,
        left: &[Span<'static>],
        ago: String,
        width: usize,
    ) -> Vec<Span<'static>> {
        let faint = self.theme().ui.faint;
        let agent = flow
            .workspace
            .as_ref()
            .map(|space| format!("{} · ", space.agent))
            .filter(|agent| spans_width(left) + columns(agent) + columns(&ago) < width);
        agent
            .into_iter()
            .chain([ago])
            .map(|text| Span::styled(text, faint))
            .collect()
    }

    /// A card's first row: `left` cut with … to stay two columns clear of the state label; the
    /// label says less when the name would keep fewer than NAME_KEPT columns.
    fn first_row(
        &self,
        left: Vec<Span<'static>>,
        flow: Option<&FlowView>,
        width: usize,
    ) -> Line<'static> {
        let room = |label: &Span| width.saturating_sub(label.width() + 3);
        let full = spans_width(&left);
        let name = left.last().map_or(0, Span::width);
        let mut label = self.state_label(flow, false);
        if full > room(&label) && room(&label) + name < full + name.min(NAME_KEPT) {
            label = self.state_label(flow, true);
        }
        let mut room = room(&label);
        let mut fitted = Vec::new();
        for span in left {
            if room == 0 {
                break;
            }
            let cut = clip(&span.content, room);
            room = room.saturating_sub(columns(&cut));
            fitted.push(Span::styled(cut, span.style));
        }
        split(fitted, vec![label, Span::raw(" ")], width)
    }

    /// The label at a card's top right: its state in words, in the state's color, with no fill, as
    /// Claude Code says a state in colored text; `short` for a narrow card.
    fn state_label(&self, flow: Option<&FlowView>, short: bool) -> Span<'static> {
        let theme = self.theme();
        let Some(flow) = flow else {
            return Span::styled("you", theme.ui.accent);
        };
        let text: Cow<'static, str> = match flow.state {
            State::Blocked => "blocked".into(),
            State::NotResponding => "not responding".into(),
            State::Waiting if short => "waiting".into(),
            State::Waiting => "waiting for you".into(),
            State::Working => "working".into(),
            State::Idle => "ready".into(),
            State::Delegated if short => "agent".into(),
            State::Delegated => format!(
                "{} · agent",
                flow.phase.as_ref().map_or("working", Phase::name)
            )
            .into(),
            State::Done => match flow.merge_request.as_deref() {
                Some(url) => {
                    let kind = if url.contains("/pull/") { "PR" } else { "MR" };
                    let stage = if flow.merged { "merged" } else { "open" };
                    format!("{kind} {stage}").into()
                }
                None => "done".into(),
            },
            State::Ended => "ended".into(),
        };
        let services = flow
            .services
            .as_ref()
            .and_then(|services| match services.status {
                ServiceStatus::Pausing | ServiceStatus::Paused => Some("paused"),
                ServiceStatus::Resuming => Some("resuming"),
                ServiceStatus::ResumeFailed => Some("services down"),
                ServiceStatus::Running | ServiceStatus::PauseFailed => None,
            });
        let text = match services {
            Some(services) if !short => format!("{text} · {services}").into(),
            _ => text,
        };
        Span::styled(text, theme.flow_style(flow))
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
        // Inside the rect by a cell, where a border would be, so a card keeps its place.
        let body = rect.inner(Margin::new(1, 1));
        let width = body.width as usize;
        let lines = match flow {
            None => self.master_card(width),
            Some(flow) => self.flow_card(flow, number, width),
        };
        for (line, row) in lines.iter().zip(body.rows()) {
            buf.set_line(row.x, row.y, line, row.width);
        }
        if selected {
            focus::mark(buf, rect, self.theme());
        }
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

        assert_eq!(card[0], format!(" 0   master{}you ", " ".repeat(30 - 15)));
        assert_eq!(card[1], "   ~/shop · main");
        assert_eq!(card[2], "   2 workers");
        assert_eq!(card[3], "   ", "no notice while the master runs");
    }

    #[test]
    fn a_master_alone_says_there_are_no_workers_yet() {
        let (card, header) = with_view(vec![], |view| {
            (
                view.master_card(30).map(|line| text(&line)),
                text(&view.header(60)),
            )
        });

        assert_eq!(card[2], "   no workers yet");
        assert!(header.ends_with(" 1 session "), "{header}");
    }

    #[test]
    fn a_flow_card_shows_its_progress_and_what_holds_it() {
        let blocked = flow(
            "shop-221",
            Mode::Guided,
            vec![
                EventKind::Phase {
                    phase: Phase::of("implement"),
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

        assert!(card[0].starts_with(" 1 ● shop-221 "), "{card:#?}");
        assert!(card[0].ends_with("  blocked "), "{card:#?}");
        assert!(card[1].ends_with("  · guided"), "{card:#?}");
        assert!(card[2].starts_with("   4/9 tasks"), "{card:#?}");
        assert!(card[2].ends_with(" claude · 27m "), "{card:#?}");
        assert_eq!(card[3], "   migration needs a default");
    }

    #[test]
    fn a_narrow_card_drops_the_agent_before_the_age() {
        let working = flow(
            "shop-221",
            Mode::Auto,
            vec![EventKind::Tasks { done: 4, total: 9 }],
        );

        let third = with_view(vec![working.clone()], |view| {
            text(&view.flow_card(&working, Some(1), 22)[2])
        });

        assert!(!third.contains("claude"), "{third:?}");
        assert!(third.starts_with("   4/9 tasks"), "{third:?}");
        assert!(third.ends_with(" 30m "), "{third:?}");
    }

    #[test]
    fn a_finished_flow_card_shows_its_branch_and_its_request() {
        let done = flow(
            "shop-240",
            Mode::Guided,
            vec![
                EventKind::MergeRequest {
                    url: "https://x/88".into(),
                },
                EventKind::Ended,
            ],
        );
        let ended = flow("shop-241", Mode::Guided, vec![EventKind::Ended]);

        let cards = with_view(vec![done.clone(), ended.clone()], |view| {
            [&done, &ended].map(|flow| view.flow_card(flow, Some(2), 40).map(|line| text(&line)))
        });

        for (card, name) in cards.iter().zip(["shop-240", "shop-241"]) {
            assert!(!card[1].contains("guided"), "{card:#?}");
            assert!(card[2].starts_with(&format!("   flow/{name}")), "{card:#?}");
            assert!(card[2].contains(" claude · "), "{card:#?}");
            assert_eq!(
                card[3],
                format!("   {} add CSV export", name.to_uppercase())
            );
        }
    }

    #[test]
    fn a_narrow_card_cuts_the_name_before_its_label_and_drops_the_mode() {
        let waiting = flow(
            "shop-230-rate-limit-the-export",
            Mode::Guided,
            vec![
                EventKind::Phase {
                    phase: Phase::of("plan"),
                },
                EventKind::waiting("Which approach?"),
            ],
        );

        let (card, styles) = with_view(vec![waiting.clone()], |view| {
            let card = view.flow_card(&waiting, Some(2), 30);
            let note = card[3].style;
            (card.map(|line| text(&line)), (note, view.theme().ui.text))
        });

        assert_eq!(card[0], " 2 ● shop-230-rate-…  waiting ");
        assert_eq!(card[1], "   ●───○───○───○  plan", "no room for the mode");
        assert_eq!(styles.0, styles.1, "the question reads as text");
    }

    #[test]
    fn names_a_github_pull_request_a_pr() {
        let pull = flow(
            "shop-240",
            Mode::Auto,
            vec![EventKind::MergeRequest {
                url: "https://github.com/acme/shop/pull/418".into(),
            }],
        );
        let merge = flow(
            "shop-241",
            Mode::Auto,
            vec![EventKind::MergeRequest {
                url: "https://gitlab.com/shop/-/merge_requests/9".into(),
            }],
        );

        let labels = with_view(vec![], |view| {
            [&pull, &merge].map(|flow| view.state_label(Some(flow), false).content.to_string())
        });

        assert_eq!(labels, ["PR open", "MR open"]);
    }

    #[test]
    fn says_a_merged_one_is_merged() {
        let merged = |url: &str| {
            flow(
                "shop-240",
                Mode::Auto,
                vec![EventKind::Merged {
                    url: url.into(),
                    into: None,
                }],
            )
        };
        let pull = merged("https://github.com/acme/shop/pull/418");
        let merge = merged("https://gitlab.com/shop/-/merge_requests/9");

        let labels = with_view(vec![], |view| {
            [&pull, &merge].map(|flow| view.state_label(Some(flow), false).content.to_string())
        });

        assert_eq!(labels, ["PR merged", "MR merged"]);
    }

    #[test]
    fn says_where_paused_services_stand_beside_the_state() {
        let with = |state: EventKind, status| {
            flow(
                "shop-240",
                Mode::Auto,
                vec![
                    state,
                    EventKind::Services {
                        status,
                        reason: None,
                    },
                ],
            )
        };
        let idle = || EventKind::Idle {
            note: "Plan ready.".into(),
        };
        let flows = [
            with(idle(), ServiceStatus::Paused),
            with(idle(), ServiceStatus::Pausing),
            with(EventKind::Working, ServiceStatus::Resuming),
            with(EventKind::Working, ServiceStatus::ResumeFailed),
            with(EventKind::Working, ServiceStatus::Running),
            with(idle(), ServiceStatus::PauseFailed),
        ];

        let (labels, short) = with_view(vec![], |view| {
            (
                flows
                    .each_ref()
                    .map(|flow| view.state_label(Some(flow), false).content.to_string()),
                view.state_label(Some(&flows[0]), true).content.to_string(),
            )
        });

        assert_eq!(
            labels,
            [
                "ready · paused",
                "ready · paused",
                "working · resuming",
                "working · services down",
                "working",
                "ready",
            ]
        );
        assert_eq!(short, "ready", "a narrow card keeps the state");
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
