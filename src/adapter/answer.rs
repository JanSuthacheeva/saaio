use crate::adapter::cut;
use crate::event::EventKind;

/// Waiting with the last sentence of the answer's last line with text (not a code fence) that asks
/// (ends in "?", or starts "Let me know" or "Tell me"); else idle, with its last sentence: only a
/// question needs the person. A line of code never asks: it is the note as it stands.
pub fn turn_end(answer: &str) -> EventKind {
    let Some((last, code)) = last_line(answer) else {
        return EventKind::Idle {
            note: "turn ended".to_string(),
        };
    };
    if code {
        return EventKind::Idle { note: cut(last) };
    }
    let plain = plain(last);
    let sentences = sentences(&plain);
    let asks = |sentence: &&String| {
        let lower = sentence.to_lowercase();
        sentence.ends_with('?') || lower.starts_with("let me know") || lower.starts_with("tell me")
    };
    match sentences.iter().rev().find(asks) {
        Some(question) => EventKind::Waiting {
            question: cut(question),
            questions: Vec::new(),
        },
        None => EventKind::Idle {
            note: cut(sentences.last().map_or(last, |sentence| sentence.as_str())),
        },
    }
}

/// The answer's last line with text, past code fences, and whether it is inside a code block.
fn last_line(answer: &str) -> Option<(&str, bool)> {
    let mut code = false;
    let mut last = None;
    for line in answer.lines().map(str::trim) {
        if line.starts_with("```") {
            code = !code;
        } else if !line.is_empty() {
            last = Some((line, code));
        }
    }
    last
}

/// A Markdown line as plain text: no list marker, no emphasis or code ticks.
fn plain(line: &str) -> String {
    let mut text = line.trim();
    if let Some(rest) = text
        .strip_prefix("- ")
        .or_else(|| text.strip_prefix("* "))
        .or_else(|| text.strip_prefix("+ "))
    {
        text = rest;
    } else if let Some((number, rest)) = text.split_once(". ")
        && !number.is_empty()
        && number.chars().all(|c| c.is_ascii_digit())
    {
        text = rest;
    }
    text.chars()
        .filter(|c| !matches!(c, '*' | '_' | '`'))
        .collect::<String>()
        .trim()
        .to_string()
}

/// Sentences split after ".", "!" or "?" followed by whitespace.
fn sentences(text: &str) -> Vec<String> {
    let mut sentences = Vec::new();
    let mut current = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        current.push(c);
        if matches!(c, '.' | '!' | '?') && chars.peek().is_some_and(|next| next.is_whitespace()) {
            sentences.push(current.trim().to_string());
            current.clear();
        }
    }
    if !current.trim().is_empty() {
        sentences.push(current.trim().to_string());
    }
    sentences
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waits_for_the_person_only_when_the_last_line_asks() {
        let waiting = |question: &str| EventKind::Waiting {
            question: question.into(),
            questions: Vec::new(),
        };
        let idle = |note: &str| EventKind::Idle { note: note.into() };

        assert_eq!(
            turn_end("Done.\n\n- **Which** approach: `A` or B? I lean A."),
            waiting("Which approach: A or B?")
        );
        assert_eq!(turn_end("1. Shall I go on?"), waiting("Shall I go on?"));
        assert_eq!(
            turn_end("Tests pass. Tell me which one to keep."),
            waiting("Tell me which one to keep.")
        );
        assert_eq!(turn_end("All set. Tests pass."), idle("Tests pass."));
        assert_eq!(
            turn_end("Started the review.\n\nThe review is still running."),
            idle("The review is still running.")
        );
        assert_eq!(turn_end("  \n"), idle("turn ended"));
    }

    #[test]
    fn reads_past_a_closing_code_fence() {
        assert_eq!(
            turn_end("Wrote `answers.txt`:\n\n```\nColor: Green\n```\n"),
            EventKind::Idle {
                note: "Color: Green".into()
            }
        );
        assert_eq!(
            turn_end("Run this:\n\n```sql\nSELECT * FROM users WHERE id = ?\n```"),
            EventKind::Idle {
                note: "SELECT * FROM users WHERE id = ?".into()
            },
            "a line of code never asks"
        );
        assert_eq!(
            turn_end("```\n```"),
            EventKind::Idle {
                note: "turn ended".into()
            }
        );
    }
}
