use std::path::PathBuf;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, Widget};

use crate::branching::{self, Branching};
use crate::event::Mode;
use crate::launch;
use crate::ui::input::Key;
use crate::ui::sidebar::{clip, clip_place, columns, split, tail};
use crate::ui::theme::Theme;
use crate::view::FlowView;

/// The dialog's width, borders included.
const WIDTH: u16 = 78;
/// The dialog's height, borders included.
const HEIGHT: u16 = 14;

/// What `C-space n` asks before it runs `flow start`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewFlowForm {
    pub request: String,
    pub mode: Mode,
    /// The repository the flow starts in: the focused pane's.
    pub repo: PathBuf,
    /// Why the last start failed, shown under the request.
    pub error: Option<String>,
    /// The repo's branch convention, once read; its types make a type row.
    pub branching: Option<Branching>,
    /// The picked type, an index into the convention's types.
    pub kind: usize,
    field: Field,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
    Request,
    Type,
    Mode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormAction {
    Edit,
    Submit,
    Cancel,
}

impl NewFlowForm {
    /// Prefilled with the focused pane's repo.
    pub fn new(repo: PathBuf) -> NewFlowForm {
        NewFlowForm {
            request: String::new(),
            mode: Mode::Guided,
            repo,
            error: None,
            branching: None,
            kind: 0,
            field: Field::Request,
        }
    }

    /// The repo's convention arrived (LoadBranching); the type starts at its first.
    pub fn set_branching(&mut self, branching: Option<Branching>) {
        self.branching = branching;
        self.kind = 0;
    }

    /// The types the repo lists; empty without a convention.
    pub fn types(&self) -> &[String] {
        self.branching.as_ref().map_or(&[], |rules| &rules.types)
    }

    /// The picked branch type, when the repo has types.
    pub fn picked_type(&self) -> Option<&str> {
        self.types().get(self.kind).map(String::as_str)
    }

    /// Rows the type row adds to the dialog.
    fn extra_rows(&self) -> u16 {
        u16::from(!self.types().is_empty())
    }

    pub fn on_key(&mut self, key: Key) -> FormAction {
        match (self.field, key) {
            (_, Key::Esc) => return FormAction::Cancel,
            (_, Key::Enter) if !self.request.trim().is_empty() => return FormAction::Submit,
            (_, Key::Enter) => {}
            (_, Key::Tab) => {
                self.field = match self.field {
                    Field::Request if !self.types().is_empty() => Field::Type,
                    Field::Request | Field::Type => Field::Mode,
                    Field::Mode => Field::Request,
                };
            }
            (Field::Request, Key::Char(c)) => self.request.push(c),
            (Field::Request, Key::Backspace) => {
                self.request.pop();
            }
            (Field::Request, Key::Ctrl('u')) => self.request.clear(),
            (Field::Request, Key::Ctrl('w')) => {
                let kept = self.request.trim_end().len();
                let start = self.request[..kept].rfind(' ').map_or(0, |at| at + 1);
                self.request.truncate(start);
            }
            (Field::Type, Key::Right | Key::Char(' ' | 'l')) => {
                self.kind = (self.kind + 1) % self.types().len().max(1);
            }
            (Field::Type, Key::Left | Key::Char('h')) => {
                let count = self.types().len().max(1);
                self.kind = (self.kind + count - 1) % count;
            }
            (Field::Mode, Key::Left | Key::Right | Key::Char(' ' | 'h' | 'l')) => {
                self.mode = match self.mode {
                    Mode::Auto => Mode::Guided,
                    Mode::Guided => Mode::Auto,
                };
            }
            _ => {}
        }
        FormAction::Edit
    }

    /// The column of the text cursor inside the request box, while the request has the keys.
    pub fn cursor(&self, area: Rect) -> Option<(u16, u16)> {
        if self.field != Field::Request {
            return None;
        }
        let dialog = dialog_area(area, self.extra_rows());
        let typed = u16::try_from(columns(self.shown_request(dialog))).unwrap_or(u16::MAX);
        Some((dialog.x + 3 + typed, dialog.y + 5))
    }

    /// The end of the request that fits the box.
    fn shown_request(&self, dialog: Rect) -> &str {
        tail(&self.request, usize::from(dialog.width.saturating_sub(7)))
    }
}

/// The dialog, centered in `area`: request, mode and what the start creates.
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
        let theme = self.theme;
        let form = self.form;
        let dialog = dialog_area(area, form.extra_rows());
        Clear.render(dialog, buf);
        let frame = Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(theme.ui.muted)
            .style(theme.ui.background);
        let inner = frame.inner(dialog);
        frame.render(dialog, buf);
        let width = inner.width as usize;
        let line = |buf: &mut Buffer, row: u16, line: Line| {
            buf.set_line(inner.x, inner.y + row, &line, inner.width);
        };

        line(
            buf,
            0,
            split(
                vec![Span::styled(" new flow", theme.ui.strong)],
                vec![Span::styled(
                    format!("in {} ", clip_place(self.place, width.saturating_sub(15))),
                    theme.ui.faint,
                )],
                width,
            ),
        );
        line(buf, 2, Line::styled(" request", theme.ui.faint));
        let field = Rect::new(inner.x + 1, inner.y + 3, inner.width.saturating_sub(2), 3);
        let focused = if form.field == Field::Request {
            theme.ui.accent
        } else {
            theme.ui.rule
        };
        let input = Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(focused);
        let text = input.inner(field);
        input.render(field, buf);
        buf.set_line(
            text.x + 1,
            text.y,
            &Line::styled(form.shown_request(dialog), theme.ui.text),
            text.width.saturating_sub(1),
        );
        if let Some(error) = &form.error {
            line(
                buf,
                6,
                Line::styled(
                    format!(" {}", clip(error, width.saturating_sub(2))),
                    theme.state.blocked,
                ),
            );
        }

        let radio = |mode: Mode, label: &'static str| {
            let on = form.mode == mode;
            vec![
                Span::styled(
                    if on { "(•)" } else { "( )" },
                    if on { theme.ui.accent } else { theme.ui.faint },
                ),
                Span::styled(format!(" {label}"), theme.ui.text),
            ]
        };
        let extra = form.extra_rows();
        if extra > 0 {
            let mut row = vec![Span::styled(
                " type    ",
                if form.field == Field::Type {
                    theme.ui.key
                } else {
                    theme.ui.faint
                },
            )];
            for (index, kind) in form.types().iter().enumerate() {
                if index > 0 {
                    row.push(Span::raw("   "));
                }
                let on = index == form.kind;
                row.push(Span::styled(
                    if on { "(•)" } else { "( )" },
                    if on { theme.ui.accent } else { theme.ui.faint },
                ));
                row.push(Span::styled(format!(" {kind}"), theme.ui.text));
            }
            line(buf, 7, Line::from(row));
        }
        let mut mode = vec![Span::styled(" mode    ", theme.ui.faint)];
        if form.field == Field::Mode {
            mode[0] = Span::styled(" mode    ", theme.ui.key);
        }
        mode.extend(radio(Mode::Auto, "auto"));
        mode.push(Span::raw("     "));
        mode.extend(radio(Mode::Guided, "guided"));
        mode.push(Span::styled(
            "       guided stops at every phase",
            theme.ui.faint,
        ));
        line(buf, 7 + extra, Line::from(mode));

        let taken: Vec<String> = self
            .flows
            .iter()
            .filter(|flow| !flow.is_finished())
            .map(|flow| flow.name.clone())
            .collect();
        let creates = if form.request.trim().is_empty() {
            vec![Span::styled(
                " creates a branch, a worktree and a slot",
                theme.ui.faint,
            )]
        } else {
            let name = launch::flow_name(None, &form.request, &taken);
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
            let worktree = format!("{}/{name}", launch::WORKTREES_DIR);
            if Line::from(spans.clone()).width() + worktree.len() + 20 <= width {
                spans.push(Span::styled("  ·  ", theme.ui.faint));
                spans.push(Span::styled(worktree, theme.ui.text));
            }
            match launch::free_slot(self.flows) {
                Some(slot) => {
                    spans.push(Span::styled("  ·  ", theme.ui.faint));
                    spans.push(Span::styled(format!("slot {slot}"), theme.ui.text));
                }
                None => spans.push(Span::styled(
                    "  ·  every slot is taken",
                    theme.state.blocked,
                )),
            }
            spans
        };
        line(buf, 8 + extra, Line::from(creates));

        line(
            buf,
            10 + extra,
            Line::styled("─".repeat(width), theme.ui.rule),
        );
        line(
            buf,
            11 + extra,
            Line::from(vec![
                Span::styled(" enter", theme.ui.key),
                Span::styled(" start   ", theme.ui.faint),
                Span::styled("tab", theme.ui.key),
                Span::styled(" next field   ", theme.ui.faint),
                Span::styled("esc", theme.ui.key),
                Span::styled(" cancel", theme.ui.faint),
            ]),
        );
    }
}

/// Centered on the screen, a little above the middle as in the design.
fn dialog_area(area: Rect, extra_rows: u16) -> Rect {
    let width = WIDTH.min(area.width);
    let height = (HEIGHT + extra_rows).min(area.height);
    Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 3,
        width,
        height,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn typed(form: &mut NewFlowForm, text: &str) {
        for c in text.chars() {
            form.on_key(Key::Char(c));
        }
    }

    #[test]
    fn edits_the_request_and_submits_only_with_one() {
        let mut form = NewFlowForm::new(PathBuf::from("/shop"));

        assert_eq!(
            form.on_key(Key::Enter),
            FormAction::Edit,
            "nothing to start"
        );
        typed(&mut form, "SHOP-240 rate limit expor");
        form.on_key(Key::Backspace);
        typed(&mut form, "rt");
        assert_eq!(form.request, "SHOP-240 rate limit export");
        form.on_key(Key::Ctrl('w'));
        assert_eq!(form.request, "SHOP-240 rate limit ");
        form.on_key(Key::Ctrl('u'));
        assert_eq!(form.request, "");
        typed(&mut form, "fix totals");

        assert_eq!(form.on_key(Key::Enter), FormAction::Submit);
        assert_eq!(form.on_key(Key::Esc), FormAction::Cancel);
    }

    #[test]
    fn picks_a_branch_type_when_the_repo_has_types() {
        let mut form = NewFlowForm::new(PathBuf::from("/shop"));
        form.on_key(Key::Tab);
        form.on_key(Key::Right);
        assert_eq!(form.mode, Mode::Auto, "without types tab goes to the mode");

        form.set_branching(Some(Branching {
            name: "{type}/{ticket}-{slug}".into(),
            types: vec!["feature".into(), "bugfix".into(), "hotfix".into()],
        }));
        assert_eq!(form.picked_type(), Some("feature"));
        form.on_key(Key::Tab);
        form.on_key(Key::Tab);
        form.on_key(Key::Left);
        assert_eq!(form.picked_type(), Some("hotfix"), "left wraps around");
        form.on_key(Key::Char('l'));
        form.on_key(Key::Char('l'));
        assert_eq!(form.picked_type(), Some("bugfix"));
        assert_eq!(form.request, "", "the type row takes the keys");
    }

    #[test]
    fn keeps_wide_characters_inside_the_request_box() {
        let mut form = NewFlowForm::new(PathBuf::from("/shop"));
        typed(&mut form, &"界".repeat(60));
        let dialog = dialog_area(Rect::new(0, 0, 100, 30), form.extra_rows());

        let shown = form.shown_request(dialog);

        assert_eq!(columns(shown), 70);
    }

    #[test]
    fn draws_an_error_in_a_terminal_too_narrow_for_it() {
        let mut form = NewFlowForm::new(PathBuf::from("/shop"));
        form.error = Some("the branch exists".into());
        let theme = Theme::default();
        for width in 0..6 {
            let area = Rect::new(0, 0, width, 20);
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

    #[test]
    fn switches_the_mode_from_its_field_only() {
        let mut form = NewFlowForm::new(PathBuf::from("/shop"));

        form.on_key(Key::Char('l'));
        assert_eq!(form.mode, Mode::Guided, "l is typed into the request");
        form.on_key(Key::Tab);
        form.on_key(Key::Right);
        assert_eq!(form.mode, Mode::Auto);
        form.on_key(Key::Char(' '));
        assert_eq!(form.mode, Mode::Guided);
        assert_eq!(form.request, "l");
    }
}
