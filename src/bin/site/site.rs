use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Context;

use crate::layout;
use crate::links::{Broken, Reason, Target};
use crate::output::Output;
use crate::pages::{Page, Pages};
use crate::render::{self, Rendered};

pub struct Built {
    pub pages: usize,
    pub assets: usize,
}

pub enum Failure {
    Broken(Vec<Broken>),
    Io(anyhow::Error),
}

impl From<anyhow::Error> for Failure {
    fn from(error: anyhow::Error) -> Failure {
        Failure::Io(error)
    }
}

/// Reads the pages, renders them, checks every anchor, then writes the site. Nothing is written
/// while a link is broken.
pub fn build(root: &Path, out: &Path) -> Result<Built, Failure> {
    let pages = Pages::read(root)?;
    let mut rendered = Vec::new();
    let mut broken = Vec::new();
    for page in pages.iter() {
        let path = root.join(&page.source);
        let markdown =
            fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
        match render::render(page, &markdown, &pages, root) {
            Ok(done) => rendered.push((page, done)),
            Err(found) => broken.extend(found),
        }
    }
    broken.extend(missing_anchors(&rendered));
    if !broken.is_empty() {
        return Err(Failure::Broken(broken));
    }

    let output = Output::create(out)?;
    let mut assets: Vec<&PathBuf> = rendered.iter().flat_map(|(_, done)| &done.assets).collect();
    assets.sort();
    assets.dedup();
    for (page, done) in &rendered {
        output.page(&page.url, &layout::page(page, &pages, done))?;
    }
    for asset in &assets {
        output.copy(root, asset)?;
    }
    output.file("site.css", layout::CSS)?;
    output.file("site.js", layout::JS)?;
    output.file("404.html", &layout::not_found(&pages))?;
    Ok(Built {
        pages: rendered.len(),
        assets: assets.len(),
    })
}

/// The anchors a link names that its page does not have.
fn missing_anchors(rendered: &[(&Page, Rendered)]) -> Vec<Broken> {
    let mut broken = Vec::new();
    for (page, done) in rendered {
        for link in &done.links {
            let Target::Page {
                source,
                anchor: Some(anchor),
                ..
            } = &link.target
            else {
                continue;
            };
            let found = rendered
                .iter()
                .find(|(other, _)| other.source == *source)
                .is_some_and(|(_, other)| other.anchors.contains(anchor));
            if !found {
                broken.push(Broken {
                    source: page.source.clone(),
                    line: link.line,
                    href: link.href.clone(),
                    reason: Reason::NoAnchor(source.clone()),
                });
            }
        }
    }
    broken
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::checkout;

    fn broken(result: Result<Built, Failure>) -> Vec<String> {
        match result {
            Err(Failure::Broken(broken)) => broken.iter().map(ToString::to_string).collect(),
            Err(Failure::Io(error)) => panic!("{error:#}"),
            Ok(_) => panic!("built"),
        }
    }

    #[test]
    fn builds_every_page_its_images_and_the_frame() {
        let repo = checkout(&[
            ("docs/README.md", "# Docs\n\n[ui](ui.md)\n"),
            (
                "docs/ui.md",
                "# UI\n\n![shot](media/shot.png)\n\n[keys](#keys)\n\n## Keys\n",
            ),
            ("docs/media/shot.png", "png"),
            ("docs/media/unused.png", "png"),
        ]);
        let out = repo.path().join("_site");
        let built = build(repo.path(), &out).unwrap_or_else(|_| panic!());
        assert_eq!((built.pages, built.assets), (5, 1));
        for path in [
            "index.html",
            "docs/index.html",
            "docs/ui/index.html",
            "changelog/index.html",
            "roadmap/index.html",
            "docs/media/shot.png",
            "site.css",
            "site.js",
            "404.html",
        ] {
            assert!(out.join(path).is_file(), "{path}");
        }
        assert!(!out.join("docs/media/unused.png").exists());
    }

    #[test]
    fn names_every_missing_anchor_and_writes_nothing() {
        let repo = checkout(&[
            ("docs/README.md", "# Docs\n\n[ui](ui.md#nope)\n"),
            (
                "docs/ui.md",
                "# UI\n\n[here](#gone)\n\n[ok](README.md#docs)\n",
            ),
        ]);
        let out = repo.path().join("_site");
        assert_eq!(
            broken(build(repo.path(), &out)),
            [
                "docs/README.md:3: ui.md#nope: no heading #nope in docs/ui.md",
                "docs/ui.md:3: #gone: no heading #gone in docs/ui.md",
            ]
        );
        assert!(!out.exists());
    }

    #[test]
    fn names_broken_files_across_pages() {
        let repo = checkout(&[
            ("docs/README.md", "# Docs\n\n[ui](ui.md)\n"),
            ("docs/ui.md", "# UI\n\n[a](gone.md)\n"),
            ("ROADMAP.md", "# Roadmap\n\n[b](missing/)\n"),
        ]);
        assert_eq!(
            broken(build(repo.path(), &repo.path().join("_site"))),
            [
                "docs/ui.md:3: gone.md: no such file",
                "ROADMAP.md:3: missing/: no such file",
            ]
        );
    }

    #[test]
    fn the_repository_builds() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let out = tempfile::tempdir().unwrap();
        match build(root, &out.path().join("site")) {
            Ok(built) => assert!(built.pages > 10),
            Err(Failure::Broken(broken)) => {
                let lines: Vec<String> = broken.iter().map(ToString::to_string).collect();
                panic!("broken links in the docs:\n{}", lines.join("\n"));
            }
            Err(Failure::Io(error)) => panic!("{error:#}"),
        }
    }
}
