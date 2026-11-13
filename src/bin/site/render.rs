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
        link: None,
    };
    for (event, range) in Parser::new_ext(markdown, options()).into_offset_iter() {
        renderer.push(event, &range);
    }
    let Renderer {
        mut rendered,
        broken,
        mut events,
        ..
    } = renderer;
    if !broken.is_empty() {
        return Err(broken);
    }
    mark_toc(&mut events);
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
    /// The link being read: where it starts in events, its href as written, and the name of the
    /// page or heading it points to.
    link: Option<(usize, String, String)>,
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
            }) => {
                let written = dest_url.to_string();
                let (href, name) = self.href(dest_url, range);
                self.link = name
                    .filter(|_| self.heading.is_none())
                    .map(|name| (self.events.len(), written, name));
                Event::Start(Tag::Link {
                    link_type,
                    dest_url: href,
                    title,
                    id,
                })
            }
            Event::End(TagEnd::Link) => {
                self.name_link();
                event
            }
            Event::Start(Tag::Image {
                link_type,
                dest_url,
                title,
                id,
            }) => Event::Start(Tag::Image {
                link_type,
                dest_url: self.href(dest_url, range).0,
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
        if level != HeadingLevel::H1 {
            self.events.insert(
                start + 1,
                Event::Html(CowStr::from(format!(
                    "<a class=\"anchor\" href=\"#{id}\" aria-label=\"Link to this section\" data-pagefind-ignore>#</a>"
                ))),
            );
        }
        if level == HeadingLevel::H2 {
            self.rendered.outline.push(Heading {
                text,
                id: id.clone(),
            });
        }
        self.rendered.anchors.insert(id);
    }

    /// A link whose text is its own href, that href's path or its file name, reads as a file name
    /// on the site: it shows the name of the page or heading it points to instead. Not in a
    /// heading, whose id GitHub takes from the text as written.
    fn name_link(&mut self) {
        let Some((start, written, name)) = self.link.take() else {
            return;
        };
        let path = written.split('#').next().unwrap_or_default();
        let file = path.rsplit('/').next().unwrap_or_default();
        let named_by_path = match &self.events[start + 1..] {
            [Event::Text(text) | Event::Code(text)] => {
                let text = text.trim();
                text == written || text == path || (!file.is_empty() && text == file)
            }
            _ => false,
        };
        if named_by_path {
            self.events.truncate(start + 1);
            self.events.push(Event::Text(CowStr::from(name)));
        }
    }

    /// The name of the page or heading a page link points to.
    fn name_of(&self, source: &Path, anchor: Option<&str>) -> Option<String> {
        let page = self.pages.by_source(source)?;
        let heading = anchor.and_then(|anchor| {
            page.headings
                .iter()
                .find(|(id, _)| id == anchor)
                .map(|(_, text)| text.clone())
        });
        Some(heading.unwrap_or_else(|| page.title.clone()))
    }

    /// The site's href for a link or image and, for a page, the name it points to, recording
    /// what the build needs from it. A broken one is recorded and kept as written.
    fn href(&mut self, href: CowStr<'a>, range: &Range<usize>) -> (CowStr<'a>, Option<String>) {
        let line = self.lines.line(range.start);
        match links::resolve(&self.page.source, &href, self.pages, self.root) {
            Ok(target) => {
                let name = match &target {
                    Target::Page { source, anchor, .. } => self.name_of(source, anchor.as_deref()),
                    _ => None,
                };
                let site_href = match &target {
                    Target::Page {
                        source,
                        anchor: Some(anchor),
                        ..
                    } if *source == self.page.source => CowStr::from(format!("#{anchor}")),
                    _ => CowStr::from(target.href().into_owned()),
                };
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
                (site_href, name)
            }
            Err(reason) => {
                self.broken.push(Broken {
                    source: self.page.source.clone(),
                    line,
                    href: href.to_string(),
                    reason,
                });
                (href, None)
            }
        }
    }
}

/// Marks the page's own list of sections, the first list whose every item is only a link to a
/// heading of this page, as `ul.toc`.
fn mark_toc(events: &mut [Event]) {
    let mut start = 0;
    while let Some(offset) = events[start..]
        .iter()
        .position(|event| matches!(event, Event::Start(Tag::List(None))))
    {
        let open = start + offset;
        let Some(close) = list_end(events, open) else {
            return;
        };
        if only_anchor_links(&events[open + 1..close]) {
            events[open] = Event::Html(CowStr::from("<ul class=\"toc\">\n"));
            events[close] = Event::Html(CowStr::from("</ul>\n"));
            return;
        }
        start = open + 1;
    }
}

/// Where the list opened at `open` ends.
fn list_end(events: &[Event], open: usize) -> Option<usize> {
    let mut depth = 0;
    for (at, event) in events.iter().enumerate().skip(open) {
        match event {
            Event::Start(Tag::List(_)) => depth += 1,
            Event::End(TagEnd::List(_)) => {
                depth -= 1;
                if depth == 0 {
                    return Some(at);
                }
            }
            _ => {}
        }
    }
    None
}

/// Whether a list's items are each exactly one link to "#heading".
fn only_anchor_links(items: &[Event]) -> bool {
    let mut rest = items;
    let mut count = 0;
    while !rest.is_empty() {
        let [
            Event::Start(Tag::Item),
            Event::Start(Tag::Link { dest_url, .. }),
            tail @ ..,
        ] = rest
        else {
            return false;
        };
        if !dest_url.starts_with('#') {
            return false;
        }
        let Some(end) = tail
            .iter()
            .position(|event| matches!(event, Event::End(TagEnd::Link)))
        else {
            return false;
        };
        if !tail[..end]
            .iter()
            .all(|event| matches!(event, Event::Text(_) | Event::Code(_)))
            || !matches!(tail.get(end + 1), Some(Event::End(TagEnd::Item)))
        {
            return false;
        }
        rest = &tail[end + 2..];
        count += 1;
    }
    count > 0
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

/// Every heading's id and text, as render gives them.
pub fn headings(markdown: &str) -> Vec<(String, String)> {
    let mut slugs = Slugs::new();
    let mut found = Vec::new();
    let mut text: Option<String> = None;
    for event in Parser::new_ext(markdown, options()) {
        match event {
            Event::Start(Tag::Heading { .. }) => text = Some(String::new()),
            Event::Text(part) | Event::Code(part) if text.is_some() => {
                text.get_or_insert_default().push_str(&part);
            }
            Event::End(TagEnd::Heading(_)) => {
                if let Some(text) = text.take() {
                    found.push((slugs.next(&text), text));
                }
            }
            _ => {}
        }
    }
    found
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
            (
                "docs/README.md",
                "# Docs\n\n[ui](ui.md) [log](../CHANGELOG.md)\n",
            ),
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
        assert!(page.body.contains(
            "<h3 id=\"mouse\"><a class=\"anchor\" href=\"#mouse\" aria-label=\"Link to this section\" data-pagefind-ignore>#</a>Mouse</h3>"
        ));
        assert!(
            page.body
                .contains("<h2 id=\"keys-1\"><a class=\"anchor\" href=\"#keys-1\"")
        );
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
    fn keeps_a_link_within_the_page_as_its_anchor() {
        let page = rendered(
            "# UI\n\n[keys](#keys) [again](ui.md#keys) [docs](README.md#docs)\n\n## Keys\n",
        )
        .unwrap();
        assert_eq!(
            page.body.matches("href=\"#keys\"").count(),
            3,
            "{}",
            page.body
        );
        assert!(page.body.contains("href=\"/docs/#docs\""));
        assert_eq!(page.links.len(), 3);
    }

    #[test]
    fn marks_the_first_list_of_only_section_links_as_the_toc() {
        let page = rendered(
            "# UI\n\n- [mixed](#keys) and text\n\n---\n\n- [Keys](#keys)\n- [The `peek`](#the-peek)\n\ntext\n\n- [Keys](#keys)\n\n## Keys\n\n## The peek\n",
        )
        .unwrap();
        assert_eq!(
            page.body.matches("<ul class=\"toc\">").count(),
            1,
            "{}",
            page.body
        );
        let toc = page.body.split("<ul class=\"toc\">").nth(1).unwrap();
        assert!(
            toc.starts_with("\n<li><a href=\"#keys\">Keys</a></li>"),
            "{toc}"
        );
        assert!(
            page.body
                .contains("<li><a href=\"#keys\">mixed</a> and text</li>")
        );
    }

    #[test]
    fn names_a_link_whose_text_is_a_file_path() {
        let page = rendered(
            "# UI\n\n[README.md](README.md) [../CHANGELOG.md#x](../CHANGELOG.md#x) [the docs](README.md) [`README.md`](README.md#docs) [README.md#nope](README.md#nope) [ui.md](#keys)\n\n## Keys\n",
        );
        let body = match page {
            Ok(page) => page.body,
            Err(broken) => panic!("{}", broken.len()),
        };
        assert!(body.contains("<a href=\"/docs/\">Docs</a>"), "{body}");
        assert!(body.contains("<a href=\"/docs/\">the docs</a>"));
        assert!(body.contains("<a href=\"/docs/#docs\">Docs</a>"), "{body}");
        assert!(body.contains("<a href=\"/docs/#nope\">Docs</a>"), "{body}");
        assert!(body.contains("<a href=\"#keys\">ui.md</a>"), "{body}");
        let page =
            rendered("# UI\n\n[CHANGELOG.md](../CHANGELOG.md)\n").unwrap_or_else(|_| panic!());
        assert!(
            page.body.contains("<a href=\"/changelog/\">Changelog</a>"),
            "{}",
            page.body
        );
    }

    #[test]
    fn keeps_a_file_name_in_a_heading_so_its_id_stays_github_s() {
        let page = rendered("# UI\n\n## See [README.md](README.md)\n").unwrap();
        assert!(
            page.body.contains("<h2 id=\"see-readmemd\">"),
            "{}",
            page.body
        );
        assert!(page.body.contains(">README.md</a></h2>"), "{}", page.body);
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
