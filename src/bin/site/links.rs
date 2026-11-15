use std::borrow::Cow;
use std::fmt;
use std::path::{Component, Path, PathBuf};

use crate::pages::Pages;

pub const REPO: &str = "https://github.com/JanSuthacheeva/saaio";

const IMAGES: &[&str] = &["png", "gif", "jpg", "jpeg", "svg", "webp"];

/// Where one Markdown link points on the site.
#[derive(Debug, PartialEq)]
pub enum Target {
    /// A published page: its url plus "#anchor".
    Page {
        source: PathBuf,
        url: String,
        anchor: Option<String>,
    },
    /// An image in the repository, linked or shown, copied to the same path on the site.
    Asset { source: PathBuf },
    /// Any other file or folder of the repository, on GitHub.
    Repo { url: String },
    /// http(s), mailto: and the like, as written.
    External(String),
}

impl Target {
    pub fn href(&self) -> Cow<'_, str> {
        match self {
            Target::Page {
                url,
                anchor: Some(anchor),
                ..
            } => Cow::Owned(format!("{url}#{anchor}")),
            Target::Page { url, .. } | Target::Repo { url } | Target::External(url) => {
                Cow::Borrowed(url)
            }
            Target::Asset { source } => Cow::Owned(format!("/{}", web_path(source))),
        }
    }
}

/// A link of a page into another page, kept to check its anchor once every page rendered.
pub struct Link {
    pub line: usize,
    pub href: String,
    pub target: Target,
}

/// A link the site cannot serve.
#[derive(Debug, PartialEq)]
pub struct Broken {
    pub source: PathBuf,
    pub line: usize,
    pub href: String,
    pub reason: Reason,
}

#[derive(Debug, PartialEq)]
pub enum Reason {
    NoFile,
    /// The page it names has no heading with that id.
    NoAnchor(PathBuf),
    OutsideRepo,
}

impl fmt::Display for Broken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let place = format!("{}:{}: {}", self.source.display(), self.line, self.href);
        match &self.reason {
            Reason::NoFile => write!(f, "{place}: no such file"),
            Reason::OutsideRepo => write!(f, "{place}: leaves the repository"),
            Reason::NoAnchor(page) => {
                let anchor = self.href.split_once('#').map_or("", |(_, anchor)| anchor);
                write!(f, "{place}: no heading #{anchor} in {}", page.display())
            }
        }
    }
}

/// Resolves href against the folder of `from`, a page's path in the checkout. "#x" is the page
/// itself; "/x" starts at the checkout, as on GitHub.
pub fn resolve(from: &Path, href: &str, pages: &Pages, root: &Path) -> Result<Target, Reason> {
    if is_external(href) {
        return Ok(Target::External(href.to_owned()));
    }
    let (path, anchor) = match href.split_once('#') {
        Some((path, anchor)) => (path, Some(anchor.to_owned())),
        None => (href, None),
    };
    let source = if path.is_empty() {
        from.to_path_buf()
    } else if let Some(absolute) = path.strip_prefix('/') {
        normal(Path::new(absolute)).ok_or(Reason::OutsideRepo)?
    } else {
        let folder = from.parent().unwrap_or(Path::new(""));
        normal(&folder.join(path)).ok_or(Reason::OutsideRepo)?
    };
    if let Some(page) = pages.by_source(&source) {
        return Ok(Target::Page {
            source,
            url: page.url.clone(),
            anchor: anchor.filter(|anchor| !anchor.is_empty()),
        });
    }
    let full = root.join(&source);
    if !full.exists() {
        return Err(Reason::NoFile);
    }
    let image = source
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| IMAGES.contains(&ext.to_ascii_lowercase().as_str()));
    if image {
        return Ok(Target::Asset { source });
    }
    let kind = if full.is_dir() { "tree" } else { "blob" };
    let fragment = anchor
        .map(|anchor| format!("#{anchor}"))
        .unwrap_or_default();
    Ok(Target::Repo {
        url: format!("{REPO}/{kind}/main/{}{fragment}", web_path(&source)),
    })
}

/// A scheme before any slash: https:, mailto:.
fn is_external(href: &str) -> bool {
    href.split_once(':')
        .is_some_and(|(scheme, _)| !scheme.is_empty() && !scheme.contains(['/', '#', '.']))
}

/// The path without "." and "..", or None when it climbs above the checkout.
pub fn normal(path: &Path) -> Option<PathBuf> {
    let mut parts: Vec<&std::ffi::OsStr> = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => parts.push(part),
            Component::CurDir => {}
            Component::ParentDir => {
                parts.pop()?;
            }
            Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    Some(parts.iter().collect())
}

fn web_path(source: &Path) -> String {
    let parts: Vec<String> = source
        .components()
        .map(|part| part.as_os_str().to_string_lossy().into_owned())
        .collect();
    parts.join("/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::checkout;

    fn resolved(from: &str, href: &str) -> Result<Target, Reason> {
        let repo = checkout(&[
            (
                "docs/README.md",
                "# Docs\n\n[ui](ui.md) [log](../CHANGELOG.md)\n",
            ),
            ("docs/ui.md", "# UI\n"),
            ("docs/media/shot.png", ""),
            ("docs/media/record.sh", ""),
            ("examples/config.toml", ""),
        ]);
        let pages = Pages::read(repo.path()).unwrap();
        resolve(Path::new(from), href, &pages, repo.path())
    }

    fn page(source: &str, url: &str, anchor: Option<&str>) -> Target {
        Target::Page {
            source: PathBuf::from(source),
            url: url.to_owned(),
            anchor: anchor.map(str::to_owned),
        }
    }

    #[test]
    fn a_page_link_goes_to_its_url_and_anchor() {
        let target = resolved("docs/README.md", "ui.md#keys").unwrap();
        assert_eq!(target, page("docs/ui.md", "/docs/ui/", Some("keys")));
        assert_eq!(target.href(), "/docs/ui/#keys");
        assert_eq!(
            resolved("README.md", "docs/ui.md").unwrap().href(),
            "/docs/ui/"
        );
        assert_eq!(
            resolved("docs/ui.md", "../CHANGELOG.md").unwrap().href(),
            "/changelog/"
        );
        assert_eq!(resolved("docs/ui.md", "/README.md").unwrap().href(), "/");
    }

    #[test]
    fn an_anchor_alone_is_the_page_itself() {
        let target = resolved("docs/ui.md", "#the-peek").unwrap();
        assert_eq!(target, page("docs/ui.md", "/docs/ui/", Some("the-peek")));
    }

    #[test]
    fn an_image_is_an_asset_whether_shown_or_linked() {
        let target = resolved("docs/ui.md", "media/shot.png").unwrap();
        assert_eq!(target.href(), "/docs/media/shot.png");
        assert_eq!(
            resolved("README.md", "./docs/media/shot.png").unwrap(),
            Target::Asset {
                source: PathBuf::from("docs/media/shot.png")
            }
        );
    }

    #[test]
    fn another_repository_file_or_folder_goes_to_github() {
        assert_eq!(
            resolved("docs/ui.md", "../examples/config.toml")
                .unwrap()
                .href(),
            "https://github.com/JanSuthacheeva/saaio/blob/main/examples/config.toml"
        );
        assert_eq!(
            resolved("docs/ui.md", "media/record.sh#L3").unwrap().href(),
            "https://github.com/JanSuthacheeva/saaio/blob/main/docs/media/record.sh#L3"
        );
        assert_eq!(
            resolved("README.md", "examples").unwrap().href(),
            "https://github.com/JanSuthacheeva/saaio/tree/main/examples"
        );
    }

    #[test]
    fn an_external_link_stays_as_written() {
        for href in ["https://saaio.dev", "mailto:a@b.c", "http://x/y.md"] {
            assert_eq!(resolved("docs/ui.md", href).unwrap().href(), href);
        }
    }

    #[test]
    fn a_missing_file_or_a_path_out_of_the_checkout_is_broken() {
        assert_eq!(resolved("docs/ui.md", "gone.md"), Err(Reason::NoFile));
        assert_eq!(
            resolved("docs/ui.md", "../../x.md"),
            Err(Reason::OutsideRepo)
        );
    }

    #[test]
    fn says_where_a_link_breaks() {
        let broken = Broken {
            source: PathBuf::from("docs/ui.md"),
            line: 12,
            href: "flows.md#nope".to_owned(),
            reason: Reason::NoAnchor(PathBuf::from("docs/flows.md")),
        };
        assert_eq!(
            broken.to_string(),
            "docs/ui.md:12: flows.md#nope: no heading #nope in docs/flows.md"
        );
        let broken = Broken {
            reason: Reason::NoFile,
            ..broken
        };
        assert_eq!(
            broken.to_string(),
            "docs/ui.md:12: flows.md#nope: no such file"
        );
    }
}
