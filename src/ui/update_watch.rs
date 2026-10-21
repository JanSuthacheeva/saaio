//! `UpdateWatch`: the UI's look at the latest release, once a day while it is turned on

use std::sync::mpsc::Sender;
use std::thread;
use std::time::Duration;

use super::Event;
use crate::release::source::Releases;
use crate::release::{Build, Tag};
use crate::update::{self, UpdateCheck};

/// How long curl gets to name the latest release.
const LIMIT: Duration = Duration::from_secs(5);

/// Asks for the latest release when a day passed since any UI on the machine did, and names a
/// newer one.
#[derive(Debug)]
pub struct UpdateWatch {
    /// None without a shared folder: then it never asks.
    check: Option<UpdateCheck>,
    releases: Releases,
    running: Build,
    /// `[ui] update_check`.
    enabled: bool,
    /// A look is out, its answer not back yet.
    asking: bool,
}

impl UpdateWatch {
    /// Off until set_enabled turns it on.
    pub fn new(check: Option<UpdateCheck>, releases: Releases, running: Build) -> UpdateWatch {
        UpdateWatch {
            check,
            releases,
            running,
            enabled: false,
            asking: false,
        }
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// When on and due: records the look, so no other UI asks too, and asks curl on a thread
    /// of its own. The answer comes back as Event::Latest, None when there was none.
    pub fn ask_if_due(&mut self, now: u64, events: &Sender<Event>) {
        let Some(check) = self.check.as_ref().filter(|_| self.enabled && !self.asking) else {
            return;
        };
        if !check.due(now) || check.record(now, None).is_err() {
            return;
        }
        self.asking = true;
        let releases = self.releases.clone();
        let events = events.clone();
        thread::spawn(move || {
            let _ = events.send(Event::Latest(releases.latest(LIMIT).ok()));
        });
    }

    /// Records what the look found; a look that found nothing keeps the earlier answer.
    pub fn answered(&mut self, now: u64, latest: Option<Tag>) {
        self.asking = false;
        if let (Some(check), Some(latest)) = (&self.check, latest) {
            let _ = check.record(now, Some(latest));
        }
    }

    /// `0.1.1 is out: saaio update` when on and the last look found a newer release.
    pub fn notice(&self) -> Option<String> {
        let check = self.check.as_ref().filter(|_| self.enabled)?;
        update::notice(&self.running, &check.last()?.latest?)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;
    use std::sync::mpsc;

    use tempfile::TempDir;

    use super::*;

    const WAIT: Duration = Duration::from_secs(30);

    fn tag(text: &str) -> Tag {
        Tag::parse(text).unwrap()
    }

    /// A watch of a v0.1.0 build, its shared folder `dir`, reading a releases page in `dir`
    /// whose latest is `latest`.
    fn watch(dir: &Path, latest: &str) -> UpdateWatch {
        let page = dir.join("releases/latest/download");
        fs::create_dir_all(&page).unwrap();
        fs::write(page.join("VERSION"), format!("{latest}\n")).unwrap();
        let releases = Releases::at(Some(format!("file://{}/releases", dir.display())));
        let mut watch = UpdateWatch::new(
            Some(UpdateCheck::in_folder(dir)),
            releases,
            Build::Release(tag("v0.1.0")),
        );
        watch.set_enabled(true);
        watch
    }

    #[test]
    fn asks_once_a_day_and_names_the_newer_release() {
        let dir = TempDir::new().unwrap();
        let mut watch = watch(dir.path(), "v0.1.1");
        let (events, inbox) = mpsc::channel();

        watch.ask_if_due(1_000, &events);
        let Ok(Event::Latest(latest)) = inbox.recv_timeout(WAIT) else {
            panic!("no answer");
        };
        assert_eq!(latest, Some(tag("v0.1.1")));
        watch.answered(1_000, latest);

        assert_eq!(
            watch.notice().as_deref(),
            Some("0.1.1 is out: saaio update")
        );
        watch.ask_if_due(2_000, &events);
        assert!(inbox.recv_timeout(Duration::from_millis(200)).is_err());
    }

    #[test]
    fn a_look_that_found_nothing_keeps_the_last_answer() {
        let dir = TempDir::new().unwrap();
        let mut watch = watch(dir.path(), "v0.1.1");
        watch.answered(1_000, Some(tag("v0.1.1")));

        watch.answered(1_000 + update::EVERY_MS, None);

        assert_eq!(
            watch.notice().as_deref(),
            Some("0.1.1 is out: saaio update")
        );
    }

    #[test]
    fn turned_off_it_neither_asks_nor_tells() {
        let dir = TempDir::new().unwrap();
        let mut watch = watch(dir.path(), "v0.1.1");
        watch.answered(1_000, Some(tag("v0.1.1")));
        watch.set_enabled(false);
        let (events, inbox) = mpsc::channel();

        watch.ask_if_due(1_000 + update::EVERY_MS, &events);

        assert_eq!(watch.notice(), None);
        assert!(inbox.recv_timeout(Duration::from_millis(200)).is_err());
    }
}
