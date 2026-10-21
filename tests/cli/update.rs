//! Tests of updates: the UI's notice, and `saaio update` from a source build, an unreachable
//! releases page, a tag that is none

use std::fs;
use std::path::Path;
use std::thread;
use std::time::Duration;

use tempfile::TempDir;

use crate::support::*;

/// A releases page in `dir` whose latest release is `latest`; its file:// URL.
fn releases_page(dir: &Path, latest: &str) -> String {
    let page = dir.join("releases/latest/download");
    fs::create_dir_all(&page).unwrap();
    fs::write(page.join("VERSION"), format!("{latest}\n")).unwrap();
    format!("file://{}/releases", dir.display())
}

#[test]
fn the_ui_names_a_newer_release_unless_turned_off() {
    for on in [true, false] {
        let lab = Lab::new();
        lab.agent("command = [\"cat\"]\nprompt = \"\"\nresume = [\"cat\"]");
        if !on {
            let config = lab.home.join("no-config.toml");
            let text = fs::read_to_string(&config).unwrap();
            fs::write(&config, format!("{text}\n[ui]\nupdate_check = false\n")).unwrap();
        }
        let url = releases_page(lab.dir.path(), "v9.9.9");
        let _stops = StopsServer(&lab.home);

        let terminal = Terminal::open(&lab.home, &lab.repo, &[], &[("SAAIO_RELEASES_URL", &url)]);

        wait_until("the UI never drew", || terminal.shows("\x1b[?1049h"));
        let looked = shared(&lab.home).join("saaio/update.json");
        if on {
            wait_until("the UI never named the release", || {
                terminal.shows("9.9.9 is out: ")
            });
            assert!(
                fs::read_to_string(&looked)
                    .unwrap()
                    .contains("\"latest\":\"v9.9.9\"")
            );
        } else {
            thread::sleep(Duration::from_secs(1));
            assert!(!terminal.shows("is out: "), "{}", terminal.text());
            assert!(!looked.exists());
        }
        assert!(lab.run(&["--stop"]).status.success());
    }
}

#[test]
fn a_build_from_source_names_the_cargo_line_for_the_latest_release() {
    let home = TempDir::new().unwrap();
    let page = home.path().join("releases/latest/download");
    fs::create_dir_all(&page).unwrap();
    fs::write(page.join("VERSION"), "v9.9.9\n").unwrap();

    let output = saaio_command(home.path())
        .args(["update"])
        .env(
            "SAAIO_RELEASES_URL",
            format!("file://{}/releases", home.path().display()),
        )
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1), "{}", stdout(&output));
    assert_eq!(
        stdout(&output),
        "error: \"this saaio was built from source, so saaio update does not replace it\"\nhelp[2]: \"Run `cargo install --locked --git https://github.com/JanSuthacheeva/saaio --tag v9.9.9 --force`\",\"Or install the release binary instead: docs/install.md\"\n"
    );
}

#[test]
fn an_unreachable_releases_page_fails_with_what_to_do() {
    let home = TempDir::new().unwrap();

    let output = saaio_command(home.path())
        .args(["update"])
        .env(
            "SAAIO_RELEASES_URL",
            format!("file://{}/nowhere", home.path().display()),
        )
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    assert!(
        stdout(&output)
            .starts_with("error: \"could not read the latest release: could not read file://"),
        "{}",
        stdout(&output)
    );
    assert!(
        stdout(&output).contains("help[1]: \"Check the connection, or pass --version <tag>\""),
        "{}",
        stdout(&output)
    );
}

#[test]
fn a_version_that_is_no_tag_is_a_usage_error() {
    let home = TempDir::new().unwrap();

    let output = saaio(home.path(), &["update", "--version", "latest"]);

    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        stdout(&output),
        "error: \"not a release tag: latest\"\nhelp[1]: \"Pass a release tag, such as --version v0.1.1\"\n"
    );
}
