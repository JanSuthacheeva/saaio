//! What `saaio update` prints: the update, the latest release already running, or why it cannot

use std::path::{Path, PathBuf};

use serde::Serialize;

use super::{Document, Failure, toon};
use crate::binaries::Binary;
use crate::client::Running;
use crate::release::source::{GITHUB, Releases};
use crate::release::{Build, Tag};

/// What update finds beside its own install: the saaio first on PATH and the servers on another
/// version; paths written with ~.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Others {
    pub shadowed: Option<Binary>,
    /// Each server's store root and who listens there.
    pub servers: Vec<(PathBuf, Running)>,
}

/// One row of servers[].
#[derive(Serialize)]
struct Server {
    store: String,
    pid: u32,
    build: String,
    binary: String,
}

impl Others {
    /// `~/.cargo/bin/saaio (saaio 0.1.0 (9b77d8c))`.
    fn shadowed(&self) -> Option<String> {
        self.shadowed
            .as_ref()
            .map(|other| format!("{} (saaio {})", other.exe.display(), other.build))
    }

    fn rows(&self) -> Vec<Server> {
        self.servers
            .iter()
            .map(|(store, running)| Server {
                store: store.display().to_string(),
                pid: running.pid,
                build: running.server.as_ref().map_or_else(
                    || "an earlier protocol".to_string(),
                    |server| server.build.clone(),
                ),
                binary: running.server.as_ref().map_or_else(
                    || "unknown".to_string(),
                    |server| server.exe.display().to_string(),
                ),
            })
            .collect()
    }

    /// What to do about them: `binary` is the saaio update installed or found.
    fn help(&self, binary: &str) -> Vec<String> {
        let shadowed = self.shadowed.as_ref().map(|other| {
            let remove = if other
                .exe
                .parent()
                .is_some_and(|dir| dir.ends_with(".cargo/bin"))
            {
                "remove it (`cargo uninstall saaio`)"
            } else {
                "remove it"
            };
            let folder = Path::new(binary)
                .parent()
                .map_or_else(|| binary.to_string(), |dir| dir.display().to_string());
            format!(
                "`saaio` runs {}, not {binary}: {remove} or put {folder} first on PATH",
                other.exe.display()
            )
        });
        let servers = match self.servers.as_slice() {
            [] => None,
            [(store, _)] => Some(format!(
                "Run `SAAIO_HOME={} saaio --stop`, then start saaio again; unfinished flows resume",
                store.display()
            )),
            _ => Some(
                "Run `SAAIO_HOME=<store> saaio --stop` for each server, then start saaio again; unfinished flows resume"
                    .to_string(),
            ),
        };
        shadowed.into_iter().chain(servers).collect()
    }
}

/// updated, binary, shadowed and servers[] when there are, help: what to do about them.
pub(crate) fn updated(from: &Build, to: &Tag, binary: &str, others: &Others) -> Document {
    #[derive(Serialize)]
    struct Updated<'a> {
        updated: String,
        binary: &'a str,
        #[serde(skip_serializing_if = "Option::is_none")]
        shadowed: Option<String>,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        servers: Vec<Server>,
        help: Vec<String>,
    }
    let mut help = others.help(binary);
    help.push(format!("Changes: {GITHUB}/tag/{to}"));
    Document::Toon(toon(&Updated {
        updated: format!("saaio {} -> {}", from.version(), to.version()),
        binary,
        shadowed: others.shadowed(),
        servers: others.rows(),
        help,
    }))
}

/// running, latest, update: none, shadowed and servers[] when there are.
pub(crate) fn latest(running: &Build, latest: &Tag, binary: &str, others: &Others) -> Document {
    #[derive(Serialize)]
    struct Latest {
        running: String,
        latest: String,
        update: &'static str,
        #[serde(skip_serializing_if = "Option::is_none")]
        shadowed: Option<String>,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        servers: Vec<Server>,
        help: Vec<String>,
    }
    let mut help = others.help(binary);
    help.push("Run `saaio update --version <tag>` to install another release".to_string());
    Document::Toon(toon(&Latest {
        running: format!("v{}", running.version()),
        latest: latest.to_string(),
        update: "none",
        shadowed: others.shadowed(),
        servers: others.rows(),
        help,
    }))
}

/// A build from source is updated by building again.
pub(crate) fn source(to: &Tag) -> Failure {
    Failure::error(
        "this saaio was built from source, so saaio update does not replace it",
        vec![
            format!(
                "Run `cargo install --locked --git https://github.com/JanSuthacheeva/saaio --tag {to} --force`"
            ),
            "Or install the release binary instead: docs/install.md".to_string(),
        ],
    )
}

/// `to` could not be installed: `why`, such as a tag that is no release or a failed download.
pub(crate) fn failed(to: &Tag, why: &str, releases: &Releases) -> Failure {
    Failure::error(
        format!("could not install {to}: {why}"),
        vec![
            format!("Check that {to} is a release: {}", releases.page()),
            "Check the connection, then try again".to_string(),
        ],
    )
}

/// The binary's folder `dir` cannot be written: install.sh run as root can.
pub(crate) fn unwritable(dir: &str, releases: &Releases, to: &Tag) -> Failure {
    let env: Vec<String> = releases
        .install_env(to)
        .into_iter()
        .chain([("SAAIO_INSTALL_DIR".to_string(), dir.to_string())])
        .map(|(key, value)| format!("{key}={}", word(&value)))
        .collect();
    Failure::error(
        format!("cannot write to {dir}"),
        vec![format!(
            "Run `curl -fsSL {} | sudo env {} sh`",
            releases.url(Some(to), "install.sh"),
            env.join(" ")
        )],
    )
}

/// `text` as one shell word: as it is when it needs no quotes, else in single quotes.
fn word(text: &str) -> String {
    if !text.is_empty()
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "/._~:=-+@".contains(c))
    {
        text.to_string()
    } else {
        format!("'{}'", text.replace('\'', r"'\''"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tag(text: &str) -> Tag {
        Tag::parse(text).unwrap()
    }

    fn others() -> Others {
        Others {
            shadowed: Some(Binary {
                exe: "~/.cargo/bin/saaio".into(),
                build: "0.1.0 (9b77d8c)".into(),
            }),
            servers: vec![
                (
                    "~/.local/state/saaio/projects/shop".into(),
                    Running {
                        pid: 4321,
                        server: Some(Binary {
                            exe: "~/.local/bin/saaio".into(),
                            build: "0.1.2 (45630ee)".into(),
                        }),
                    },
                ),
                (
                    "~/.local/state/saaio/projects/blog".into(),
                    Running {
                        pid: 4400,
                        server: None,
                    },
                ),
            ],
        }
    }

    #[test]
    fn says_what_was_updated() {
        let document = updated(
            &Build::Release(tag("v0.1.0")),
            &tag("v0.1.1"),
            "~/.local/bin/saaio",
            &Others::default(),
        );

        assert_eq!(
            document.text(),
            "updated: saaio 0.1.0 -> 0.1.1\nbinary: ~/.local/bin/saaio\nhelp[1]: \"Changes: https://github.com/JanSuthacheeva/saaio/releases/tag/v0.1.1\""
        );
    }

    #[test]
    fn names_the_saaio_first_on_path_and_the_servers_on_another_version() {
        let document = updated(
            &Build::Release(tag("v0.1.2")),
            &tag("v0.2.0"),
            "~/.local/bin/saaio",
            &others(),
        );

        assert_eq!(
            document.text(),
            "updated: saaio 0.1.2 -> 0.2.0\nbinary: ~/.local/bin/saaio\nshadowed: ~/.cargo/bin/saaio (saaio 0.1.0 (9b77d8c))\nservers[2]{store,pid,build,binary}:\n  ~/.local/state/saaio/projects/shop,4321,0.1.2 (45630ee),~/.local/bin/saaio\n  ~/.local/state/saaio/projects/blog,4400,an earlier protocol,unknown\nhelp[3]: \"`saaio` runs ~/.cargo/bin/saaio, not ~/.local/bin/saaio: remove it (`cargo uninstall saaio`) or put ~/.local/bin first on PATH\",\"Run `SAAIO_HOME=<store> saaio --stop` for each server, then start saaio again; unfinished flows resume\",\"Changes: https://github.com/JanSuthacheeva/saaio/releases/tag/v0.2.0\""
        );
    }

    #[test]
    fn names_the_one_store_to_stop() {
        let mut others = others();
        others.shadowed = None;
        others.servers.truncate(1);

        let document = latest(
            &Build::Release(tag("v0.1.1")),
            &tag("v0.1.1"),
            "~/.local/bin/saaio",
            &others,
        );

        assert_eq!(
            document.text(),
            "running: v0.1.1\nlatest: v0.1.1\nupdate: none\nservers[1]{store,pid,build,binary}:\n  ~/.local/state/saaio/projects/shop,4321,0.1.2 (45630ee),~/.local/bin/saaio\nhelp[2]: \"Run `SAAIO_HOME=~/.local/state/saaio/projects/shop saaio --stop`, then start saaio again; unfinished flows resume\",Run `saaio update --version <tag>` to install another release"
        );
    }

    #[test]
    fn names_the_latest_release_when_nothing_is_newer() {
        let document = latest(
            &Build::Release(tag("v0.1.1")),
            &tag("v0.1.1"),
            "~/.local/bin/saaio",
            &Others::default(),
        );

        assert_eq!(
            document.text(),
            "running: v0.1.1\nlatest: v0.1.1\nupdate: none\nhelp[1]: Run `saaio update --version <tag>` to install another release"
        );
    }

    #[test]
    fn names_the_sudo_line_for_a_folder_it_cannot_write() {
        let failure = unwritable("/opt/my tools", &Releases::at(None), &tag("v0.1.1"));

        assert_eq!(failure.message, "cannot write to /opt/my tools");
        assert_eq!(
            failure.help,
            vec![
                "Run `curl -fsSL https://github.com/JanSuthacheeva/saaio/releases/download/v0.1.1/install.sh | sudo env SAAIO_VERSION=v0.1.1 SAAIO_INSTALL_DIR='/opt/my tools' sh`"
            ]
        );
        assert_eq!(word("it's"), r"'it'\''s'");
    }

    #[test]
    fn a_failed_install_names_the_tag_and_where_releases_are() {
        let failure = failed(&tag("v0.1.9"), "curl: (22) 404", &Releases::at(None));

        assert_eq!(failure.message, "could not install v0.1.9: curl: (22) 404");
        assert_eq!(
            failure.help,
            vec![
                "Check that v0.1.9 is a release: https://github.com/JanSuthacheeva/saaio/releases",
                "Check the connection, then try again",
            ]
        );
    }

    #[test]
    fn names_the_cargo_line_for_a_build_from_source() {
        let failure = source(&tag("v0.1.1"));

        assert_eq!(
            failure.help[0],
            "Run `cargo install --locked --git https://github.com/JanSuthacheeva/saaio --tag v0.1.1 --force`"
        );
    }
}
