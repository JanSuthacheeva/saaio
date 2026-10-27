//! `NewFlowView`: the new-flow dialog drawn over the session

use ratatui::buffer::Buffer;
use ratatui::layout::Margin;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Widget};

use super::{BODY_ROWS, BODY_WIDTH, Field, NewFlowForm};
use crate::branching;
use crate::event::Mode;
use crate::launch::{name, worktree};
use crate::slots;
use crate::ui::dialog::{Dialog, PAD_COLS};
use crate::ui::fit::{clip, clip_place, spans_width, split};
use crate::ui::focus;
use crate::ui::hints::Hint;
use crate::ui::theme::Theme;
use crate::view::FlowView;

/// The dialog, centered in `area`: request (or folder), mode and what the start creates.
#[derive(Debug)]
pub struct NewFlowView<'a> {
    pub form: &'a NewFlowForm,
    pub flows: &'a [FlowView],
    /// Where the flow starts, as shown: ~/projects/shop.
    pub place: &'a str,
    pub theme: &'a Theme,
}

impl Widget for NewFlowView<'_> {
    /// A Dialog (title "new flow" or "new session", aside "in ~/shop", keys enter, tab, esc)
    /// with its fields drawn into the body; the focused field gets focus::mark.
    fn render(self, area: Rect, buf: &mut Buffer) {
        let form = self.form;
        let dialog = self.dialog();
        let body = dialog.body_area(area);
        dialog.render(area, buf);
        let width = usize::from(body.width);
        let extra = form.extra_rows();
        let mut lines = vec![
            (0, Line::styled(self.label(), self.theme.ui.faint)),
            (5 + extra, self.mode_row(width)),
            (6 + extra, self.creates(width)),
        ];
        if let Some(error) = &form.error {
            lines.push((
                4,
                Line::styled(clip(error, width), self.theme.state.blocked),
            ));
        }
        if form.has_type_row() {
            lines.push((5, self.type_row()));
        }
        if form.has_agent_row() {
            lines.push((5 + u16::from(form.has_type_row()), self.agent_row()));
        }
        if form.has_base_row() {
            lines.push((form.base_row(), self.base_row(body)));
        }
        // A terminal too short for the dialog gets the rows that fit.
        for (row, line) in lines {
            if row < body.height {
                buf.set_line(body.x, body.y + row, &line, body.width);
            }
        }
        self.render_text(body, buf);
        let (top, rows) = match form.field {
            Field::Request => (0, 4),
            Field::Type => (5, 1),
            Field::Agent => (5 + u16::from(form.has_type_row()), 1),
            Field::Base => (form.base_row(), 1),
            Field::Mode => (5 + extra, 1),
        };
        // From the border to the border, so the bar stands where the sidebar's does.
        let focused = Rect::new(
            body.x.saturating_sub(PAD_COLS),
            body.y + top,
            body.width + 2 * PAD_COLS,
            rows,
        )
        .intersection(body.outer(Margin::new(PAD_COLS, 0)));
        focus::mark(buf, focused, self.theme);
    }
}

impl NewFlowView<'_> {
    /// The NewFlowForm as a Dialog, so the view and the cursor agree on where the body is.
    pub fn dialog(&self) -> Dialog<'_> {
        let theme = self.theme;
        let (title, aside) = if self.form.mode == Mode::Plain {
            ("new session", None)
        } else {
            (
                "new flow",
                Some(format!("in {}", clip_place(self.place, 40))),
            )
        };
        Dialog {
            aside,
            body_rows: BODY_ROWS + self.form.extra_rows(),
            body_width: BODY_WIDTH,
            keys: vec![
                Hint::new("enter", "start"),
                Hint::new("tab", "next field"),
                Hint::new("esc", "cancel").kept(),
            ],
            ..Dialog::new(title, Vec::new(), theme)
        }
    }

    fn label(&self) -> &'static str {
        if self.form.mode == Mode::Plain {
            "folder"
        } else {
            "request"
        }
    }

    /// The text box and the end of the text that fits it.
    fn render_text(&self, body: Rect, buf: &mut Buffer) {
        let field = Rect::new(body.x, body.y + 1, body.width, 3).intersection(body);
        if field.is_empty() {
            return;
        }
        let input = Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(self.theme.ui.rule);
        let text = input.inner(field);
        input.render(field, buf);
        if !text.is_empty() {
            buf.set_line(
                text.x + 1,
                text.y,
                &Line::styled(self.form.shown_text(body), self.theme.ui.text),
                text.width.saturating_sub(1),
            );
        }
    }

    /// "(•)" in ui.strong; in the focused row the chosen option is reversed.
    fn radio(&self, field: Field, on: bool, label: &str) -> Vec<Span<'static>> {
        let theme = self.theme;
        let mut spans = if on {
            vec![
                Span::styled("(•)", theme.ui.strong),
                Span::styled(format!(" {label}"), theme.ui.text),
            ]
        } else {
            vec![
                Span::styled("( )", theme.ui.faint),
                Span::styled(format!(" {label}"), theme.ui.text),
            ]
        };
        if on && self.form.field == field {
            for span in &mut spans {
                span.style = span.style.add_modifier(Modifier::REVERSED);
            }
        }
        spans
    }

    fn heading(&self, text: &'static str) -> Span<'static> {
        Span::styled(text, self.theme.ui.faint)
    }

    fn type_row(&self) -> Line<'static> {
        let mut row = vec![self.heading("type    ")];
        for (index, kind) in self.form.types().iter().enumerate() {
            if index > 0 {
                row.push(Span::raw("   "));
            }
            row.extend(self.radio(Field::Type, index == self.form.kind, kind));
        }
        Line::from(row)
    }

    fn agent_row(&self) -> Line<'static> {
        let mut row = vec![self.heading("agent   ")];
        for (index, agent) in self.form.agents.iter().enumerate() {
            if index > 0 {
                row.push(Span::raw("   "));
            }
            row.extend(self.radio(Field::Agent, index == self.form.agent, agent));
        }
        Line::from(row)
    }

    /// The base typed or picked, else what the repo has checked out, faint; with the keys, how up
    /// and down pick a flow's branch.
    fn base_row(&self, body: Rect) -> Line<'static> {
        let theme = self.theme;
        let form = self.form;
        let mut row = vec![self.heading("base    ")];
        if form.base.is_empty() {
            let checked_out = form.checked_out.as_ref().map_or_else(
                || "what the repo has checked out".to_string(),
                |branch| format!("{branch} (checked out)"),
            );
            row.push(Span::styled(checked_out, theme.ui.faint));
        } else {
            row.push(Span::styled(
                form.shown_base(body).to_string(),
                theme.ui.text,
            ));
        }
        if form.field != Field::Base || form.bases.is_empty() {
            return Line::from(row);
        }
        let width = usize::from(body.width);
        let hint = vec![Span::styled("↑↓ a flow's branch", theme.ui.faint)];
        if spans_width(&row) + spans_width(&hint) + 2 > width {
            return Line::from(row);
        }
        split(row, hint, width)
    }

    /// auto, guided, plain, and what the picked one means.
    fn mode_row(&self, width: usize) -> Line<'static> {
        let mut row = vec![self.heading("mode    ")];
        for (index, mode) in Mode::ALL.into_iter().enumerate() {
            if index > 0 {
                row.push(Span::raw("   "));
            }
            row.extend(self.radio(Field::Mode, mode == self.form.mode, mode.name()));
        }
        let hint = match self.form.mode {
            Mode::Auto => "auto runs on its own",
            Mode::Guided => "guided stops at every phase",
            Mode::Plain => "plain is just the agent",
        };
        split(row, vec![Span::styled(hint, self.theme.ui.faint)], width)
    }

    /// The branch, worktree and slot the start creates, as far as the width allows; for a plain
    /// session where it opens.
    fn creates(&self, width: usize) -> Line<'static> {
        let theme = self.theme;
        let form = self.form;
        if form.mode == Mode::Plain && form.folder.trim().is_empty() {
            return Line::styled(
                "opens an agent there, no worktree, branch or slot",
                theme.ui.faint,
            );
        }
        if form.mode == Mode::Plain {
            let rest = "  ·  no worktree, branch or slot";
            let folder = clip_place(form.folder.trim(), width.saturating_sub(9 + rest.len()));
            return Line::from(vec![
                Span::styled("opens in ", theme.ui.faint),
                Span::styled(folder, theme.ui.text),
                Span::styled(rest, theme.ui.faint),
            ]);
        }
        if form.request.trim().is_empty() {
            return Line::styled("creates a branch, a worktree and a slot", theme.ui.faint);
        }
        let taken: Vec<String> = self
            .flows
            .iter()
            .filter(|flow| !flow.is_finished())
            .map(|flow| flow.name.clone())
            .collect();
        let name = name::choose(None, &form.request, &taken);
        let branch = branching::branch_name(
            form.branching.as_ref(),
            form.picked_type(),
            &form.request,
            &name,
        );
        let mut spans = vec![
            Span::styled("creates ", theme.ui.faint),
            Span::styled(branch, theme.ui.text),
        ];
        if let Some(base) = form.picked_base() {
            spans.push(Span::styled(" from ", theme.ui.faint));
            spans.push(Span::styled(base.to_string(), theme.ui.text));
        }
        let worktree = format!("{}/{name}", worktree::WORKTREES_DIR);
        if spans_width(&spans) + worktree.len() + 20 <= width {
            spans.push(Span::styled("  ·  ", theme.ui.faint));
            spans.push(Span::styled(worktree, theme.ui.text));
        }
        match slots::free_slot(self.flows) {
            Some(slot) => {
                spans.push(Span::styled("  ·  ", theme.ui.faint));
                spans.push(Span::styled(format!("slot {slot}"), theme.ui.text));
            }
            None => spans.push(Span::styled(
                "  ·  every slot is taken",
                theme.state.blocked,
            )),
        }
        Line::from(spans)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn reverses_the_chosen_option_of_the_row_with_the_keys() {
        let mut form = NewFlowForm::new(PathBuf::from("/shop"), "~/shop".into(), Vec::new());
        let theme = Theme::default();
        let view = |form: &NewFlowForm| {
            NewFlowView {
                form,
                flows: &[],
                place: "~/shop",
                theme: &theme,
            }
            .radio(Field::Mode, true, "guided")
        };

        assert!(
            !view(&form)[0]
                .style
                .add_modifier
                .contains(Modifier::REVERSED)
        );
        form.on_key(crate::ui::input::Key::Tab);
        form.on_key(crate::ui::input::Key::Tab);
        let chosen = view(&form);
        assert!(chosen[0].style.add_modifier.contains(Modifier::REVERSED));
        assert_eq!(chosen[0].style.fg, theme.ui.strong.fg);
    }

    #[test]
    fn draws_an_error_in_a_terminal_too_small_for_it() {
        let mut form = NewFlowForm::new(PathBuf::from("/shop"), "~/shop".into(), Vec::new());
        form.error = Some("the branch exists".into());
        let theme = Theme::default();
        for (width, height) in (0..6)
            .map(|width| (width, 20))
            .chain((0..14).map(|height| (80, height)))
        {
            let area = Rect::new(0, 0, width, height);
            let mut buf = Buffer::empty(area);

            NewFlowView {
                form: &form,
                flows: &[],
                place: "~/shop",
                theme: &theme,
            }
            .render(area, &mut buf);
        }
    }
}
