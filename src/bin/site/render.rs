use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};

fn options() -> Options {
    Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_GFM
}

/// The text of the first # heading.
pub fn title(markdown: &str) -> Option<String> {
    let mut inside = false;
    let mut text = String::new();
    for event in Parser::new_ext(markdown, options()) {
        match event {
            Event::Start(Tag::Heading {
                level: HeadingLevel::H1,
                ..
            }) => inside = true,
            Event::End(TagEnd::Heading(HeadingLevel::H1)) => return Some(text),
            Event::Text(part) | Event::Code(part) if inside => text.push_str(&part),
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_title_is_the_first_level_one_heading() {
        let markdown = "intro\n\n## Not this\n\n# The `saaio` UI\n\n# Later\n";
        assert_eq!(title(markdown).as_deref(), Some("The saaio UI"));
    }

    #[test]
    fn a_page_without_a_level_one_heading_has_no_title() {
        assert_eq!(title("## Only this\n"), None);
    }
}
