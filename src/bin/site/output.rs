#![allow(
    clippy::disallowed_methods,
    reason = "the site is public output, not the user's private files"
)]

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};

/// The file an earlier build leaves, so a rebuild may empty its folder.
const MARKER: &str = ".saaio-site";

/// The folder the site is written to.
pub struct Output {
    dir: PathBuf,
}

impl Output {
    /// Empties dir, or creates it. Refuses a folder that holds anything but an earlier build,
    /// so a wrong path cannot wipe a checkout.
    pub fn create(dir: &Path) -> anyhow::Result<Output> {
        if dir.exists() {
            let empty = fs::read_dir(dir)
                .with_context(|| format!("reading {}", dir.display()))?
                .next()
                .is_none();
            if !empty && !dir.join(MARKER).is_file() {
                bail!(
                    "{} is not empty and holds no earlier build; pick another folder",
                    dir.display()
                );
            }
            fs::remove_dir_all(dir).with_context(|| format!("emptying {}", dir.display()))?;
        }
        fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        let output = Output {
            dir: dir.to_path_buf(),
        };
        output.file(MARKER, "")?;
        Ok(output)
    }

    /// "/docs/ui/" -> dir/docs/ui/index.html
    pub fn page(&self, url: &str, html: &str) -> anyhow::Result<()> {
        self.file(&format!("{}index.html", url.trim_start_matches('/')), html)
    }

    pub fn file(&self, path: &str, text: &str) -> anyhow::Result<()> {
        let path = self.dir.join(path);
        create_parent(&path)?;
        fs::write(&path, text).with_context(|| format!("writing {}", path.display()))
    }

    /// Copies a file of the checkout to the same path in the site.
    pub fn copy(&self, root: &Path, source: &Path) -> anyhow::Result<()> {
        let path = self.dir.join(source);
        create_parent(&path)?;
        fs::copy(root.join(source), &path)
            .with_context(|| format!("copying {}", source.display()))?;
        Ok(())
    }
}

fn create_parent(path: &Path) -> anyhow::Result<()> {
    let parent = path.parent().unwrap_or(Path::new(""));
    fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::checkout;

    #[test]
    fn writes_pages_files_and_copies() {
        let repo = checkout(&[("docs/media/shot.png", "png")]);
        let out = repo.path().join("_site");
        let output = Output::create(&out).unwrap();
        output.page("/", "home").unwrap();
        output.page("/docs/ui/", "ui").unwrap();
        output.file("site.css", "css").unwrap();
        output
            .copy(repo.path(), Path::new("docs/media/shot.png"))
            .unwrap();
        let read = |path: &str| fs::read_to_string(out.join(path)).unwrap();
        assert_eq!(read("index.html"), "home");
        assert_eq!(read("docs/ui/index.html"), "ui");
        assert_eq!(read("site.css"), "css");
        assert_eq!(read("docs/media/shot.png"), "png");
        assert!(out.join(MARKER).is_file());
    }

    #[test]
    fn replaces_an_earlier_build() {
        let repo = checkout(&[]);
        let out = repo.path().join("_site");
        Output::create(&out)
            .unwrap()
            .file("old.html", "old")
            .unwrap();
        Output::create(&out).unwrap();
        assert!(!out.join("old.html").exists());
        assert!(out.join(MARKER).is_file());
    }

    #[test]
    fn refuses_a_folder_that_holds_something_else() {
        let repo = checkout(&[]);
        let error = Output::create(repo.path()).err().unwrap();
        assert!(
            error.to_string().contains("holds no earlier build"),
            "{error}"
        );
        assert!(repo.path().join("README.md").is_file());
    }

    #[test]
    fn takes_an_empty_folder() {
        let dir = tempfile::tempdir().unwrap();
        Output::create(dir.path()).unwrap();
        assert!(dir.path().join(MARKER).is_file());
    }
}
