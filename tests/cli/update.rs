//! Tests of `saaio update`: a source build, an unreachable releases page, a tag that is none

use std::fs;

use tempfile::TempDir;

use crate::support::*;

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
