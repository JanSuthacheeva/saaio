use ratatui::Frame as TuiFrame;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};

use super::{App, View};
use crate::proto::{PaneKey, PaneStatus};
use crate::text::plural;
use crate::ui::board::{BoardState, BoardView};
use crate::ui::dialog::{draw_dialog, draw_help, draw_no_session, draw_not_running};
use crate::ui::fit::{clip, clip_place, columns, spans_width};
use crate::ui::layout::{self, Areas};
use crate::ui::newflow::NewFlowView;
use crate::ui::peek::PeekLine;
use crate::ui::rail::Rail;
use crate::ui::screen::ScreenView;
use crate::ui::theme::Theme;
use crate::view::{FlowView, State};

impl App {
    pub fn draw(&self, frame: &mut TuiFrame, theme: &Theme, peek: &[PeekLine]) {
        frame.render_widget(Block::default().style(theme.ui.background), frame.area());
        if let View::Board(state) = self.view {
            self.draw_board(frame, state, theme);
            return;
        }
        let areas = self.areas();
        self.draw_title(frame, areas.title, theme);
        if let Some(rule) = areas.rule {
            self.draw_rule(frame, rule, theme);
        }
        frame.render_widget(
            ScreenView {
                screen: &self.screen,
                text: theme.palette.text,
                background: theme.palette.background,
            },
            areas.pane,
        );
        if let Some(bar) = areas.bar {
            self.draw_bar(frame, bar, theme);
        }
        if let Some(side) = areas.side {
            if self.rail {
                frame.render_widget(
                    Rail {
                        sidebar: self.sidebar(theme, &[]),
                    },
                    side,
                );
            } else {
                frame.render_widget(self.sidebar(theme, peek), side);
            }
        }
        let asking = matches!(self.view, View::Confirm { .. });
        match self.status(&self.focus) {
            Some(PaneStatus::Running) => {}
            Some(_) if asking => {}
            Some(status) => draw_not_running(
                frame,
                areas.pane,
                status,
                self.ended_worker().as_deref(),
                theme,
            ),
            None if self.focus != PaneKey::Master => draw_no_session(frame, areas.pane, theme),
            None => {}
        }
        self.draw_view(frame, &areas, theme);
    }

    fn draw_board(&self, frame: &mut TuiFrame, state: BoardState, theme: &Theme) {
        let back = match &self.focus {
            PaneKey::Master => "master",
            PaneKey::Flow(id) => self.worker(id).map_or(id.as_str(), |flow| &flow.name),
        };
        frame.render_widget(
            BoardView {
                sidebar: self.sidebar(theme, &[]),
                state,
                back,
                offset_secs: self.offset_secs,
                notice: self.notice.as_deref(),
                projects: self.projects.as_ref(),
                here: self.here.as_deref(),
            },
            layout::screen(self.size),
        );
    }

    /// The line between the session and the sidebar or rail, joined to their header rules.
    fn draw_rule(&self, frame: &mut TuiFrame, rule: Rect, theme: &Theme) {
        let mut line =
            vec![Line::styled(theme.glyphs.rule_v, theme.ui.rule); usize::from(rule.height)];
        // The sidebar's and the rail's header rule meets the line in its second row.
        if let Some(joint) = line.get_mut(1) {
            *joint = Line::styled(theme.glyphs.rule_joint, theme.ui.rule);
        }
        if self.rail {
            let foot = line.len().saturating_sub(2);
            if let Some(joint) = line.get_mut(foot) {
                *joint = Line::styled(theme.glyphs.rule_joint, theme.ui.rule);
            }
        }
        frame.render_widget(Paragraph::new(line), rule);
    }

    /// The open dialog or overlay over the session, or the pane's cursor when there is none.
    fn draw_view(&self, frame: &mut TuiFrame, areas: &Areas, theme: &Theme) {
        match &self.view {
            View::Stop(unfinished) => self.draw_stop(frame, areas.pane, theme, unfinished),
            View::Confirm { question, .. } => {
                let text = vec![
                    Line::styled(question.as_str(), theme.ui.text),
                    Line::from(vec![
                        Span::styled("y", theme.ui.key),
                        Span::styled(" yes, any other key keeps the flow", theme.ui.muted),
                    ]),
                ];
                draw_dialog(
                    frame,
                    areas.pane,
                    theme,
                    " clean up ",
                    theme.state.waiting,
                    text,
                );
            }
            View::Help => draw_help(frame, areas.pane, theme),
            View::Board(_) => {}
            View::History(back) => {
                let hint = Line::from(vec![
                    Span::styled(format!(" ↑ {back} lines back   "), theme.state.waiting),
                    Span::styled("k j", theme.ui.key),
                    Span::styled(" scroll  ", theme.ui.faint),
                    Span::styled("g G", theme.ui.key),
                    Span::styled(" ends  ", theme.ui.faint),
                    Span::styled("q", theme.ui.key),
                    Span::styled(" live ", theme.ui.faint),
                ]);
                let edge = Rect::new(
                    areas.pane.x,
                    areas.pane.y + areas.pane.height.saturating_sub(1),
                    areas.pane.width,
                    1,
                );
                frame.render_widget(Clear, edge);
                frame.render_widget(Paragraph::new(hint).alignment(Alignment::Right), edge);
            }
            View::NewFlow(form) => {
                let repo = form.repo.display().to_string();
                let screen = frame.area();
                frame.buffer_mut().set_style(screen, Modifier::DIM);
                frame.render_widget(
                    NewFlowView {
                        form,
                        flows: self.board.as_ref().map_or(&[], |board| &board.flows),
                        place: &self.tilde(&repo),
                        theme,
                    },
                    areas.session(),
                );
                if let Some(cursor) = form.cursor(areas.session()) {
                    frame.set_cursor_position(cursor);
                }
            }
            View::Panes => {
                if let Some((col, row)) = self.screen.cursor
                    && self
                        .status(&self.focus)
                        .is_none_or(|status| *status == PaneStatus::Running)
                    && self.screen.history.is_none()
                    && col < areas.pane.width
                    && row < areas.pane.height
                {
                    frame.set_cursor_position((areas.pane.x + col, areas.pane.y + row));
                }
            }
        }
    }

    /// Before flow stops: asks to stop, or to keep or end the flows that are not finished.
    fn draw_stop(&self, frame: &mut TuiFrame, pane: Rect, theme: &Theme, unfinished: &[String]) {
        if unfinished.is_empty() {
            let text = vec![
                Line::styled("Stop flow? Every session in it ends.", theme.ui.text),
                Line::from(vec![
                    Span::styled("y", theme.ui.key),
                    Span::styled(" stops it, any other key keeps it", theme.ui.muted),
                ]),
            ];
            draw_dialog(frame, pane, theme, " stop ", theme.ui.rule, text);
            return;
        }
        let sidebar = self.sidebar(theme, &[]);
        let count = unfinished.len();
        let mut text = vec![Line::styled(
            format!(
                "{count} flow{} {} not finished:",
                plural(count),
                if count == 1 { "is" } else { "are" }
            ),
            theme.ui.text,
        )];
        let listed: Vec<&FlowView> = self
            .workers()
            .into_iter()
            .filter(|flow| unfinished.contains(&flow.id))
            .collect();
        // Names padded to the longest, so what each flow does starts in one column.
        let name_width = listed
            .iter()
            .map(|flow| flow.name.chars().count())
            .max()
            .unwrap_or(0);
        for flow in listed {
            let doing = flow
                .blocker
                .clone()
                .or_else(|| flow.delegate.as_ref().map(|text| format!("agent: {text}")))
                .unwrap_or_else(|| sidebar.label(flow).content.to_string());
            text.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(sidebar.glyph(flow.state), theme.state_style(flow.state)),
                Span::styled(format!(" {:name_width$}  ", flow.name), theme.ui.strong),
                Span::styled(clip(&doing, 44), theme.ui.muted),
            ]));
        }
        let (them, they) = if count == 1 {
            ("it", "it resumes")
        } else {
            ("them", "they resume")
        };
        text.push(Line::raw(""));
        text.push(Line::from(vec![
            Span::styled("k", theme.ui.key),
            Span::styled(
                format!(" keep {them}; {they} the next time flow ui starts"),
                theme.ui.muted,
            ),
        ]));
        text.push(Line::from(vec![
            Span::styled("e", theme.ui.key),
            Span::styled(
                format!(" end {them}; branches and worktrees stay until flow clear"),
                theme.ui.muted,
            ),
        ]));
        text.push(Line::from(vec![
            Span::styled("esc", theme.ui.key),
            Span::styled(" keep flow running", theme.ui.muted),
        ]));
        draw_dialog(frame, pane, theme, " stop ", theme.state.waiting, text);
    }

    /// The row above the pane that names the session, on the panel color: what is in view on the
    /// left, a notice or the rail's hint on the right. The left is fitted so the right stays.
    fn draw_title(&self, frame: &mut TuiFrame, area: Rect, theme: &Theme) {
        let right = match &self.notice {
            Some(notice) => Some(Line::styled(format!(" {notice} "), theme.state.waiting)),
            None => self.border_hint(theme),
        };
        let left = self.title_line(theme, area.width, right.as_ref().map_or(0, Line::width));
        frame.render_widget(Block::default().style(theme.ui.bar), area);
        frame.render_widget(Paragraph::new(left), area);
        if let Some(right) = right {
            frame.render_widget(Paragraph::new(right).alignment(Alignment::Right), area);
        }
    }

    /// The session in view: the master and its branch, or a worker's glyph, name, steps, phase and
    /// branch; a worker that needs you leads with a bar in its state's color.
    fn title_line(&self, theme: &Theme, width: u16, right: usize) -> Line<'static> {
        let room = |used: usize| (width as usize).saturating_sub(right + 2 + used);
        let PaneKey::Flow(id) = &self.focus else {
            let mut title = vec![Span::styled(" master", theme.ui.strong)];
            // The sidebar says where the master runs; alone, the title says it.
            let place = match (&self.master, self.areas().side) {
                (Some(place), Some(_)) => place
                    .rsplit_once(" · ")
                    .map(|(_, branch)| branch.to_string()),
                (Some(place), None) => Some(place.clone()),
                (None, _) => None,
            };
            if let Some(place) = place {
                let fitted = clip_place(&place, room(spans_width(&title) + 2));
                if !fitted.is_empty() {
                    title.push(Span::styled(format!("  {fitted}"), theme.ui.faint));
                }
            }
            return Line::from(title);
        };
        let Some(flow) = self.worker(id) else {
            return Line::styled(format!(" {id}"), theme.ui.strong);
        };
        let sidebar = self.sidebar(theme, &[]);
        let lead = if flow.needs_you() {
            Span::styled(theme.glyphs.cursor, theme.state_style(flow.state))
        } else {
            Span::raw(" ")
        };
        let mut title = vec![
            lead,
            Span::styled(sidebar.glyph(flow.state), theme.state_style(flow.state)),
            Span::styled(format!(" {}  ", flow.name), theme.ui.strong),
        ];
        title.extend(sidebar.steps(flow));
        title.push(Span::raw(" "));
        title.push(sidebar.label(flow));
        if let Some(space) = &flow.workspace {
            let place = format!("{} · slot {}", space.branch, space.slot);
            let lead = "  · ";
            let fitted = clip(&place, room(spans_width(&title) + columns(lead)));
            if !fitted.is_empty() {
                title.push(Span::styled(format!("{lead}{fitted}"), theme.ui.faint));
            }
        }
        Line::from(title)
    }

    /// The right of the pane's top edge: with the rail, the next flow that needs you and the way back
    /// to the sidebar; with the sidebar, nothing.
    fn border_hint(&self, theme: &Theme) -> Option<Line<'static>> {
        // With the sidebar in view its key hint says the rest.
        if !self.rail {
            return None;
        }
        if self.workers().is_empty() {
            return None;
        }
        let sidebar = self.sidebar(theme, &[]);
        let mut spans = Vec::new();
        let next = self
            .workers()
            .into_iter()
            .enumerate()
            .find(|(_, flow)| flow.needs_you() && !self.in_view(&flow.id));
        if let Some((index, flow)) = next {
            let what = match flow.state {
                State::Waiting => "is waiting",
                State::NotResponding => "is not responding",
                _ => "is blocked",
            };
            spans.push(Span::styled(
                format!(
                    " {} {} {} {what}",
                    sidebar.glyph(flow.state),
                    index + 1,
                    flow.name
                ),
                theme.state_style(flow.state),
            ));
            spans.push(Span::styled(
                format!("  {}  ", theme.glyphs.separator),
                theme.ui.rule,
            ));
        } else {
            spans.push(Span::raw(" "));
        }
        spans.push(Span::styled("C-space b", theme.ui.key));
        spans.push(Span::styled(" sidebar ", theme.ui.faint));
        Some(Line::from(spans))
    }

    fn draw_bar(&self, frame: &mut TuiFrame, area: Rect, theme: &Theme) {
        let glyphs = &theme.glyphs;
        let mut left = vec![
            Span::raw(" "),
            Span::styled(glyphs.mark, theme.ui.accent),
            Span::styled(" flow ", theme.ui.strong),
            Span::styled(format!(" {} ", glyphs.separator), theme.ui.rule),
            Span::styled("master", theme.ui.text),
        ];
        if let Some(place) = &self.master {
            let room = usize::from(area.width).saturating_sub(spans_width(&left) + 3);
            left.push(Span::styled(
                format!("  {}", clip_place(place, room)),
                theme.ui.faint,
            ));
        }
        match self.status(&PaneKey::Master) {
            Some(PaneStatus::Exited { .. }) => left.push(Span::styled(
                "  exited - C-space r resumes it",
                theme.state.waiting,
            )),
            Some(PaneStatus::Failed { reason }) => {
                left.push(Span::styled(format!("  {reason}"), theme.state.blocked));
            }
            _ => {}
        }
        let keys = vec![
            Span::styled("C-space", theme.ui.key),
            Span::styled(" then  ", theme.ui.faint),
            Span::styled("n", theme.ui.key),
            Span::styled(" new flow   ", theme.ui.muted),
            Span::styled("?", theme.ui.key),
            Span::styled(" keys ", theme.ui.muted),
        ];
        let mut full = vec![
            Span::styled("no workers yet", theme.ui.faint),
            Span::styled(format!("   {}  ", glyphs.separator), theme.ui.rule),
        ];
        full.extend(keys.clone());
        let left = Line::from(left);
        let room = (area.width as usize).saturating_sub(left.width() + 1);
        let right = [Line::from(full), Line::from(keys)]
            .into_iter()
            .find(|line| line.width() <= room);
        frame.render_widget(Block::default().style(theme.ui.bar), area);
        frame.render_widget(Paragraph::new(left), area);
        if let Some(right) = right {
            frame.render_widget(Paragraph::new(right).alignment(Alignment::Right), area);
        }
    }
}
