//! The CLI end to end: one module per subject, and the test that every file is declared

#![allow(
    clippy::disallowed_methods,
    reason = "tests set up their files and read PATH directly"
)]

mod bug_report;
mod completions;
mod home;
mod hooks;
mod identity;
mod remove;
mod report;
mod send;
mod server;
mod stack;
mod start;
mod status;
mod store;
mod support;
mod trust;
mod update;
mod wait;

use std::fs;
use std::path::Path;

#[test]
fn every_file_in_tests_cli_is_a_module() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/cli");
    let main = fs::read_to_string(dir.join("main.rs")).unwrap();
    let mut forgotten: Vec<String> = fs::read_dir(&dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "rs"))
        .filter_map(|path| Some(path.file_stem()?.to_str()?.to_string()))
        .filter(|stem| stem != "main" && !main.lines().any(|line| line == format!("mod {stem};")))
        .collect();
    forgotten.sort();
    assert!(
        forgotten.is_empty(),
        "never compiled, add `mod`: {forgotten:?}"
    );
}
