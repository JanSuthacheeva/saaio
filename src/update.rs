//! Updates: when the UI looks for a release again (`UpdateCheck`), its notice, `saaio update`'s `Step`

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::private;
use crate::release::{Build, Tag};

/// How long a look at the latest release holds.
pub const EVERY_MS: u64 = 24 * 60 * 60 * 1000;
/// The file in the shared folder that holds the last look.
pub const FILE: &str = "update.json";

/// The last look: when, and the latest tag it, or an earlier look, found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Checked {
    pub at: u64,
    pub latest: Option<Tag>,
}

/// update.json in the shared folder, which every UI on the machine shares.
#[derive(Debug, Clone)]
pub struct UpdateCheck {
    file: PathBuf,
}

impl UpdateCheck {
    pub fn in_folder(registry: &Path) -> UpdateCheck {
        UpdateCheck {
            file: registry.join(FILE),
        }
    }

    /// None when the file is missing or unreadable.
    pub fn last(&self) -> Option<Checked> {
        let text = fs::read_to_string(&self.file).ok()?;
        serde_json::from_str(&text).ok()
    }

    /// True when no look happened within EVERY_MS, or the last one is in the future.
    pub fn due(&self, now: u64) -> bool {
        self.last()
            .is_none_or(|last| now < last.at || now - last.at >= EVERY_MS)
    }

    /// Records a look at `now`; a look that found nothing keeps the tag an earlier one found.
    pub fn record(&self, now: u64, latest: Option<Tag>) -> io::Result<()> {
        let latest = latest.or_else(|| self.last().and_then(|last| last.latest));
        let checked = Checked { at: now, latest };
        if let Some(dir) = self.file.parent() {
            private::create_dir(dir)?;
        }
        let json = serde_json::to_vec(&checked).map_err(io::Error::other)?;
        private::write_atomic(&self.file, &json)
    }
}

/// `0.1.1 is out: saaio update` when `latest` is newer than the running build.
pub fn notice(running: &Build, latest: &Tag) -> Option<String> {
    (latest.version() > running.version())
        .then(|| format!("{} is out: saaio update", latest.version()))
}

/// What `saaio update` does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// The running build is the latest release, or newer.
    Latest,
    /// Built from source: cargo install updates it.
    Source,
    /// The binary's folder cannot be written.
    Unwritable,
    Install,
}

/// `asked` is true when --version named the target, which installs it even when older.
pub fn step(running: &Build, target: &Tag, asked: bool, writable: bool) -> Step {
    if matches!(running, Build::Source(_)) {
        Step::Source
    } else if !asked && target.version() <= running.version() {
        Step::Latest
    } else if !writable {
        Step::Unwritable
    } else {
        Step::Install
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use tempfile::TempDir;

    use super::*;

    fn tag(text: &str) -> Tag {
        Tag::parse(text).unwrap()
    }

    fn release(text: &str) -> Build {
        Build::Release(tag(text))
    }

    #[test]
    fn a_look_holds_for_a_day() {
        let dir = TempDir::new().unwrap();
        let check = UpdateCheck::in_folder(&dir.path().join("saaio"));
        assert!(check.due(1_000));

        check.record(1_000, Some(tag("v0.1.1"))).unwrap();

        assert!(!check.due(1_000 + EVERY_MS - 1));
        assert!(check.due(1_000 + EVERY_MS));
        assert!(check.due(999), "a clock set back looks again");
        let file = dir.path().join("saaio/update.json");
        assert_eq!(
            fs::read_to_string(&file).unwrap(),
            r#"{"at":1000,"latest":"v0.1.1"}"#
        );
        assert_eq!(
            fs::metadata(&file).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[test]
    fn a_look_that_found_nothing_keeps_the_tag_found_before() {
        let dir = TempDir::new().unwrap();
        let check = UpdateCheck::in_folder(dir.path());
        check.record(1_000, Some(tag("v0.1.1"))).unwrap();

        check.record(2_000, None).unwrap();

        assert_eq!(
            check.last(),
            Some(Checked {
                at: 2_000,
                latest: Some(tag("v0.1.1"))
            })
        );
        fs::write(dir.path().join(FILE), "{").unwrap();
        assert_eq!(check.last(), None);
        assert!(check.due(2_000));
    }

    #[test]
    fn tells_of_a_newer_release_only() {
        assert_eq!(
            notice(&release("v0.1.0"), &tag("v0.1.1")).as_deref(),
            Some("0.1.1 is out: saaio update")
        );
        assert_eq!(
            notice(&release("v0.1.1-rc.1"), &tag("v0.1.1")).as_deref(),
            Some("0.1.1 is out: saaio update")
        );
        assert_eq!(notice(&release("v0.1.1"), &tag("v0.1.1")), None);
        assert_eq!(notice(&release("v0.1.2-rc.1"), &tag("v0.1.1")), None);
        let source = Build::Source(semver::Version::new(0, 1, 0));
        assert!(notice(&source, &tag("v0.1.1")).is_some());
    }

    #[test]
    fn installs_a_newer_or_asked_release_into_a_folder_it_can_write() {
        let running = release("v0.1.1");
        let source = Build::Source(semver::Version::new(0, 1, 0));

        assert_eq!(step(&running, &tag("v0.1.2"), false, true), Step::Install);
        assert_eq!(step(&running, &tag("v0.1.1"), false, true), Step::Latest);
        assert_eq!(step(&running, &tag("v0.1.0"), false, true), Step::Latest);
        assert_eq!(step(&running, &tag("v0.1.0"), true, true), Step::Install);
        assert_eq!(
            step(&running, &tag("v0.1.2"), false, false),
            Step::Unwritable
        );
        assert_eq!(step(&running, &tag("v0.1.1"), false, false), Step::Latest);
        assert_eq!(step(&source, &tag("v0.1.2"), false, true), Step::Source);
        assert_eq!(step(&source, &tag("v0.1.2"), true, true), Step::Source);
    }
}
