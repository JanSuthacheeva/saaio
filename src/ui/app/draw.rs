use ratatui::Frame;
use ratatui::layout::{Alignment, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};

use super::{App, View};
use crate::proto::{PaneKey, PaneStatus};
use crate::text::plural;
use crate::ui::board::{BoardState, BoardView};
use crate::ui::dialog::{self, Dialog};
use crate::ui::fit::{clip, spans_width};
use crate::ui::hints::{self, Hint};
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
        self.draw_rule(frame, areas.rule, theme);
        frame.render_widget(
            ScreenView {
                screen: &self.screen,
                text: theme.palette.text,
                background: theme.palette.background,
            },
            areas.pane,
        );
        if self.folded() {
            frame.render_widget(
                Rail {
                    sidebar: self.sidebar(theme, &[]),
                },
                areas.side,
            );
        } else {
            frame.render_widget(self.sidebar(theme, peek), areas.side);
        }
        let asking = matches!(self.view, View::Confirm { .. });
        let notice = match self.status(&self.focus) {
            Some(PaneStatus::Running) => None,
            Some(_) if asking => None,
            Some(status) => {
                dialog::not_running(status, self.ended_worker().as_deref(), self.prefix, theme)
            }
            None if self.focus != PaneKey::Master => Some(dialog::no_session(self.prefix, theme)),
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
        if self.folded() {
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
            View::Help => frame.render_widget(dialog::help(self.prefix, theme), areas.session()),
            View::Board(_) => {}
            View::History(back) => self.draw_history(frame, areas.pane, *back, theme),
            View::NewFlow(form) => {
                let repo = form.repo.display().to_string();
                let view = NewFlowView {
                    form,
                    flows: self.board.as_ref().map_or(&[], |board| &board.flows),
                    place: &self.tilde(&repo),
                    theme,
                };
                let body = view.dialog().body_area(areas.session());
                frame.render_widget(view, areas.session());
                if let Some(cursor) = form.cursor(body) {
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

    /// The scrollback hint along the pane's last row, fitted by hints::line.
    fn draw_history(&self, frame: &mut Frame, pane: Rect, back: u32, theme: &Theme) {
        let mut spans = vec![Span::styled(
            format!("↑ {back} lines back"),
            theme.state.waiting,
        )];
        let keys = [
            Hint::new("k j", "scroll"),
            Hint::new("g G", "ends"),
            Hint::new("q", "live").kept(),
        ];
        let room = usize::from(pane.width).saturating_sub(spans_width(&spans) + 5);
        let keys = hints::line(&keys, room, theme);
        if keys.width() > 0 {
            spans.push(Span::raw("   "));
            spans.extend(keys.spans);
        }
        spans.push(Span::raw(" "));
        let edge = Rect::new(
            pane.x,
            pane.y + pane.height.saturating_sub(1),
            pane.width,
            1,
        );
        frame.render_widget(Clear, edge);
        frame.render_widget(
            Paragraph::new(Line::from(spans)).alignment(Alignment::Right),
            edge,
        );
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
