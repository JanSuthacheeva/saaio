//! `Releases`: where releases are read, GitHub's releases page or a mirror of it, through curl

use std::time::Duration;

use anyhow::{Context, Result};

use super::Tag;
use crate::workspace::process;

/// saaio's releases page.
pub const GITHUB: &str = "https://github.com/JanSuthacheeva/saaio/releases";

/// What curl gets beyond its own --max-time before it is killed.
const GRACE: Duration = Duration::from_secs(1);

/// A releases page: `latest/download/<asset>` is the latest release's, `download/<tag>/<asset>`
/// a tag's, as on GitHub.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Releases {
    root: String,
}

impl Releases {
    /// `root` ($SAAIO_RELEASES_URL), else GITHUB.
    pub fn at(root: Option<String>) -> Releases {
        let root = root
            .map(|root| root.trim().trim_end_matches('/').to_string())
            .filter(|root| !root.is_empty())
            .unwrap_or_else(|| GITHUB.to_string());
        Releases { root }
    }

    /// The releases page itself.
    pub fn page(&self) -> &str {
        &self.root
    }

    /// The latest release's `asset` without a tag, else that tag's.
    pub fn url(&self, tag: Option<&Tag>, asset: &str) -> String {
        match tag {
            None => format!("{}/latest/download/{asset}", self.root),
            Some(tag) => format!("{}/download/{tag}/{asset}", self.root),
        }
    }

    /// The tag the latest release's VERSION holds. GitHub's latest is never a pre-release.
    pub fn latest(&self, limit: Duration) -> Result<Tag> {
        let text = self.curl(&self.url(None, "VERSION"), limit)?;
        Tag::parse(text.lines().next().unwrap_or(""))
    }

    /// The text of a release's `asset`.
    pub fn fetch(&self, tag: &Tag, asset: &str, limit: Duration) -> Result<String> {
        self.curl(&self.url(Some(tag), asset), limit)
    }

    /// What install.sh needs to install `tag` from here: SAAIO_VERSION, and SAAIO_DOWNLOAD_URL
    /// when this is not GITHUB.
    pub fn install_env(&self, tag: &Tag) -> Vec<(String, String)> {
        let mut env = vec![("SAAIO_VERSION".to_string(), tag.to_string())];
        if !self.is_github() {
            env.push((
                "SAAIO_DOWNLOAD_URL".to_string(),
                format!("{}/download/{tag}", self.root),
            ));
        }
        env
    }

    fn is_github(&self) -> bool {
        self.root == GITHUB
    }

    /// What `url` holds; an error with curl's message when it cannot be read within `limit`.
    fn curl(&self, url: &str, limit: Duration) -> Result<String> {
        let mut argv: Vec<String> = ["curl", "-fsSL", "--connect-timeout", "3", "--max-time"]
            .map(String::from)
            .to_vec();
        argv.push(limit.as_secs().max(1).to_string());
        // As install.sh does: GitHub only over https, a mirror as it is given.
        if self.is_github() {
            argv.extend(["--proto", "=https", "--tlsv1.2"].map(String::from));
        }
        argv.push(url.to_string());
        process::output(&argv, limit + GRACE).with_context(|| format!("could not read {url}"))
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use tempfile::TempDir;

    use super::*;

    const LIMIT: Duration = Duration::from_secs(5);

    fn tag(text: &str) -> Tag {
        Tag::parse(text).unwrap()
    }

    /// A folder laid out like a releases page, its latest release `latest`.
    fn mirror(dir: &Path, latest: &str) -> Releases {
        let folder = dir.join("latest/download");
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join("VERSION"), format!("{latest}\n")).unwrap();
        let tagged = dir.join("download").join(latest);
        fs::create_dir_all(&tagged).unwrap();
        fs::write(tagged.join("install.sh"), "echo installing\n").unwrap();
        Releases::at(Some(format!("file://{}/", dir.display())))
    }

    #[test]
    fn names_the_latest_release_or_a_tag() {
        let github = Releases::at(None);

        assert_eq!(
            github.url(None, "VERSION"),
            "https://github.com/JanSuthacheeva/saaio/releases/latest/download/VERSION"
        );
        assert_eq!(
            github.url(Some(&tag("v0.1.1")), "install.sh"),
            "https://github.com/JanSuthacheeva/saaio/releases/download/v0.1.1/install.sh"
        );
        assert_eq!(Releases::at(Some(" ".to_string())), github);
    }

    #[test]
    fn install_sh_reads_a_mirror_from_the_tag_folder() {
        let tag = tag("v0.1.1");

        assert_eq!(
            Releases::at(None).install_env(&tag),
            vec![("SAAIO_VERSION".to_string(), "v0.1.1".to_string())]
        );
        assert_eq!(
            Releases::at(Some("file:///mirror/".to_string())).install_env(&tag),
            vec![
                ("SAAIO_VERSION".to_string(), "v0.1.1".to_string()),
                (
                    "SAAIO_DOWNLOAD_URL".to_string(),
                    "file:///mirror/download/v0.1.1".to_string()
                ),
            ]
        );
    }

    #[test]
    fn reads_the_latest_tag_and_an_asset_from_a_mirror() {
        let dir = TempDir::new().unwrap();
        let releases = mirror(dir.path(), "v0.1.1");

        assert_eq!(releases.latest(LIMIT).unwrap(), tag("v0.1.1"));
        assert_eq!(
            releases.fetch(&tag("v0.1.1"), "install.sh", LIMIT).unwrap(),
            "echo installing\n"
        );
        let error = releases
            .fetch(&tag("v0.1.2"), "install.sh", LIMIT)
            .unwrap_err();
        assert!(
            format!("{error:#}").starts_with(&format!(
                "could not read file://{}/download/v0.1.2/install.sh: ",
                dir.path().display()
            )),
            "{error:#}"
        );
    }

    #[test]
    fn a_release_without_a_tag_in_version_fails() {
        let dir = TempDir::new().unwrap();
        let releases = mirror(dir.path(), "nonsense");

        assert_eq!(
            format!("{:#}", releases.latest(LIMIT).unwrap_err()),
            "not a release tag: nonsense"
        );
        let empty = TempDir::new().unwrap();
        assert!(
            Releases::at(Some(format!("file://{}", empty.path().display())))
                .latest(LIMIT)
                .is_err()
        );
    }
}
