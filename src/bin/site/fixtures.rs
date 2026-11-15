#![allow(clippy::disallowed_methods, reason = "tests write scratch checkouts")]

use std::fs;

use tempfile::TempDir;

/// A scratch checkout with the pages every site has, then `files` over them.
pub fn checkout(files: &[(&str, &str)]) -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    let base = [
        ("README.md", "# saaio\n"),
        ("CHANGELOG.md", "# Changelog\n"),
        ("ROADMAP.md", "# Roadmap\n"),
    ];
    for (path, text) in base.iter().chain(files) {
        let path = dir.path().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    dir
}
