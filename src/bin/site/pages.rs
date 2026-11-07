use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use pulldown_cmark::{Event, Parser, Tag};

use crate::render;

/// One Markdown file the site publishes.
pub struct Page {
    /// Relative to the checkout: docs/ui.md.
    pub source: PathBuf,
    /// Where the site serves it: /docs/ui/.
    pub url: String,
    /// Its first # heading.
    pub title: String,
}

/// The pages the site publishes, in nav order.
pub struct Pages {
    list: Vec<Page>,
}

/// Files in docs/ that are for contributors, not pages.
const UNPUBLISHED: &[&str] = &["docs/HANDOVER.md"];

const INDEX: &str = "docs/README.md";

impl Pages {
    /// README.md, docs/README.md, the docs pages in the order docs/README.md links them, then
    /// CHANGELOG.md and ROADMAP.md. Errors when a docs page is missing from docs/README.md.
    pub fn read(root: &Path) -> anyhow::Result<Pages> {
        let mut sources = vec![PathBuf::from("README.md"), PathBuf::from(INDEX)];
        let listed = listed(root)?;
        let unlisted = unlisted(root, &listed)?;
        if !unlisted.is_empty() {
            let names: Vec<String> = unlisted
                .iter()
                .map(|source| source.display().to_string())
                .collect();
            let verb = if names.len() == 1 { "is" } else { "are" };
            bail!("{} {verb} not listed in {INDEX}", names.join(", "));
        }
        sources.extend(listed);
        sources.extend(["CHANGELOG.md", "ROADMAP.md"].map(PathBuf::from));
        let list = sources
            .into_iter()
            .map(|source| page(root, source))
            .collect::<anyhow::Result<_>>()?;
        Ok(Pages { list })
    }

    pub fn by_source(&self, source: &Path) -> Option<&Page> {
        self.list.iter().find(|page| page.source == source)
    }

    pub fn iter(&self) -> impl Iterator<Item = &Page> {
        self.list.iter()
    }
}

fn page(root: &Path, source: PathBuf) -> anyhow::Result<Page> {
    let path = root.join(&source);
    let markdown =
        fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let Some(title) = render::title(&markdown) else {
        bail!("{} has no # heading to title it", source.display());
    };
    Ok(Page {
        url: url_of(&source),
        source,
        title,
    })
}

/// The docs pages docs/README.md links, in the order it first links them.
fn listed(root: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let path = root.join(INDEX);
    let markdown =
        fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let mut listed: Vec<PathBuf> = Vec::new();
    for event in Parser::new(&markdown) {
        let Event::Start(Tag::Link { dest_url, .. }) = event else {
            continue;
        };
        let file = dest_url.split('#').next().unwrap_or_default();
        if file.contains('/') || Path::new(file).extension().is_none_or(|ext| ext != "md") {
            continue;
        }
        let source = Path::new("docs").join(file);
        if !root.join(&source).is_file() {
            bail!("{INDEX} lists {file}, which does not exist");
        }
        if !listed.contains(&source) {
            listed.push(source);
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

    #[test]
    fn orders_the_docs_pages_as_the_index_lists_them() {
        let repo = checkout(&[
            (
                "docs/README.md",
                "# Docs\n\n[b](b.md), [a](a.md#x), [b again](b.md), [log](../CHANGELOG.md)\n",
            ),
            ("docs/a.md", "# A\n"),
            ("docs/b.md", "# B\n"),
            ("docs/HANDOVER.md", "# Handover\n"),
        ]);
        let pages = Pages::read(repo.path()).unwrap();
        let order: Vec<(&str, &str)> = pages
            .iter()
            .map(|page| (page.url.as_str(), page.title.as_str()))
            .collect();
        assert_eq!(
            order,
            [
                ("/", "saaio"),
                ("/docs/", "Docs"),
                ("/docs/b/", "B"),
                ("/docs/a/", "A"),
                ("/changelog/", "Changelog"),
                ("/roadmap/", "Roadmap"),
            ]
        );
        assert_eq!(
            pages.by_source(Path::new("docs/a.md")).unwrap().url,
            "/docs/a/"
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
