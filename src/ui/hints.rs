use std::borrow::Cow;

use ratatui::text::{Line, Span};

use crate::ui::fit::columns;
use crate::ui::theme::Theme;

/// Columns between two hints.
const GAP: &str = "   ";

/// One key and what it does; `active` false draws both faint, for a key that does nothing now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hint {
    pub key: &'static str,
    pub label: Cow<'static, str>,
    pub active: bool,
    /// Kept when the row is too narrow: esc and ? stay as the others go.
    pub kept: bool,
}

impl Hint {
    pub fn new(key: &'static str, label: impl Into<Cow<'static, str>>) -> Hint {
        Hint {
            key,
            label: label.into(),
            active: true,
            kept: false,
        }
    }

    pub fn inactive(self) -> Hint {
        Hint {
            active: false,
            ..self
        }
    }

    pub fn kept(self) -> Hint {
        Hint { kept: true, ..self }
    }

    fn width(&self) -> usize {
        match self.label.as_ref() {
            "" => columns(self.key),
            label => columns(self.key) + 1 + columns(label),
        }
    }
}

/// The hints that fit `width`, three columns apart: whole hints are dropped from the right, the
/// kept ones last; never a cut word.
pub fn line(hints: &[Hint], width: usize, theme: &Theme) -> Line<'static> {
    let mut shown: Vec<&Hint> = hints.iter().collect();
    let used = |shown: &[&Hint]| {
        shown.iter().map(|hint| hint.width()).sum::<usize>()
            + columns(GAP) * shown.len().saturating_sub(1)
    };
    while used(&shown) > width {
        let drop = shown
            .iter()
            .rposition(|hint| !hint.kept)
            .unwrap_or(shown.len() - 1);
        shown.remove(drop);
    }
    let mut spans = Vec::new();
    for hint in shown {
        if !spans.is_empty() {
            spans.push(Span::raw(GAP));
        }
        let (key, label) = if hint.active {
            (theme.ui.key, theme.ui.faint)
        } else {
            (theme.ui.faint, theme.ui.faint)
        };
        spans.push(Span::styled(hint.key, key));
        if !hint.label.is_empty() {
            spans.push(Span::styled(format!(" {}", hint.label), label));
        }
    }
    Line::from(spans)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hints() -> Vec<Hint> {
        vec![
            Hint::new("enter", "jump in"),
            Hint::new("n", "new flow"),
            Hint::new("x", "clear finished flow").inactive(),
            Hint::new("esc", "back to master").kept(),
        ]
    }

    fn text(line: &Line) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }

    #[test]
    fn shows_every_hint_that_fits() {
        let line = line(&hints(), 200, &Theme::default());

        assert_eq!(
            text(&line),
            "enter jump in   n new flow   x clear finished flow   esc back to master"
        );
    }

    #[test]
    fn drops_whole_hints_from_the_right_and_the_kept_ones_last() {
        let theme = Theme::default();
        let full = text(&line(&hints(), 200, &theme));

        for width in 0..=full.chars().count() {
            let fitted = line(&hints(), width, &theme);
            assert!(fitted.width() <= width, "{width}: {}", text(&fitted));
        }
        assert_eq!(
            text(&line(&hints(), 50, &theme)),
            "enter jump in   n new flow   esc back to master"
        );
        assert_eq!(
            text(&line(&hints(), 20, &theme)),
            "esc back to master",
            "the kept hint outlasts the others"
        );
        assert_eq!(text(&line(&hints(), 10, &theme)), "", "never a cut word");
    }

    #[test]
    fn draws_a_key_that_does_nothing_faint() {
        let theme = Theme::default();
        let line = line(&hints(), 200, &theme);

        let style = |content: &str| {
            line.spans
                .iter()
                .find(|span| span.content == content)
                .unwrap()
                .style
        };
        assert_eq!(style("n"), theme.ui.key);
        assert_eq!(style(" new flow"), theme.ui.faint);
        assert_eq!(style("x"), theme.ui.faint);
        assert_eq!(style(" clear finished flow"), theme.ui.faint);
    }

    #[test]
    fn a_key_without_a_label_is_the_key_alone() {
        let line = line(
            &[Hint::new("?", ""), Hint::new("q", "live")],
            20,
            &Theme::default(),
        );

        assert_eq!(text(&line), "?   q live");
    }
}
