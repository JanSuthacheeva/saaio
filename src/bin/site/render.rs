use std::collections::HashSet;
use std::ops::Range;
use std::path::{Path, PathBuf};

use pulldown_cmark::{
    BlockQuoteKind, CowStr, Event, HeadingLevel, Options, Parser, Tag, TagEnd, html,
};

use crate::links::{self, Broken, Link, Target};
use crate::pages::{Page, Pages};
use crate::slug::Slugs;

/// One page's HTML body and what the build needs to know about it.
pub struct Rendered {
    pub body: String,
    /// Its ## headings, for "On this page".
    pub outline: Vec<Heading>,
    /// Every heading id.
    pub anchors: HashSet<String>,
    /// Links into pages, their anchors checked once every page rendered.
    pub links: Vec<Link>,
    /// Repository files the site copies.
    pub assets: Vec<PathBuf>,
}

pub struct Heading {
    pub level: u8,
    pub text: String,
    pub id: String,
}

fn options() -> Options {
    Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_GFM
}

/// Renders a page as GitHub does: headings with GitHub's ids, GitHub alerts with their label,
/// links and images through links::resolve, tables in a box of their own that scrolls. Errors
/// list every link whose file is missing or outside the checkout.
pub fn render(
    page: &Page,
    markdown: &str,
    pages: &Pages,
    root: &Path,
) -> Result<Rendered, Vec<Broken>> {
    let mut renderer = Renderer {
        page,
        pages,
        root,
        lines: LineIndex::new(markdown),
        slugs: Slugs::new(),
        rendered: Rendered {
            body: String::new(),
            outline: Vec::new(),
            anchors: HashSet::new(),
            links: Vec::new(),
            assets: Vec::new(),
        },
        broken: Vec::new(),
        events: Vec::new(),
        heading: None,
    };
    for (event, range) in Parser::new_ext(markdown, options()).into_offset_iter() {
        renderer.push(event, &range);
    }
    let Renderer {
        mut rendered,
        broken,
        events,
        ..
    } = renderer;
    if !broken.is_empty() {
        return Err(broken);
    }
    html::push_html(&mut rendered.body, events.into_iter());
    Ok(rendered)
}

/// One page on its way through the parser's events.
struct Renderer<'a> {
    page: &'a Page,
    pages: &'a Pages,
    root: &'a Path,
    lines: LineIndex,
    slugs: Slugs,
    rendered: Rendered,
    broken: Vec<Broken>,
    events: Vec<Event<'a>>,
    /// Where the heading being read starts in events, and its level.
    heading: Option<(usize, HeadingLevel)>,
}

impl<'a> Renderer<'a> {
    fn push(&mut self, event: Event<'a>, range: &Range<usize>) {
        let event = match event {
            Event::Start(Tag::Heading { level, .. }) => {
                self.heading = Some((self.events.len(), level));
                event
            }
            Event::End(TagEnd::Heading(_)) => {
                self.close_heading();
                event
            }
            Event::Start(Tag::Link {
                link_type,
                dest_url,
                title,
                id,
            }) => Event::Start(Tag::Link {
                link_type,
                dest_url: self.href(dest_url, range),
                title,
                id,
            }),
            Event::Start(Tag::Image {
                link_type,
                dest_url,
                title,
                id,
            }) => Event::Start(Tag::Image {
                link_type,
                dest_url: self.href(dest_url, range),
                title,
                id,
            }),
            Event::Start(Tag::Table(alignments)) => {
                self.events
                    .push(Event::Html(CowStr::from("<div class=\"table\">")));
                Event::Start(Tag::Table(alignments))
            }
            Event::End(TagEnd::Table) => {
                self.events.push(Event::End(TagEnd::Table));
                Event::Html(CowStr::from("</div>"))
            }
            Event::Start(Tag::BlockQuote(Some(kind))) => {
                self.events.push(Event::Start(Tag::BlockQuote(Some(kind))));
                Event::Html(CowStr::from(format!(
                    "<p class=\"markdown-alert-title\">{}</p>",
                    label(kind)
                )))
            }
            event => event,
        };
        self.events.push(event);
    }

    /// Gives the heading just read its id, and records it.
    fn close_heading(&mut self) {
        let Some((start, level)) = self.heading.take() else {
            return;
        };
        let text = plain_text(&self.events[start + 1..]);
        let id = self.slugs.next(&text);
        self.events[start] = Event::Start(Tag::Heading {
            level,
            id: Some(CowStr::from(id.clone())),
            classes: Vec::new(),
            attrs: Vec::new(),
        });
        if level == HeadingLevel::H2 {
            self.rendered.outline.push(Heading {
                level: 2,
                text,
                id: id.clone(),
            });
        }
        self.rendered.anchors.insert(id);
    }

    /// The site's href for a link or image, recording what the build needs from it. A broken
    /// one is recorded and kept as written.
    fn href(&mut self, href: CowStr<'a>, range: &Range<usize>) -> CowStr<'a> {
        let line = self.lines.line(range.start);
        match links::resolve(&self.page.source, &href, self.pages, self.root) {
            Ok(target) => {
                let site_href = CowStr::from(target.href().into_owned());
                match target {
                    Target::Asset { source } => self.rendered.assets.push(source),
                    Target::Page {
                        anchor: Some(_), ..
                    } => self.rendered.links.push(Link {
                        line,
                        href: href.to_string(),
                        target,
                    }),
                    _ => {}
                }
                site_href
            }
            Err(reason) => {
                self.broken.push(Broken {
                    source: self.page.source.clone(),
                    line,
                    href: href.to_string(),
                    reason,
                });
                href
            }
        }
    }
}

fn label(kind: BlockQuoteKind) -> &'static str {
    match kind {
        BlockQuoteKind::Note => "Note",
        BlockQuoteKind::Tip => "Tip",
        BlockQuoteKind::Important => "Important",
        BlockQuoteKind::Warning => "Warning",
        BlockQuoteKind::Caution => "Caution",
    }
}

/// The text a heading reads as: its text and code, no markup.
fn plain_text(events: &[Event]) -> String {
    let mut text = String::new();
    for event in events {
        if let Event::Text(part) | Event::Code(part) = event {
            text.push_str(part);
        }
    }
    text
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

/// Byte offsets to 1-based line numbers.
struct LineIndex {
    starts: Vec<usize>,
}

impl LineIndex {
    fn new(text: &str) -> LineIndex {
        let starts = std::iter::once(0)
            .chain(text.match_indices('\n').map(|(at, _)| at + 1))
            .collect();
        LineIndex { starts }
    }

    fn line(&self, offset: usize) -> usize {
        self.starts.partition_point(|start| *start <= offset)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::checkout;
    use crate::links::Reason;

    fn rendered(markdown: &str) -> Result<Rendered, Vec<Broken>> {
        let repo = checkout(&[
            ("docs/README.md", "# Docs\n\n[ui](ui.md)\n"),
            ("docs/ui.md", markdown),
            ("docs/media/shot.png", ""),
        ]);
        let pages = Pages::read(repo.path()).unwrap();
        let page = pages.by_source(Path::new("docs/ui.md")).unwrap();
        render(page, markdown, &pages, repo.path())
    }

    #[test]
    fn gives_headings_github_ids_and_outlines_the_level_two_ones() {
        let page = rendered("# The `saaio` UI\n\n## Keys\n\n### Mouse\n\n## Keys\n").unwrap();
        assert!(
            page.body
                .contains("<h1 id=\"the-saaio-ui\">The <code>saaio</code> UI</h1>")
        );
        assert!(page.body.contains("<h3 id=\"mouse\">Mouse</h3>"));
        assert!(page.body.contains("<h2 id=\"keys-1\">Keys</h2>"));
        let outline: Vec<(&str, &str)> = page
            .outline
            .iter()
            .map(|heading| (heading.text.as_str(), heading.id.as_str()))
            .collect();
        assert_eq!(outline, [("Keys", "keys"), ("Keys", "keys-1")]);
        let mut anchors: Vec<&str> = page.anchors.iter().map(String::as_str).collect();
        anchors.sort_unstable();
        assert_eq!(anchors, ["keys", "keys-1", "mouse", "the-saaio-ui"]);
    }

    #[test]
    fn labels_a_github_alert() {
        let page = rendered("# UI\n\n> [!WARNING]\n> Careful.\n").unwrap();
        assert!(page.body.contains(
            "<blockquote class=\"markdown-alert-warning\">\n<p class=\"markdown-alert-title\">Warning</p>\n<p>Careful.</p>"
        ), "{}", page.body);
    }

    #[test]
    fn puts_a_table_in_a_box_of_its_own() {
        let page = rendered("# UI\n\n| a |\n| - |\n| 1 |\n").unwrap();
        assert!(
            page.body.contains("<div class=\"table\"><table>"),
            "{}",
            page.body
        );
        assert!(page.body.contains("</table>\n</div>"), "{}", page.body);
    }

    #[test]
    fn rewrites_links_and_images_for_the_site() {
        let page = rendered(
            "# UI\n\n[docs](README.md#top) [log](../CHANGELOG.md) [cfg](../ROADMAP.md)\n\n![shot](media/shot.png) <https://saaio.dev>\n",
        )
        .unwrap();
        assert!(page.body.contains("href=\"/docs/#top\""), "{}", page.body);
        assert!(page.body.contains("href=\"/changelog/\""));
        assert!(page.body.contains("src=\"/docs/media/shot.png\""));
        assert!(page.body.contains("href=\"https://saaio.dev\""));
        assert_eq!(page.assets, [PathBuf::from("docs/media/shot.png")]);
        let checked: Vec<(usize, &str)> = page
            .links
            .iter()
            .map(|link| (link.line, link.href.as_str()))
            .collect();
        assert_eq!(checked, [(3, "README.md#top")]);
    }

    #[test]
    fn lists_every_broken_link_with_its_line() {
        let broken = rendered("# UI\n\n[a](gone.md)\n\ntext\n![b](../../x.png)\n")
            .err()
            .unwrap();
        let found: Vec<(usize, &str, &Reason)> = broken
            .iter()
            .map(|broken| (broken.line, broken.href.as_str(), &broken.reason))
            .collect();
        assert_eq!(
            found,
            [
                (3, "gone.md", &Reason::NoFile),
                (6, "../../x.png", &Reason::OutsideRepo)
            ]
        );
    }

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
