use ratatui::text::{Line, Span};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// `left` and `right` on one line of `width`, the left clipped when both do not fit.
pub fn split(left: Vec<Span<'static>>, right: Vec<Span<'static>>, width: usize) -> Line<'static> {
    let right_width = spans_width(&right);
    let room = width.saturating_sub(right_width);
    let mut spans = Vec::new();
    let mut used = 0;
    for span in left {
        let left_room = room.saturating_sub(used);
        if span.width() > left_room {
            let style = span.style;
            let cut = clip(&span.content, left_room.saturating_sub(1));
            used += columns(&cut);
            spans.push(Span::styled(cut, style));
            break;
        }
        used += span.width();
        spans.push(span);
    }
    spans.push(Span::raw(" ".repeat(room.saturating_sub(used))));
    spans.extend(right);
    Line::from(spans)
}

/// Words of `text` on rows of at most `width` columns; a word longer than a row is clipped.
pub fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut rows = Vec::new();
    for paragraph in text.lines().filter(|line| !line.trim().is_empty()) {
        let mut row = String::new();
        for word in paragraph.split_whitespace() {
            let next = if row.is_empty() {
                word.to_string()
            } else {
                format!("{row} {word}")
            };
            if columns(&next) <= width {
                row = next;
                continue;
            }
            if !row.is_empty() {
                rows.push(row);
            }
            row = clip(word, width);
        }
        if !row.is_empty() {
            rows.push(row);
        }
    }
    rows
}

/// The columns `text` takes on a terminal.
pub fn columns(text: &str) -> usize {
    text.width()
}

/// The columns a run of spans takes.
pub fn spans_width(spans: &[Span]) -> usize {
    spans.iter().map(Span::width).sum()
}

/// The end of `text` that fits in `room` columns.
pub fn tail(text: &str, room: usize) -> &str {
    let mut used = 0;
    let start = text
        .char_indices()
        .rev()
        .take_while(|(_, c)| {
            used += c.width().unwrap_or(0);
            used <= room
        })
        .last()
        .map_or(text.len(), |(index, _)| index);
    &text[start..]
}

/// The text cut to `width` columns, ending in … when it was longer.
pub fn clip(text: &str, width: usize) -> String {
    if columns(text) <= width {
        return text.to_string();
    }
    let mut kept = String::new();
    let mut used = 0;
    for c in text.chars() {
        let char_width = c.width().unwrap_or(0);
        if used + char_width + 1 > width {
            break;
        }
        kept.push(c);
        used += char_width;
    }
    kept.push('…');
    kept
}

/// `path` with ~ for the user's `home`: /Users/jan/projects/shop -> ~/projects/shop.
pub fn tilde(path: &str, home: Option<&str>) -> String {
    match home.and_then(|home| path.strip_prefix(home)) {
        Some(rest) if rest.is_empty() || rest.starts_with('/') => format!("~{rest}"),
        _ => path.to_string(),
    }
}

/// A place (`path · branch`) in `width` columns: the path loses folders from its middle first
/// (/private/…/ui-look/repo · main), then its first folder; the branch goes only when the last
/// folder alone does not fit beside it.
pub fn clip_place(place: &str, width: usize) -> String {
    let fits = |text: &str| columns(text) <= width;
    if fits(place) {
        return place.to_string();
    }
    let (path, suffix) = match place.rsplit_once(" · ") {
        Some((path, branch)) => (path, format!(" · {branch}")),
        None => (place, String::new()),
    };
    let parts: Vec<&str> = path.split('/').collect();
    let head_len = if parts.first() == Some(&"") { 2 } else { 1 };
    let head = parts[..head_len.min(parts.len())].join("/");
    let tail = |keep: usize| parts[parts.len() - keep..].join("/");
    let removable = parts.len().saturating_sub(head_len + 1);
    let with_head = (1..=removable)
        .rev()
        .map(|keep| format!("{head}/…/{}{suffix}", tail(keep)));
    let without_head = (1..parts.len())
        .rev()
        .map(|keep| format!("…/{}{suffix}", tail(keep)));
    let last = format!("…/{}", tail(1));
    with_head
        .chain(without_head)
        .chain(std::iter::once(last.clone()))
        .find(|candidate| fits(candidate))
        .unwrap_or_else(|| clip(&last, width))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortens_a_place_in_the_middle_and_keeps_its_branch() {
        let place = "/private/tmp/claude-501/x/scratchpad/ui-look/repo · main";

        assert_eq!(
            clip_place("~/projects/shop · main", 30),
            "~/projects/shop · main"
        );
        assert_eq!(clip_place(place, 34), "/private/…/ui-look/repo · main");
        assert_eq!(clip_place(place, 24), "/private/…/repo · main");
        assert_eq!(clip_place(place, 15), "…/repo · main");
        assert_eq!(clip_place(place, 9), "…/repo", "too narrow for the branch");
        assert_eq!(clip_place("~/a/b/c/d", 8), "~/…/c/d");
        for width in 1..60 {
            assert!(
                Span::raw(clip_place(place, width)).width() <= width,
                "{width}"
            );
        }
    }

    #[test]
    fn keeps_the_end_that_fits_in_columns() {
        assert_eq!(tail("ab界", 4), "ab界");
        assert_eq!(tail("ab界", 3), "b界");
        assert_eq!(tail("ab界", 1), "");
        assert_eq!(tail("", 5), "");
    }

    #[test]
    fn clips_long_text_with_an_ellipsis() {
        assert_eq!(clip("short", 10), "short");
        assert_eq!(clip("migration needs a default", 12), "migration n…");
        assert_eq!(Span::raw(clip("界界界界", 5)).width(), 5);
    }

    #[test]
    fn puts_the_right_part_flush_with_the_edge() {
        let line = split(
            vec![Span::raw(" a very long name that will not fit")],
            vec![Span::raw("29m ")],
            20,
        );

        let text: String = line
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect();
        assert_eq!(text, " a very long n… 29m ");
        assert_eq!(line.width(), 20);
    }
}
