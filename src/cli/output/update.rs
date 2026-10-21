//! What `saaio update` prints: the update, the latest release already running, or why it cannot

use serde::Serialize;

use super::{Document, Failure, toon};
use crate::release::source::{GITHUB, Releases};
use crate::release::{Build, Tag};

/// updated, binary, help: the running server keeps the old version.
pub(crate) fn updated(from: &Build, to: &Tag, binary: &str) -> Document {
    #[derive(Serialize)]
    struct Updated<'a> {
        updated: String,
        binary: &'a str,
        help: Vec<String>,
    }
    Document::Toon(toon(&Updated {
        updated: format!("saaio {} -> {}", from.version(), to.version()),
        binary,
        help: vec![
            format!(
                "A running saaio server keeps {} until `saaio --stop`; unfinished flows resume after",
                from.version()
            ),
            format!("Changes: {GITHUB}/tag/{to}"),
        ],
    }))
}

/// running, latest, update: none.
pub(crate) fn latest(running: &Build, latest: &Tag) -> Document {
    #[derive(Serialize)]
    struct Latest {
        running: String,
        latest: String,
        update: &'static str,
        help: Vec<String>,
    }
    Document::Toon(toon(&Latest {
        running: format!("v{}", running.version()),
        latest: latest.to_string(),
        update: "none",
        help: vec!["Run `saaio update --version <tag>` to install another release".to_string()],
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

    #[test]
    fn says_what_was_updated_and_that_the_server_keeps_its_version() {
        let document = updated(
            &Build::Release(tag("v0.1.0")),
            &tag("v0.1.1"),
            "~/.local/bin/saaio",
        );

        assert_eq!(
            document.text(),
            "updated: saaio 0.1.0 -> 0.1.1\nbinary: ~/.local/bin/saaio\nhelp[2]: A running saaio server keeps 0.1.0 until `saaio --stop`; unfinished flows resume after,\"Changes: https://github.com/JanSuthacheeva/saaio/releases/tag/v0.1.1\""
        );
    }

    #[test]
    fn names_the_latest_release_when_nothing_is_newer() {
        let document = latest(&Build::Release(tag("v0.1.1")), &tag("v0.1.1"));

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
    fn names_the_cargo_line_for_a_build_from_source() {
        let failure = source(&tag("v0.1.1"));

        assert_eq!(
            failure.help[0],
            "Run `cargo install --locked --git https://github.com/JanSuthacheeva/saaio --tag v0.1.1 --force`"
        );
    }
}
