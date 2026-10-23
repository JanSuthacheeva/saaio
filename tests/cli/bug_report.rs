//! Tests of `saaio bug-report`: the file it writes in the shared folder, never through a link,
//! without a store

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use tempfile::TempDir;

use crate::support::*;

/// Where the reports of a test's home go: reports/ in its shared folder.
fn reports(home: &Path) -> PathBuf {
    shared(home).join("saaio/reports")
}

#[test]
fn bug_report_writes_one_file_with_versions_settings_and_the_logs() {
    let home = TempDir::new().unwrap();
    let cwd = TempDir::new().unwrap();
    fs::write(home.path().join("no-config.toml"), "[ui]\nprefix = 3\n").unwrap();
    fs::write(home.path().join("theme.toml"), "not toml [\n").unwrap();
    fs::write(home.path().join("server.log.1"), "an older line\n").unwrap();
    let lines: Vec<String> = (1..=400).map(|n| format!("line {n}")).collect();
    let lines = lines.join("\n") + "\n";
    fs::write(home.path().join("server.log"), lines).unwrap();

    let output = saaio_command(home.path())
        .arg("bug-report")
        .env("HOME", home.path())
        .current_dir(cwd.path())
        .output()
        .unwrap();

    assert!(output.status.success(), "{}", stdout(&output));
    assert_eq!(fs::read_dir(cwd.path()).unwrap().count(), 0);
    let written: Vec<_> = fs::read_dir(reports(home.path()))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    let [file] = &written[..] else {
        panic!("one file expected: {written:?}");
    };
    let name = file.file_name().unwrap().to_string_lossy().into_owned();
    assert!(
        name.starts_with("saaio-report-20") && file.extension() == Some("txt".as_ref()),
        "{name}"
    );
    let said = stdout(&output);
    let printed = said.lines().next().unwrap_or_default();
    assert_eq!(printed, format!("report: {}", file.display()), "{said}");
    assert!(
        said.contains("Check the file before uploading it"),
        "{said}"
    );
    assert!(said.contains("never written into a repository"), "{said}");

    let report = fs::read_to_string(file).unwrap();
    assert!(report.starts_with("saaio bug report, 20"), "{report}");
    let version = format!("\nsaaio   {} (", env!("CARGO_PKG_VERSION"));
    assert!(report.contains(&version), "{report}");
    assert!(report.contains("\nstore        ~\n"), "{report}");
    assert!(
        report.contains("\nconfig.toml  ~/no-config.toml: does not load: ~/no-config.toml:2: "),
        "{report}"
    );
    assert!(
        report.contains("\ntheme.toml   ~/theme.toml: does not load: "),
        "{report}"
    );
    assert!(report.contains("== ~/server.log.1 (last 300 lines) ==\nan older line\n"));
    assert!(report.contains("== ~/server.log (last 300 lines) ==\nline 101\n"));
    assert!(report.ends_with("line 400\n"), "{report}");
    assert!(!report.contains("line 100\n"), "{report}");
    assert!(
        !report.contains(&*home.path().to_string_lossy()),
        "{report}"
    );
}

#[test]
fn bug_report_writes_a_linked_home_as_a_tilde_in_every_form_a_log_names_it() {
    let dir = TempDir::new().unwrap();
    let real = dir.path().join("real");
    let home = dir.path().join("home");
    let cwd = TempDir::new().unwrap();
    fs::create_dir(&real).unwrap();
    std::os::unix::fs::symlink(&real, &home).unwrap();
    let real = fs::canonicalize(&real).unwrap();
    let dashed = |path: &std::path::Path| -> String {
        path.to_string_lossy()
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect()
    };
    let log = format!(
        "open file://{home}/x.md\nreal {real}/y\nPATH=/bin:{home}/bin\n\
         read ~/.claude/projects/{dashed_home}-shop/a.jsonl\n\
         read ~/.claude/projects/{dashed_real}-shop/b.jsonl\n",
        home = home.display(),
        real = real.display(),
        dashed_home = dashed(&home),
        dashed_real = dashed(&real),
    );
    fs::write(real.join("server.log"), log).unwrap();

    let output = saaio_command(&home)
        .arg("bug-report")
        .env("HOME", &home)
        .current_dir(cwd.path())
        .output()
        .unwrap();

    assert!(output.status.success(), "{}", stdout(&output));
    let file = fs::read_dir(reports(&home))
        .unwrap()
        .next()
        .unwrap()
        .unwrap();
    let report = fs::read_to_string(file.path()).unwrap();
    assert!(
        report.contains(
            "open file://~/x.md\nreal ~/y\nPATH=/bin:~/bin\n\
             read ~/.claude/projects/~-shop/a.jsonl\n\
             read ~/.claude/projects/~-shop/b.jsonl\n"
        ),
        "{report}"
    );
}

#[test]
fn bug_report_never_writes_through_what_is_already_there() {
    let home = TempDir::new().unwrap();
    let cwd = TempDir::new().unwrap();
    let report = || {
        let output = saaio_command(home.path())
            .arg("bug-report")
            .current_dir(cwd.path())
            .output()
            .unwrap();
        assert!(output.status.success(), "{}", stdout(&output));
        let said = stdout(&output);
        let path = said
            .lines()
            .next()
            .unwrap()
            .strip_prefix("report: ")
            .unwrap();
        PathBuf::from(path).file_name().unwrap().to_owned()
    };
    let first = report();
    let reports = reports(home.path());
    let target = home.path().join("target");
    fs::write(&target, "mine").unwrap();
    fs::remove_file(reports.join(&first)).unwrap();
    std::os::unix::fs::symlink(&target, reports.join(&first)).unwrap();

    let second = report();

    assert_eq!(fs::read_to_string(&target).unwrap(), "mine");
    let first = first.to_string_lossy();
    assert_eq!(
        second.to_string_lossy(),
        first.replace(".txt", "-2.txt"),
        "the next name, beside the link"
    );
    let mode = fs::metadata(reports.join(&second))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
    let mode = fs::metadata(&reports).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o700);
}

#[test]
fn bug_report_writes_nothing_into_the_repository_it_runs_in() {
    let home = TempDir::new().unwrap();
    let repo = TempDir::new().unwrap();
    git(repo.path(), &["init", "-q"]);

    let output = saaio_command(home.path())
        .arg("bug-report")
        .current_dir(repo.path())
        .output()
        .unwrap();

    assert!(output.status.success(), "{}", stdout(&output));
    let status = std::process::Command::new("git")
        .args(["status", "--porcelain", "--ignored"])
        .current_dir(repo.path())
        .output()
        .unwrap();
    assert_eq!(stdout(&status), "");
    assert_eq!(fs::read_dir(reports(home.path())).unwrap().count(), 1);
}

#[test]
fn bug_report_says_when_there_is_no_state_directory() {
    let home = TempDir::new().unwrap();
    let cwd = TempDir::new().unwrap();

    let output = saaio_command(home.path())
        .arg("bug-report")
        .env_remove("SAAIO_HOME")
        .env_remove("XDG_STATE_HOME")
        .env_remove("HOME")
        .current_dir(cwd.path())
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1), "{}", stdout(&output));
    let said = stdout(&output);
    assert!(said.contains("cannot find a state directory"), "{said}");
    assert!(said.contains("Set HOME or XDG_STATE_HOME"), "{said}");
    assert_eq!(fs::read_dir(cwd.path()).unwrap().count(), 0);
}
