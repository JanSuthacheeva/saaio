use pulldown_cmark::{Event, Parser, Tag, TagEnd};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::ui::fit::{clip, columns};
use crate::ui::theme::Theme;

/// Worker Markdown rendered in terminal columns, with no terminal escape sequences.
pub fn lines(text: &str, width: usize, theme: &Theme) -> Vec<Line<'static>> {
    if width == 0 || text.trim().is_empty() {
        return Vec::new();
    }
    let mut renderer = Renderer::new(width, theme);
    for event in Parser::new(text) {
        renderer.event(event);
    }
    renderer.finish()
}

#[derive(Debug)]
struct List {
    next: Option<u64>,
}

#[derive(Debug)]
struct Link {
    destination: String,
    label: String,
}

/// The current Markdown block and line; wrapping happens before any view adds its own prefix.
struct Renderer<'a> {
    theme: &'a Theme,
    width: usize,
    rows: Vec<Line<'static>>,
    line: Vec<Span<'static>>,
    used: usize,
    has_text: bool,
    prefix: String,
    continuation: String,
    continued: bool,
    pending_space: bool,
    quote_depth: usize,
    lists: Vec<List>,
    in_item: bool,
    heading: bool,
    code_block: bool,
    strong: usize,
    emphasis: usize,
    links: Vec<Link>,
}

impl<'a> Renderer<'a> {
    fn new(width: usize, theme: &'a Theme) -> Self {
        Self {
            theme,
            width,
            rows: Vec::new(),
            line: Vec::new(),
            used: 0,
            has_text: false,
            prefix: String::new(),
            continuation: String::new(),
            continued: false,
            pending_space: false,
            quote_depth: 0,
            lists: Vec::new(),
            in_item: false,
            heading: false,
            code_block: false,
            strong: 0,
            emphasis: 0,
            links: Vec::new(),
        }
    }

    fn finish(mut self) -> Vec<Line<'static>> {
        self.flush();
        while self.rows.last().is_some_and(|line| line.spans.is_empty()) {
            self.rows.pop();
        }
        self.rows
    }

    fn event(&mut self, event: Event<'_>) {
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(text) | Event::Html(text) | Event::InlineHtml(text) => {
                if self.code_block {
                    self.code(&text);
                } else {
                    for link in &mut self.links {
                        link.label.push_str(&text);
                    }
                    self.words(&text, self.style());
                }
            }
            Event::Code(text) => {
                for link in &mut self.links {
                    link.label.push_str(&text);
                }
                self.words(&text, self.theme.ui.muted);
            }
            Event::SoftBreak => self.pending_space = true,
            Event::HardBreak => self.flush(),
            Event::Rule => {
                self.block();
                self.words("───", self.theme.ui.rule);
                self.flush();
            }
            Event::TaskListMarker(done) => {
                self.words(if done { "[x]" } else { "[ ]" }, self.theme.ui.faint);
                self.pending_space = true;
            }
            Event::InlineMath(text) | Event::DisplayMath(text) | Event::FootnoteReference(text) => {
                self.words(&text, self.style());
            }
        }
    }

    fn start(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Paragraph if !self.in_item && !self.code_block => self.block(),
            Tag::Paragraph => {}
            Tag::Heading { .. } => {
                self.block();
                self.heading = true;
            }
            Tag::List(start) => {
                if self.lists.is_empty() {
                    self.block();
                }
                self.lists.push(List { next: start });
            }
            Tag::Item => {
                self.flush();
                let indent = "  ".repeat(self.lists.len().saturating_sub(1));
                let marker = self.lists.last_mut().map_or_else(
                    || "• ".to_string(),
                    |list| match &mut list.next {
                        Some(next) => {
                            let marker = format!("{next}. ");
                            *next += 1;
                            marker
                        }
                        None => "• ".to_string(),
                    },
                );
                let quote = "│ ".repeat(self.quote_depth);
                self.prefix = format!("{quote}{indent}{marker}");
                self.continuation = format!("{quote}{indent}{}", " ".repeat(columns(&marker)));
                self.continued = false;
                self.in_item = true;
            }
            Tag::BlockQuote(_) => {
                self.block();
                self.quote_depth += 1;
                self.prefix = "│ ".repeat(self.quote_depth);
                self.continuation = self.prefix.clone();
            }
            Tag::CodeBlock(_) => {
                self.block();
                self.code_block = true;
            }
            Tag::Strong => self.strong += 1,
            Tag::Emphasis => self.emphasis += 1,
            Tag::Link { dest_url, .. } => self.links.push(Link {
                destination: dest_url.to_string(),
                label: String::new(),
            }),
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph | TagEnd::Heading(_) => {
                self.flush();
                self.heading = false;
            }
            TagEnd::Item => {
                self.flush();
                self.in_item = false;
                self.prefix = "│ ".repeat(self.quote_depth);
                self.continuation = self.prefix.clone();
                self.continued = false;
            }
            TagEnd::List(_) => {
                self.flush();
                self.lists.pop();
            }
            TagEnd::BlockQuote(_) => {
                self.flush();
                self.quote_depth = self.quote_depth.saturating_sub(1);
                self.prefix = "│ ".repeat(self.quote_depth);
                self.continuation = self.prefix.clone();
            }
            TagEnd::CodeBlock => {
                self.flush();
                self.code_block = false;
            }
            TagEnd::Strong => self.strong = self.strong.saturating_sub(1),
            TagEnd::Emphasis => self.emphasis = self.emphasis.saturating_sub(1),
            TagEnd::Link => {
                if let Some(link) = self.links.pop()
                    && !link.destination.is_empty()
                    && link.destination != link.label
                {
                    self.words(&format!(" ({})", link.destination), self.theme.ui.muted);
                }
            }
            _ => {}
        }
    }

    fn style(&self) -> Style {
        let mut style = if self.heading {
            self.theme.ui.strong
        } else {
            self.theme.ui.text
        };
        if self.heading || self.strong > 0 {
            style = style.add_modifier(Modifier::BOLD);
        }
        if self.emphasis > 0 {
            style = style.add_modifier(Modifier::ITALIC);
        }
        if !self.links.is_empty() {
            style = style.add_modifier(Modifier::UNDERLINED);
        }
        style
    }

    fn block(&mut self) {
        self.flush();
        if self.rows.last().is_some_and(|line| !line.spans.is_empty()) {
            self.rows.push(Line::raw(""));
        }
        self.continued = false;
        self.pending_space = false;
    }

    fn ensure_line(&mut self) {
        if !self.line.is_empty() {
            return;
        }
        let prefix = if self.continued {
            &self.continuation
        } else {
            &self.prefix
        };
        if !prefix.is_empty() {
            let prefix = clip(prefix, self.width.saturating_sub(1));
            self.used = columns(&prefix);
            self.line.push(Span::styled(prefix, self.theme.ui.faint));
        }
    }

    fn words(&mut self, text: &str, style: Style) {
        let mut space = self.pending_space || text.chars().next().is_some_and(char::is_whitespace);
        for word in text.split_whitespace() {
            self.word(word, style, space);
            space = true;
        }
        self.pending_space = text.chars().last().is_some_and(char::is_whitespace);
    }

    fn word(&mut self, word: &str, style: Style, space: bool) {
        self.ensure_line();
        let gap = usize::from(space && self.has_text);
        if self.has_text && self.used + gap + columns(word) > self.width {
            self.flush();
            self.continued = true;
            self.ensure_line();
        } else if gap > 0 {
            self.line.push(Span::styled(" ", style));
            self.used += 1;
        }
        let room = self.width.saturating_sub(self.used);
        if room == 0 {
            return;
        }
        let fitted = clip(word, room);
        self.used += columns(&fitted);
        self.has_text = true;
        self.line.push(Span::styled(fitted, style));
    }

    fn code(&mut self, text: &str) {
        for part in text.split_inclusive('\n') {
            let line = part.trim_end_matches('\n');
            if !line.is_empty() {
                let room = self.width.saturating_sub(self.used);
                if room > 0 {
                    let fitted = clip(line, room);
                    self.used += columns(&fitted);
                    self.line.push(Span::styled(fitted, self.theme.ui.muted));
                    self.has_text = true;
                }
            }
            if part.ends_with('\n') {
                self.flush();
                if line.is_empty() {
                    self.rows.push(Line::raw(""));
                }
            }
        }
    }

    fn flush(&mut self) {
        if self.has_text {
            self.rows.push(Line::from(std::mem::take(&mut self.line)));
            self.continued = true;
        } else {
            self.line.clear();
        }
        self.used = 0;
        self.has_text = false;
        self.pending_space = false;
    }
}

#[cfg(test)]
mod tests {
    use ratatui::style::Modifier;

    use crate::ui::theme::Theme;

    use super::lines;

    fn text(lines: &[ratatui::text::Line<'static>]) -> Vec<String> {
        lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect()
            })
            .collect()
    }

    #[test]
    fn styles_inline_markdown_and_writes_links_as_text() {
        let rows = lines(
            "**bold** and `code` at [docs](https://example.com)",
            80,
            &Theme::default(),
        );
        assert_eq!(text(&rows), ["bold and code at docs (https://example.com)"]);
        assert!(rows[0].spans.iter().any(|span| {
            span.content == "bold" && span.style.add_modifier.contains(Modifier::BOLD)
        }));
        assert!(rows[0].spans.iter().any(|span| span.content == "code"));
    }

    #[test]
    fn renders_headings_lists_quotes_and_code_blocks() {
        let rows = lines(
            "# Title\n\n- first\n- second\n\n> quoted\n\n```text\nx = 1\n```",
            28,
            &Theme::default(),
        );
        let written = text(&rows);
        assert!(written.iter().any(|row| row == "Title"), "{written:?}");
        assert!(
            written.iter().any(|row| row.starts_with("• first")),
            "{written:?}"
        );
        assert!(
            written.iter().any(|row| row.starts_with("• second")),
            "{written:?}"
        );
        assert!(
            written.iter().any(|row| row.starts_with("│ quoted")),
            "{written:?}"
        );
        assert!(written.iter().any(|row| row == "x = 1"), "{written:?}");
    }

    #[test]
    fn fits_unicode_and_clips_long_words_by_terminal_columns() {
        let rows = lines("**界界** supercalifragilistic", 7, &Theme::default());
        assert!(rows.iter().all(|line| line.width() <= 7));
        assert!(text(&rows).iter().any(|row| row.ends_with('…')));
        assert!(lines("anything", 0, &Theme::default()).is_empty());
    }

    #[test]
    fn preserves_blank_code_lines_and_hangs_numbered_list_continuations() {
        let code = lines("```\na\n\nb\n```", 12, &Theme::default());
        assert_eq!(text(&code), ["a", "", "b"]);

        let list = lines("3. alpha beta gamma", 10, &Theme::default());
        assert_eq!(text(&list), ["3. alpha", "   beta", "   gamma"]);
        assert!(list.iter().all(|line| line.width() <= 10));
    }
}
