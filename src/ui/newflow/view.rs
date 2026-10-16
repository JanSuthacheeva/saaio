use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, Widget};

use super::{Field, NewFlowForm, dialog_area};
use crate::branching;
use crate::event::Mode;
use crate::launch::{name, worktree};
use crate::slots;
use crate::ui::fit::{clip, clip_place, spans_width, split};
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
    fn render(self, area: Rect, buf: &mut Buffer) {
        let form = self.form;
        let dialog = dialog_area(area, form.extra_rows());
        Clear.render(dialog, buf);
        let frame = Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(self.theme.ui.muted)
            .style(self.theme.ui.background);
        let inner = frame.inner(dialog);
        frame.render(dialog, buf);
        let width = usize::from(inner.width);
        let extra = form.extra_rows();
        let mut lines = vec![
            (0, self.header(width)),
            (2, Line::styled(self.label(), self.theme.ui.faint)),
            (7 + extra, self.mode_row(width)),
            (8 + extra, self.creates(width)),
            (
                10 + extra,
                Line::styled("─".repeat(width), self.theme.ui.rule),
            ),
            (11 + extra, self.keys()),
        ];
        if let Some(error) = &form.error {
            let text = format!(" {}", clip(error, width.saturating_sub(2)));
            lines.push((6, Line::styled(text, self.theme.state.blocked)));
        }
        if extra > 0 {
            lines.push((7, self.type_row()));
        }
        // A terminal too short for the dialog gets the rows that fit.
        for (row, line) in lines {
            if row < inner.height {
                buf.set_line(inner.x, inner.y + row, &line, inner.width);
            }
        }
        self.render_text(inner, dialog, buf);
    }
}

impl NewFlowView<'_> {
    /// new flow, and where it starts; new session, whose folder is in the box.
    fn header(&self, width: usize) -> Line<'static> {
        if self.form.mode == Mode::Plain {
            return Line::styled(" new session", self.theme.ui.strong);
        }
        let place = clip_place(self.place, width.saturating_sub(15));
        split(
            vec![Span::styled(" new flow", self.theme.ui.strong)],
            vec![Span::styled(format!("in {place} "), self.theme.ui.faint)],
            width,
        )
    }

    fn label(&self) -> &'static str {
        if self.form.mode == Mode::Plain {
            " folder"
        } else {
            " request"
        }
    }

    /// The text box and the end of the text that fits it.
    fn render_text(&self, inner: Rect, dialog: Rect, buf: &mut Buffer) {
        let field = Rect::new(inner.x + 1, inner.y + 3, inner.width.saturating_sub(2), 3)
            .intersection(inner);
        if field.is_empty() {
            return;
        }
        let border = if self.form.field == Field::Request {
            self.theme.ui.accent
        } else {
            self.theme.ui.rule
        };
        let input = Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(border);
        let text = input.inner(field);
        input.render(field, buf);
        if !text.is_empty() {
            buf.set_line(
                text.x + 1,
                text.y,
                &Line::styled(self.form.shown_text(dialog), self.theme.ui.text),
                text.width.saturating_sub(1),
            );
        }
    }

    fn radio(&self, on: bool, label: &str) -> [Span<'static>; 2] {
        let theme = self.theme;
        [
            Span::styled(
                if on { "(•)" } else { "( )" },
                if on { theme.ui.accent } else { theme.ui.faint },
            ),
            Span::styled(format!(" {label}"), theme.ui.text),
        ]
    }

    /// A row's label, lit while its field has the keys.
    fn heading(&self, field: Field, text: &'static str) -> Span<'static> {
        let style = if self.form.field == field {
            self.theme.ui.key
        } else {
            self.theme.ui.faint
        };
        Span::styled(text, style)
    }

    fn type_row(&self) -> Line<'static> {
        let mut row = vec![self.heading(Field::Type, " type    ")];
        for (index, kind) in self.form.types().iter().enumerate() {
            if index > 0 {
                row.push(Span::raw("   "));
            }
            row.extend(self.radio(index == self.form.kind, kind));
        }
        Line::from(row)
    }

    /// auto, guided, plain, and what the picked one means.
    fn mode_row(&self, width: usize) -> Line<'static> {
        let mut row = vec![self.heading(Field::Mode, " mode    ")];
        for (index, mode) in Mode::ALL.into_iter().enumerate() {
            if index > 0 {
                row.push(Span::raw("   "));
            }
            row.extend(self.radio(mode == self.form.mode, mode.name()));
        }
        let hint = match self.form.mode {
            Mode::Auto => "auto runs on its own ",
            Mode::Guided => "guided stops at every phase ",
            Mode::Plain => "plain is just the agent ",
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
                " opens an agent there, no worktree, branch or slot",
                theme.ui.faint,
            );
        }
        if form.mode == Mode::Plain {
            let rest = "  ·  no worktree, branch or slot";
            let folder = clip_place(form.folder.trim(), width.saturating_sub(10 + rest.len()));
            return Line::from(vec![
                Span::styled(" opens in ", theme.ui.faint),
                Span::styled(folder, theme.ui.text),
                Span::styled(rest, theme.ui.faint),
            ]);
        }
        if form.request.trim().is_empty() {
            return Line::styled(" creates a branch, a worktree and a slot", theme.ui.faint);
        }
        let taken: Vec<String> = self
            .flows
            .iter()
            .filter(|flow| !flow.is_finished())
            .map(|flow| flow.name.clone())
            .collect();
        let name = name::flow_name(None, &form.request, &taken);
        let branch = branching::branch_name(
            form.branching.as_ref(),
            form.picked_type(),
            &form.request,
            &name,
        );
        let mut spans = vec![
            Span::styled(" creates ", theme.ui.faint),
            Span::styled(branch, theme.ui.text),
        ];
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

    fn keys(&self) -> Line<'static> {
        let theme = self.theme;
        Line::from(vec![
            Span::styled(" enter", theme.ui.key),
            Span::styled(" start   ", theme.ui.faint),
            Span::styled("tab", theme.ui.key),
            Span::styled(" next field   ", theme.ui.faint),
            Span::styled("esc", theme.ui.key),
            Span::styled(" cancel", theme.ui.faint),
        ])
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn draws_an_error_in_a_terminal_too_small_for_it() {
        let mut form = NewFlowForm::new(PathBuf::from("/shop"), "~/shop".into());
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
