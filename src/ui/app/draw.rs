use ratatui::Frame;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};

use super::{App, View};
use crate::proto::{PaneKey, PaneStatus};
use crate::text::plural;
use crate::ui::board::{BoardState, BoardView};
use crate::ui::dialog::{self, Dialog};
use crate::ui::fit::clip;
use crate::ui::hints::Hint;
use crate::ui::layout::{self, Areas};
use crate::ui::newflow::NewFlowView;
use crate::ui::peek::PeekLine;
use crate::ui::rail::Rail;
use crate::ui::screen::ScreenView;
use crate::ui::theme::Theme;
use crate::view::FlowView;

impl App {
    pub fn draw(&self, frame: &mut Frame, theme: &Theme, peek: &[PeekLine]) {
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
        let notice = match self.status(&self.focus) {
            Some(PaneStatus::Running) => None,
            Some(_) if asking => None,
            Some(status) => dialog::not_running(status, self.ended_worker().as_deref(), theme),
            None if self.focus != PaneKey::Master => Some(dialog::no_session(theme)),
            None => None,
        };
        if let Some(notice) = notice {
            frame.render_widget(notice, areas.session());
        }
        self.draw_view(frame, &areas, theme);
    }

    fn draw_board(&self, frame: &mut Frame, state: BoardState, theme: &Theme) {
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
    fn draw_rule(&self, frame: &mut Frame, rule: Rect, theme: &Theme) {
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
    fn draw_view(&self, frame: &mut Frame, areas: &Areas, theme: &Theme) {
        match &self.view {
            View::Stop(unfinished) => {
                frame.render_widget(self.stop_dialog(theme, unfinished), areas.session());
            }
            View::Confirm { question, .. } => frame.render_widget(
                Dialog {
                    border: theme.state.waiting,
                    keys: vec![
                        Hint::new("y", "yes"),
                        Hint::new("any other key", "keeps the flow").kept(),
                    ],
                    ..Dialog::new(
                        "clean up",
                        vec![Line::styled(question.as_str(), theme.ui.text)],
                        theme,
                    )
                },
                areas.session(),
            ),
            View::Help => frame.render_widget(dialog::help(theme), areas.session()),
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

    /// The stop question, or the flows it would leave unfinished with keep, end and esc as keys.
    fn stop_dialog<'a>(&'a self, theme: &'a Theme, unfinished: &[String]) -> Dialog<'a> {
        if unfinished.is_empty() {
            return Dialog {
                keys: vec![
                    Hint::new("y", "stops it"),
                    Hint::new("any other key", "keeps it").kept(),
                ],
                ..Dialog::new(
                    "stop",
                    vec![Line::styled(
                        "Stop flow? Every session in it ends.",
                        theme.ui.text,
                    )],
                    theme,
                )
            };
        }
        let sidebar = self.sidebar(theme, &[]);
        let count = unfinished.len();
        let mut body = vec![Line::styled(
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
            body.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(sidebar.glyph(flow.state), theme.flow_style(flow)),
                Span::styled(format!(" {:name_width$}  ", flow.name), theme.ui.strong),
                Span::styled(clip(&doing, 44), theme.ui.muted),
            ]));
        }
        let (them, they) = if count == 1 {
            ("it", "it resumes")
        } else {
            ("them", "they resume")
        };
        body.push(Line::raw(""));
        body.push(Line::styled(
            format!("Kept, {they} the next time flow ui starts."),
            theme.ui.muted,
        ));
        body.push(Line::styled(
            "Ended, branches and worktrees stay until flow clear.",
            theme.ui.muted,
        ));
        Dialog {
            border: theme.state.waiting,
            keys: vec![
                Hint::new("k", format!("keep {them}")),
                Hint::new("e", format!("end {them}")),
                Hint::new("esc", "keep flow running").kept(),
            ],
            ..Dialog::new("stop", body, theme)
        }
    }
}
