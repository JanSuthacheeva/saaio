use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};

use crate::links;
use crate::render;

/// One Markdown file the site publishes.
pub struct Page {
    /// Relative to the checkout: docs/ui.md.
    pub source: PathBuf,
    /// Where the site serves it: /docs/ui/.
    pub url: String,
    /// Its first # heading.
    pub title: String,
    /// The docs/README.md section that lists it, which the nav groups it under; the home page
    /// leads the first section. None for a page outside the nav.
    pub section: Option<String>,
    /// Its Markdown as the site renders it: without the github-only parts.
    pub markdown: String,
    /// Every heading's id and text, for links that name one.
    pub headings: Vec<(String, String)>,
}

/// The pages the site publishes, in nav order.
pub struct Pages {
    list: Vec<Page>,
}

/// Files in docs/ that are for contributors, not pages.
const UNPUBLISHED: &[&str] = &["docs/HANDOVER.md"];

const INDEX: &str = "docs/README.md";

impl Pages {
    /// README.md, docs/README.md, then the pages docs/README.md links, in its order, each in
    /// the `##` section that links it. Errors when a docs page is missing from docs/README.md.
    pub fn read(root: &Path) -> anyhow::Result<Pages> {
        let listed = listed(root)?;
        let sources: Vec<PathBuf> = listed.iter().map(|(source, _)| source.clone()).collect();
        let unlisted = unlisted(root, &sources)?;
        if !unlisted.is_empty() {
            let names: Vec<String> = unlisted
                .iter()
                .map(|source| source.display().to_string())
                .collect();
            let verb = if names.len() == 1 { "is" } else { "are" };
            bail!("{} {verb} not listed in {INDEX}", names.join(", "));
        }
        let first = listed.iter().find_map(|(_, section)| section.clone());
        let mut all = vec![
            (PathBuf::from("README.md"), first),
            (PathBuf::from(INDEX), None),
        ];
        all.extend(listed);
        let list = all
            .into_iter()
            .map(|(source, section)| page(root, source, section))
            .collect::<anyhow::Result<_>>()?;
        Ok(Pages { list })
    }

    /// The pages before and after this one in the nav, for the pager.
    pub fn neighbours(&self, page: &Page) -> (Option<&Page>, Option<&Page>) {
        if page.section.is_none() {
            return (None, None);
        }
        let nav: Vec<&Page> = self
            .list
            .iter()
            .filter(|page| page.section.is_some())
            .collect();
        let Some(at) = nav.iter().position(|other| other.source == page.source) else {
            return (None, None);
        };
        (
            at.checked_sub(1).map(|before| nav[before]),
            nav.get(at + 1).copied(),
        )
    }

    pub fn by_source(&self, source: &Path) -> Option<&Page> {
        self.list.iter().find(|page| page.source == source)
    }

    pub fn iter(&self) -> impl Iterator<Item = &Page> {
        self.list.iter()
    }
}

fn page(root: &Path, source: PathBuf, section: Option<String>) -> anyhow::Result<Page> {
    let path = root.join(&source);
    let text = fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let markdown = for_site(&text).map_err(|line| {
        anyhow::anyhow!(
            "{}:{line}: {GITHUB_ONLY} has no {GITHUB_ONLY_END} after it",
            source.display()
        )
    })?;
    let Some(title) = render::title(&markdown) else {
        bail!("{} has no # heading to title it", source.display());
    };
    Ok(Page {
        url: url_of(&source),
        source,
        title,
        section,
        headings: render::headings(&markdown),
        markdown,
    })
}

const GITHUB_ONLY: &str = "<!-- github-only -->";
const GITHUB_ONLY_END: &str = "<!-- /github-only -->";

/// The Markdown without what sits between the github-only markers, each line of it left blank,
/// so a broken link's line number still points into the file. A part that never ends is refused
/// with the line it starts on, so it cannot drop the rest of the page.
fn for_site(markdown: &str) -> Result<String, usize> {
    let mut opened: Option<usize> = None;
    let mut out = String::with_capacity(markdown.len());
    for (index, line) in markdown.split_inclusive('\n').enumerate() {
        let marker = line.trim();
        if marker == GITHUB_ONLY && opened.is_none() {
            opened = Some(index + 1);
        }
        if opened.is_some() {
            out.push_str(if line.ends_with('\n') { "\n" } else { "" });
        } else {
            out.push_str(line);
        }
        if marker == GITHUB_ONLY_END {
            opened = None;
        }
    }
    match opened {
        Some(line) => Err(line),
        None => Ok(out),
    }
}

/// The Markdown files docs/README.md links, in the order it first links them, each with the
/// `##` heading it sits under.
fn listed(root: &Path) -> anyhow::Result<Vec<(PathBuf, Option<String>)>> {
    let path = root.join(INDEX);
    let markdown =
        fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let mut listed: Vec<(PathBuf, Option<String>)> = Vec::new();
    let mut section: Option<String> = None;
    let mut heading: Option<String> = None;
    for event in Parser::new_ext(&markdown, Options::ENABLE_TABLES) {
        match event {
            Event::Start(Tag::Heading {
                level: HeadingLevel::H2,
                ..
            }) => heading = Some(String::new()),
            Event::Text(text) | Event::Code(text) if heading.is_some() => {
                heading.get_or_insert_default().push_str(&text);
            }
            Event::End(TagEnd::Heading(HeadingLevel::H2)) => section = heading.take(),
            Event::Start(Tag::Link { dest_url, .. }) => {
                let file = dest_url.split('#').next().unwrap_or_default();
                let markdown_file = Path::new(file).extension().is_some_and(|ext| ext == "md");
                if file.contains(':') || !markdown_file {
                    continue;
                }
                let Some(source) = links::normal(&Path::new("docs").join(file)) else {
                    bail!("{INDEX} lists {file}, which is outside the repository");
                };
                if !root.join(&source).is_file() {
                    bail!("{INDEX} lists {file}, which does not exist");
                }
                let home = source == Path::new("README.md") || source == Path::new(INDEX);
                if !home && !listed.iter().any(|(listed, _)| *listed == source) {
                    listed.push((source, section.clone()));
                }
            }
            _ => {}
        }
    }
    Ok(listed)
}

/// The docs pages that are neither listed nor unpublished, sorted.
fn unlisted(root: &Path, listed: &[PathBuf]) -> anyhow::Result<Vec<PathBuf>> {
    let mut unlisted = Vec::new();
    for entry in fs::read_dir(root.join("docs")).context("reading docs/")? {
        let name = entry?.file_name();
        let source = Path::new("docs").join(&name);
        let published = !UNPUBLISHED.iter().any(|path| Path::new(path) == source);
        if source.extension().is_some_and(|ext| ext == "md")
            && source != Path::new(INDEX)
            && published
            && !listed.contains(&source)
        {
            unlisted.push(source);
        }
    }
    unlisted.sort();
    Ok(unlisted)
}

/// README.md -> "/", docs/README.md -> "/docs/", docs/ui.md -> "/docs/ui/",
/// CHANGELOG.md -> "/changelog/".
fn url_of(source: &Path) -> String {
    let folder = source
        .parent()
        .map(|parent| parent.to_string_lossy().into_owned())
        .unwrap_or_default();
    let stem = source
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let parts: Vec<&str> = [folder.as_str(), stem.as_str()]
        .into_iter()
        .filter(|part| !part.is_empty() && *part != "readme")
        .collect();
    if parts.is_empty() {
        "/".to_owned()
    } else {
        format!("/{}/", parts.join("/"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::checkout;

    #[test]
    fn serves_each_file_at_a_folder_url() {
        assert_eq!(url_of(Path::new("README.md")), "/");
        assert_eq!(url_of(Path::new("docs/README.md")), "/docs/");
        assert_eq!(url_of(Path::new("docs/ui.md")), "/docs/ui/");
        assert_eq!(url_of(Path::new("CHANGELOG.md")), "/changelog/");
        assert_eq!(url_of(Path::new("ROADMAP.md")), "/roadmap/");
    }

    const INDEX_WITH_SECTIONS: &str = "# Docs\n\n## Getting started\n\n| Page |\n| --- |\n| [b](b.md) |\n\n## Guides\n\n| Page |\n| --- |\n| [a](a.md#x) |\n| [b again](b.md) |\n\n## Project\n\n| Page |\n| --- |\n| [log](../CHANGELOG.md) |\n\nExample: [config](../examples/config.toml)\n";

    #[test]
    fn takes_pages_and_their_sections_from_the_index() {
        let repo = checkout(&[
            ("docs/README.md", INDEX_WITH_SECTIONS),
            ("docs/a.md", "# A\n"),
            ("docs/b.md", "# B\n"),
            ("docs/HANDOVER.md", "# Handover\n"),
            ("examples/config.toml", ""),
        ]);
        let pages = Pages::read(repo.path()).unwrap();
        let order: Vec<(&str, &str, Option<&str>)> = pages
            .iter()
            .map(|page| {
                (
                    page.url.as_str(),
                    page.title.as_str(),
                    page.section.as_deref(),
                )
            })
            .collect();
        assert_eq!(
            order,
            [
                ("/", "saaio", Some("Getting started")),
                ("/docs/", "Docs", None),
                ("/docs/b/", "B", Some("Getting started")),
                ("/docs/a/", "A", Some("Guides")),
                ("/changelog/", "Changelog", Some("Project")),
            ]
        );
        assert!(pages.by_source(Path::new("ROADMAP.md")).is_none());
        assert_eq!(
            pages.by_source(Path::new("docs/a.md")).unwrap().url,
            "/docs/a/"
        );
    }

    #[test]
    fn neighbours_follow_the_nav_and_skip_the_index() {
        let repo = checkout(&[
            ("docs/README.md", INDEX_WITH_SECTIONS),
            ("docs/a.md", "# A\n"),
            ("docs/b.md", "# B\n"),
        ]);
        let pages = Pages::read(repo.path()).unwrap();
        let titles = |source: &str| {
            let (before, after) = pages.neighbours(pages.by_source(Path::new(source)).unwrap());
            (
                before.map(|page| page.title.clone()),
                after.map(|page| page.title.clone()),
            )
        };
        assert_eq!(titles("README.md"), (None, Some("B".to_owned())));
        assert_eq!(
            titles("docs/a.md"),
            (Some("B".to_owned()), Some("Changelog".to_owned()))
        );
        assert_eq!(titles("CHANGELOG.md"), (Some("A".to_owned()), None));
        assert_eq!(titles("docs/README.md"), (None, None));
    }

    #[test]
    fn leaves_out_the_github_only_parts_but_keeps_their_lines() {
        let markdown = "# A\nseen\n<!-- github-only -->\n## Docs\nRead it here\n<!-- /github-only -->\nafter\n";
        assert_eq!(for_site(markdown).unwrap(), "# A\nseen\n\n\n\n\nafter\n");
        assert_eq!(for_site("no markers").unwrap(), "no markers");
    }

    #[test]
    fn refuses_a_github_only_part_that_never_ends() {
        let repo = checkout(&[
            ("docs/README.md", "# Docs\n\n[a](a.md)\n"),
            (
                "docs/a.md",
                "# A\n\n<!-- github-only -->\n## Docs\n\nThe rest\n",
            ),
        ]);
        let Err(error) = Pages::read(repo.path()) else {
            panic!("the unclosed github-only part was read");
        };
        assert!(format!("{error:#}").contains("docs/a.md:3: "), "{error:#}");
    }

    #[test]
    fn keeps_each_page_s_markdown_and_headings() {
        let repo = checkout(&[
            ("docs/README.md", "# Docs\n\n[a](a.md)\n"),
            (
                "docs/a.md",
                "# A\n\n## The `peek`\n\n<!-- github-only -->\n## Gone\n<!-- /github-only -->\n",
            ),
        ]);
        let pages = Pages::read(repo.path()).unwrap();
        let page = pages.by_source(Path::new("docs/a.md")).unwrap();
        assert!(!page.markdown.contains("Gone"));
        assert_eq!(
            page.headings,
            [
                ("a".to_owned(), "A".to_owned()),
                ("the-peek".to_owned(), "The peek".to_owned())
            ]
        );
    }

    #[test]
    fn refuses_a_docs_page_the_index_does_not_list() {
        let repo = checkout(&[
            ("docs/README.md", "# Docs\n\n[a](a.md)\n"),
            ("docs/a.md", "# A\n"),
            ("docs/new.md", "# New\n"),
        ]);
        let error = Pages::read(repo.path()).err().unwrap();
        assert_eq!(
            error.to_string(),
            "docs/new.md is not listed in docs/README.md"
        );
    }

    #[test]
    fn refuses_an_index_entry_without_its_file() {
        let repo = checkout(&[("docs/README.md", "# Docs\n\n[gone](gone.md)\n")]);
        let error = Pages::read(repo.path()).err().unwrap();
        assert_eq!(
            error.to_string(),
            "docs/README.md lists gone.md, which does not exist"
        );
    }

    #[test]
    fn refuses_a_page_without_a_title() {
        let repo = checkout(&[
            ("docs/README.md", "# Docs\n\n[a](a.md)\n"),
            ("docs/a.md", "no heading\n"),
        ]);
        let error = Pages::read(repo.path()).err().unwrap();
        assert_eq!(error.to_string(), "docs/a.md has no # heading to title it");
    }
}
