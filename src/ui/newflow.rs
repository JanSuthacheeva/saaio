//! `NewFlowForm`: the new-flow dialog's form and keys

mod view;

use std::path::PathBuf;

use ratatui::layout::Rect;

pub use view::NewFlowView;

use crate::branching::Branching;
use crate::event::Mode;
use crate::ui::fit::{columns, tail};
use crate::ui::input::Key;

/// The dialog body's width, inside its padding.
const BODY_WIDTH: u16 = 72;
/// The body's rows without the type, agent and base rows: label, text box, error, mode, creates.
const BODY_ROWS: u16 = 7;

/// The column the base starts at in its row, after its heading.
const BASE_AT: u16 = 8;

/// What the prefix and `n` ask before it runs `saaio start`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewFlowForm {
    pub request: String,
    /// The folder a plain session opens in; prefilled with the place in view (~/projects/shop).
    pub folder: String,
    pub mode: Mode,
    /// The repository the flow starts in: the focused pane's.
    pub repo: PathBuf,
    /// Why the last start failed, shown under the request.
    pub error: Option<String>,
    /// The repo's branch convention, once read; its types make a type row.
    pub branching: Option<Branching>,
    /// The picked type, an index into the convention's types.
    pub kind: usize,
    /// The agents a flow can run, the default first; two or more make an agent row.
    pub agents: Vec<String>,
    /// The picked agent, an index into agents.
    pub agent: usize,
    /// The base typed or picked; empty starts from what the repo has checked out.
    pub base: String,
    /// What the repo has checked out, shown faint in an empty base row.
    pub checked_out: Option<String>,
    /// The branches of the repo's unfinished flows, which up and down put into the base row.
    pub bases: Vec<String>,
    field: Field,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
    Request,
    Type,
    Agent,
    Base,
    Mode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormAction {
    Edit,
    Submit,
    Cancel,
}

impl NewFlowForm {
    /// Prefilled with the focused pane's repo, `place` as a plain session's folder, and the
    /// default agent picked.
    pub fn new(repo: PathBuf, place: String, agents: Vec<String>) -> NewFlowForm {
        NewFlowForm {
            request: String::new(),
            folder: place,
            mode: Mode::Guided,
            repo,
            error: None,
            branching: None,
            kind: 0,
            agents,
            agent: 0,
            base: String::new(),
            checked_out: None,
            bases: Vec::new(),
            field: Field::Request,
        }
    }

    /// The picked agent when it is not the default.
    pub fn picked_agent(&self) -> Option<&str> {
        (self.agent > 0)
            .then(|| self.agents.get(self.agent))
            .flatten()
            .map(String::as_str)
    }

    /// The repo's convention and what it has checked out arrived (LoadBranching); the type starts
    /// at its first.
    pub fn set_branching(&mut self, branching: Option<Branching>, checked_out: Option<String>) {
        self.branching = branching;
        self.kind = 0;
        self.checked_out = checked_out;
    }

    /// The base a flow starts from, when one is typed or picked.
    pub fn picked_base(&self) -> Option<&str> {
        let base = self.base.trim();
        (self.has_base_row() && !base.is_empty()).then_some(base)
    }

    /// The types the repo lists; empty without a convention.
    pub fn types(&self) -> &[String] {
        self.branching.as_ref().map_or(&[], |rules| &rules.types)
    }

    /// The picked branch type, when the repo has types.
    pub fn picked_type(&self) -> Option<&str> {
        self.types().get(self.kind).map(String::as_str)
    }

    /// The text the box edits: the folder for a plain session, else the request.
    pub fn text(&self) -> &str {
        if self.mode == Mode::Plain {
            &self.folder
        } else {
            &self.request
        }
    }

    fn text_mut(&mut self) -> &mut String {
        if self.mode == Mode::Plain {
            &mut self.folder
        } else {
            &mut self.request
        }
    }

    /// Whether the type row shows: the repo has types and the start makes a branch.
    fn has_type_row(&self) -> bool {
        !self.types().is_empty() && self.mode != Mode::Plain
    }

    /// Whether the agent row shows: there is more than one agent to pick.
    fn has_agent_row(&self) -> bool {
        self.agents.len() > 1
    }

    /// Whether the base row shows: the start makes a branch.
    fn has_base_row(&self) -> bool {
        self.mode != Mode::Plain
    }

    /// Rows the type, agent and base rows add to the dialog.
    fn extra_rows(&self) -> u16 {
        u16::from(self.has_type_row())
            + u16::from(self.has_agent_row())
            + u16::from(self.has_base_row())
    }

    /// The body row of the base row.
    fn base_row(&self) -> u16 {
        5 + u16::from(self.has_type_row()) + u16::from(self.has_agent_row())
    }

    /// The base `step` places from the typed one in "" (what the repo has checked out) and bases,
    /// wrapping.
    fn cycled_base(&self, step: usize) -> String {
        let choices: Vec<&str> = std::iter::once("")
            .chain(self.bases.iter().map(String::as_str))
            .collect();
        let at = choices
            .iter()
            .position(|choice| *choice == self.base.trim())
            .unwrap_or(0);
        choices[(at + step) % choices.len()].to_string()
    }

    pub fn on_key(&mut self, key: Key) -> FormAction {
        match (self.field, key) {
            (_, Key::Esc) => return FormAction::Cancel,
            (_, Key::Enter) if !self.text().trim().is_empty() => return FormAction::Submit,
            (_, Key::Enter) => {}
            (_, Key::Tab) => {
                self.field = match self.field {
                    Field::Request if self.has_type_row() => Field::Type,
                    Field::Request | Field::Type if self.has_agent_row() => Field::Agent,
                    Field::Request | Field::Type | Field::Agent if self.has_base_row() => {
                        Field::Base
                    }
                    Field::Request | Field::Type | Field::Agent | Field::Base => Field::Mode,
                    Field::Mode => Field::Request,
                };
            }
            (Field::Request, Key::Char(c)) => self.text_mut().push(c),
            (Field::Request, Key::Backspace) => {
                self.text_mut().pop();
            }
            (Field::Request, Key::Ctrl('u')) => self.text_mut().clear(),
            (Field::Request, Key::Ctrl('w')) => {
                // A folder loses its last folder, a request its last word.
                let ends: &[char] = if self.mode == Mode::Plain {
                    &[' ', '/']
                } else {
                    &[' ']
                };
                let text = self.text_mut();
                let kept = text.trim_end_matches(ends).len();
                let start = text[..kept].rfind(ends).map_or(0, |at| at + 1);
                text.truncate(start);
            }
            (Field::Base, Key::Char(c)) => self.base.push(c),
            (Field::Base, Key::Backspace) => {
                self.base.pop();
            }
            (Field::Base, Key::Ctrl('u')) => self.base.clear(),
            (Field::Base, Key::Ctrl('w')) => {
                let kept = self.base.trim_end_matches('/').len();
                let start = self.base[..kept].rfind('/').map_or(0, |at| at + 1);
                self.base.truncate(start);
            }
            (Field::Base, Key::Down) => self.base = self.cycled_base(1),
            (Field::Base, Key::Up) => self.base = self.cycled_base(self.bases.len()),
            (Field::Type, Key::Right | Key::Char(' ' | 'l')) => {
                self.kind = (self.kind + 1) % self.types().len().max(1);
            }
            (Field::Type, Key::Left | Key::Char('h')) => {
                let count = self.types().len().max(1);
                self.kind = (self.kind + count - 1) % count;
            }
            (Field::Agent, Key::Right | Key::Char(' ' | 'l')) => {
                self.agent = (self.agent + 1) % self.agents.len().max(1);
            }
            (Field::Agent, Key::Left | Key::Char('h')) => {
                let count = self.agents.len().max(1);
                self.agent = (self.agent + count - 1) % count;
            }
            (Field::Mode, Key::Right | Key::Char(' ' | 'l')) => {
                self.mode = self.cycled(1);
            }
            (Field::Mode, Key::Left | Key::Char('h')) => {
                self.mode = self.cycled(Mode::ALL.len() - 1);
            }
            _ => {}
        }
        FormAction::Edit
    }

    /// The mode `steps` after the picked one in Mode::ALL, wrapping.
    fn cycled(&self, steps: usize) -> Mode {
        let at = Mode::ALL
            .iter()
            .position(|mode| *mode == self.mode)
            .unwrap_or(0);
        Mode::ALL[(at + steps) % Mode::ALL.len()]
    }

    /// Where the text cursor goes inside the text box or after the base, while that has the keys;
    /// `body` is the dialog's body (Dialog::body_area).
    pub fn cursor(&self, body: Rect) -> Option<(u16, u16)> {
        if self.field == Field::Base && self.base_row() < body.height {
            let typed = u16::try_from(columns(self.shown_base(body))).unwrap_or(u16::MAX);
            return Some((body.x + BASE_AT + typed, body.y + self.base_row()));
        }
        if self.field != Field::Request || body.height < 3 {
            return None;
        }
        let typed = u16::try_from(columns(self.shown_text(body))).unwrap_or(u16::MAX);
        Some((body.x + 2 + typed, body.y + 2))
    }

    /// The end of the text that fits the box.
    fn shown_text(&self, body: Rect) -> &str {
        tail(self.text(), usize::from(body.width.saturating_sub(4)))
    }

    /// The end of the base that fits its row.
    fn shown_base(&self, body: Rect) -> &str {
        tail(
            &self.base,
            usize::from(body.width.saturating_sub(BASE_AT + 1)),
        )
    }
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
        let mut form = NewFlowForm::new(PathBuf::from("/shop"), "~/shop".into(), Vec::new());

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
        let mut form = NewFlowForm::new(PathBuf::from("/shop"), "~/shop".into(), Vec::new());
        form.on_key(Key::Tab);
        form.on_key(Key::Tab);
        form.on_key(Key::Left);
        assert_eq!(
            form.mode,
            Mode::Auto,
            "without types tab goes to the base, then the mode"
        );

        form.set_branching(
            Some(Branching {
                name: "{type}/{ticket}-{slug}".into(),
                types: vec!["feature".into(), "bugfix".into(), "hotfix".into()],
            }),
            Some("main".into()),
        );
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
        let mut form = NewFlowForm::new(PathBuf::from("/shop"), "~/shop".into(), Vec::new());
        typed(&mut form, &"界".repeat(60));
        let body = Rect::new(0, 0, BODY_WIDTH, 10);

        let shown = form.shown_text(body);

        assert_eq!(columns(shown), 68);
    }

    #[test]
    fn cycles_the_mode_from_its_field_only() {
        let mut form = NewFlowForm::new(PathBuf::from("/shop"), "~/shop".into(), Vec::new());

        form.on_key(Key::Char('l'));
        assert_eq!(form.mode, Mode::Guided, "l is typed into the request");
        form.on_key(Key::Tab);
        form.on_key(Key::Tab);
        let mut seen = vec![];
        for _ in 0..3 {
            form.on_key(Key::Right);
            seen.push(form.mode);
        }
        assert_eq!(
            seen,
            [Mode::Plain, Mode::Auto, Mode::Guided],
            "right wraps around"
        );
        form.on_key(Key::Left);
        assert_eq!(form.mode, Mode::Auto);
        form.on_key(Key::Char('h'));
        assert_eq!(form.mode, Mode::Plain, "left wraps around");
        form.on_key(Key::Char(' '));
        assert_eq!(form.mode, Mode::Auto);
        assert_eq!(form.request, "l");
    }

    #[test]
    fn types_a_base_or_picks_a_flows_branch_for_it() {
        let mut form = NewFlowForm::new(PathBuf::from("/shop"), "~/shop".into(), Vec::new());
        form.bases = vec!["flow/shop-1".into(), "flow/shop-2".into()];
        typed(&mut form, "SHOP-3 button");
        form.on_key(Key::Tab);
        assert_eq!(form.field, Field::Base);
        assert_eq!(form.picked_base(), None, "what the repo has checked out");

        form.on_key(Key::Down);
        assert_eq!(form.picked_base(), Some("flow/shop-1"));
        form.on_key(Key::Down);
        form.on_key(Key::Down);
        assert_eq!(
            form.picked_base(),
            None,
            "down wraps to the checked out branch"
        );
        form.on_key(Key::Up);
        assert_eq!(form.picked_base(), Some("flow/shop-2"));
        form.on_key(Key::Ctrl('w'));
        assert_eq!(form.base, "flow/");
        typed(&mut form, "release");
        assert_eq!(form.picked_base(), Some("flow/release"));
        form.on_key(Key::Down);
        assert_eq!(
            form.picked_base(),
            Some("flow/shop-1"),
            "a typed base starts the cycle"
        );
        form.on_key(Key::Ctrl('u'));
        typed(&mut form, "release/2.0");
        form.on_key(Key::Backspace);
        assert_eq!(form.picked_base(), Some("release/2."));
        assert_eq!(form.request, "SHOP-3 button", "the base row takes the keys");
        let body = Rect::new(10, 4, BODY_WIDTH, 10);
        assert_eq!(form.cursor(body), Some((10 + 8 + 10, 4 + 5)));
        form.on_key(Key::Tab);
        assert_eq!(form.field, Field::Mode);
    }

    /// Picks plain on the mode row, then back to the box.
    fn plain(form: &mut NewFlowForm) {
        while form.field != Field::Mode {
            form.on_key(Key::Tab);
        }
        while form.mode != Mode::Plain {
            form.on_key(Key::Right);
        }
        form.on_key(Key::Tab);
    }

    #[test]
    fn a_plain_session_edits_its_folder_and_has_no_type_row() {
        let mut form =
            NewFlowForm::new(PathBuf::from("/shop"), "~/projects/shop".into(), Vec::new());
        form.set_branching(
            Some(Branching {
                name: "{type}/{slug}".into(),
                types: vec!["feature".into(), "bugfix".into()],
            }),
            Some("main".into()),
        );
        typed(&mut form, "fix totals");
        plain(&mut form);
        assert_eq!(form.extra_rows(), 0);

        assert_eq!(
            form.text(),
            "~/projects/shop",
            "prefilled with the place in view"
        );
        form.on_key(Key::Ctrl('w'));
        assert_eq!(form.folder, "~/projects/", "a folder at a time");
        typed(&mut form, "scratch");
        assert_eq!(form.folder, "~/projects/scratch");
        assert_eq!(form.request, "fix totals", "the request stays for flows");
        form.on_key(Key::Tab);
        assert_eq!(form.field, Field::Mode, "tab skips the type and base rows");
        form.base = "flow/shop-1".into();
        assert_eq!(form.picked_base(), None, "a plain session has no base");

        form.folder.clear();
        assert_eq!(form.on_key(Key::Enter), FormAction::Edit, "no folder");
        form.folder = "~/scratch".into();
        assert_eq!(form.on_key(Key::Enter), FormAction::Submit);
    }
}
