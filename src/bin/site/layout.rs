use std::fmt::Write as _;

use crate::links::REPO;
use crate::pages::{Page, Pages};
use crate::render::{Heading, Rendered};

const TEMPLATE: &str = include_str!("page.html");
pub const CSS: &str = include_str!("site.css");
pub const JS: &str = include_str!("site.js");
pub const FAVICON: &str = include_str!("favicon.svg");

/// A page in the site's frame: header, the page list with this page marked, its body, its
/// outline and a link to edit it on GitHub.
pub fn page(page: &Page, pages: &Pages, rendered: &Rendered) -> String {
    let edit = format!(
        "    <p class=\"edit\" data-pagefind-ignore><a href=\"{REPO}/edit/main/{}\">Edit this page on GitHub</a></p>",
        escape(&page.source.to_string_lossy())
    );
    let title = if page.url == "/" {
        "saaio - orchestrate coding agents from one terminal".to_owned()
    } else {
        format!("{} - saaio", page.title)
    };
    fill(
        TEMPLATE,
        &[
            ("title", &escape(&title)),
            ("url", &page.url),
            ("nav", &nav(pages, Some(page))),
            ("body", &rendered.body),
            ("edit", &edit),
            ("outline", &outline(&rendered.outline)),
        ],
    )
}

/// 404.html: the frame with "Page not found" and the page list.
pub fn not_found(pages: &Pages) -> String {
    let body = "<h1>Page not found</h1>\n<p>There is no page at this address. Try the search \
                above, or start from <a href=\"/docs/\">the documentation</a>.</p>";
    fill(
        TEMPLATE,
        &[
            ("title", "Page not found - saaio"),
            ("url", "/404.html"),
            ("nav", &nav(pages, None)),
            ("body", body),
            ("edit", ""),
            ("outline", ""),
        ],
    )
}

/// The page list by the sections docs/README.md gives, the home page leading the first.
fn nav(pages: &Pages, current: Option<&Page>) -> String {
    let mut html = String::new();
    let mut open: Option<&str> = None;
    for page in pages.iter() {
        let Some(section) = page.section.as_deref() else {
            continue;
        };
        if open != Some(section) {
            if open.is_some() {
                html.push_str("    </ul></div>\n");
            }
            let _ = writeln!(
                html,
                "    <div class=\"nav-section\"><p class=\"nav-title\">{}</p><ul>",
                escape(section)
            );
            open = Some(section);
        }
        let marked = if current.is_some_and(|current| current.source == page.source) {
            " aria-current=\"page\""
        } else {
            ""
        };
        let _ = writeln!(
            html,
            "      <li><a href=\"{}\"{marked}>{}</a></li>",
            page.url,
            escape(label(page))
        );
    }
    if open.is_some() {
        html.push_str("    </ul></div>");
    }
    html
}

fn label(page: &Page) -> &str {
    if page.url == "/" {
        "Overview"
    } else {
        &page.title
    }
}

/// "On this page": the ## headings, or nothing for a page without any.
fn outline(headings: &[Heading]) -> String {
    if headings.is_empty() {
        return String::new();
    }
    let mut html = String::from("    <p class=\"outline-title\">On this page</p>\n    <ul>\n");
    for heading in headings {
        let _ = writeln!(
            html,
            "      <li><a href=\"#{}\">{}</a></li>",
            heading.id,
            escape(&heading.text)
        );
    }
    html.push_str("    </ul>");
    html
}

/// Fills every {{key}} of the template in one pass; values are not searched again.
fn fill(template: &str, values: &[(&str, &str)]) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let end = after.find("}}").expect("an unclosed {{ in the template");
        let key = &after[..end];
        let value = values
            .iter()
            .find(|(name, _)| *name == key)
            .map(|(_, value)| *value)
            .unwrap_or_else(|| panic!("no value for {{{{{key}}}}}"));
        out.push_str(value);
        rest = &after[end + 2..];
    }
    out.push_str(rest);
    out
}

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::fixtures::checkout;
    use crate::render::render;

    fn built(source: &str) -> String {
        let repo = checkout(&[
            (
                "docs/README.md",
                "# Docs\n\n## Guides\n\n[ui](ui.md) [flows](flows.md)\n\n## Project\n\n[log](../CHANGELOG.md)\n",
            ),
            ("docs/ui.md", "# Keys & mouse\n\n## Keys\n\n## The peek\n"),
            ("docs/flows.md", "# Flows\n"),
        ]);
        let pages = Pages::read(repo.path()).unwrap();
        let page = pages.by_source(Path::new(source)).unwrap();
        let markdown = std::fs::read_to_string(repo.path().join(source)).unwrap();
        let rendered = render(page, &markdown, &pages, repo.path()).unwrap_or_else(|_| panic!());
        super::page(page, &pages, &rendered)
    }

    #[test]
    fn fills_every_placeholder() {
        let html = built("docs/ui.md");
        assert!(!html.contains("{{"), "{html}");
        assert!(html.contains("<title>Keys &amp; mouse - saaio</title>"));
        assert!(html.contains("<link rel=\"canonical\" href=\"https://saaio.dev/docs/ui/\">"));
        assert!(html.contains("<h2 id=\"keys\">Keys</h2>"));
        assert!(
            html.contains("href=\"https://github.com/JanSuthacheeva/saaio/edit/main/docs/ui.md\"")
        );
    }

    #[test]
    fn groups_the_nav_by_section_and_marks_the_current_page() {
        let html = built("docs/ui.md");
        assert!(html.contains("<a href=\"/docs/ui/\" aria-current=\"page\">Keys &amp; mouse</a>"));
        assert!(html.contains("<a href=\"/docs/flows/\">Flows</a>"));
        let guides = html.find("<p class=\"nav-title\">Guides</p>").unwrap();
        let overview = html.find("<a href=\"/\">Overview</a>").unwrap();
        let project = html.find("<p class=\"nav-title\">Project</p>").unwrap();
        let changelog = html.find("<a href=\"/changelog/\">").unwrap();
        assert!(guides < overview && overview < project && project < changelog);
        assert!(!html.contains("<a href=\"/docs/\">"));
        assert_eq!(html.matches("<ul>").count(), html.matches("</ul>").count());
    }

    #[test]
    fn outlines_the_level_two_headings() {
        let html = built("docs/ui.md");
        assert!(html.contains("<p class=\"outline-title\">On this page</p>"));
        assert!(html.contains("<li><a href=\"#keys\">Keys</a></li>"));
        assert!(html.contains("<li><a href=\"#the-peek\">The peek</a></li>"));
        assert!(!built("docs/flows.md").contains("outline-title"));
    }

    #[test]
    fn names_the_home_page_after_saaio() {
        assert!(
            built("README.md")
                .contains("<title>saaio - orchestrate coding agents from one terminal</title>")
        );
    }

    #[test]
    fn the_not_found_page_has_the_frame_and_no_current_page() {
        let repo = checkout(&[("docs/README.md", "# Docs\n")]);
        let html = not_found(&Pages::read(repo.path()).unwrap());
        assert!(html.contains("<h1>Page not found</h1>"));
        assert!(!html.contains("aria-current"));
        assert!(!html.contains("{{"));
    }

    #[test]
    fn fill_replaces_each_key_once() {
        assert_eq!(
            fill("a {{x}} b {{y}}", &[("x", "{{y}}"), ("y", "2")]),
            "a {{y}} b 2"
        );
    }

    #[test]
    #[should_panic(expected = "no value for {{z}}")]
    fn fill_panics_on_a_key_without_a_value() {
        fill("{{z}}", &[]);
    }
}
