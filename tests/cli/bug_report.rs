use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use tempfile::TempDir;

use crate::support::*;

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
    let written: Vec<_> = fs::read_dir(cwd.path())
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
    // The current folder as the process reads it: /private/var/... for /var/... on macOS.
    let printed = said.lines().next().unwrap_or_default();
    assert!(
        printed.starts_with("report: /") && printed.ends_with(&format!("/{name}")),
        "{said}"
    );
    assert!(
        said.contains("Check the file before uploading it"),
        "{said}"
    );

    let report = fs::read_to_string(file).unwrap();
    assert!(report.starts_with("saaio bug report, 20"), "{report}");
    assert!(report.contains("\nsaaio   0.1.0 ("), "{report}");
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
    let target = home.path().join("target");
    fs::write(&target, "mine").unwrap();
    fs::remove_file(cwd.path().join(&first)).unwrap();
    std::os::unix::fs::symlink(&target, cwd.path().join(&first)).unwrap();

    let second = report();

    assert_eq!(fs::read_to_string(&target).unwrap(), "mine");
    let first = first.to_string_lossy();
    assert_eq!(
        second.to_string_lossy(),
        first.replace(".txt", "-2.txt"),
        "the next name, beside the link"
    );
    let mode = fs::metadata(cwd.path().join(&second))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
}

#[test]
fn bug_report_says_when_there_is_no_store() {
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

    assert!(output.status.success(), "{}", stdout(&output));
    // On the Linux runner a folder lands here too once HOME is unset.
    let written: Vec<_> = fs::read_dir(cwd.path())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    let file = written
        .iter()
        .find(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("saaio-report-")
        })
        .unwrap_or_else(|| panic!("no report among {written:?}"));
    let report = fs::read_to_string(file).unwrap();
    assert!(
        report.contains("\nstore        cannot be opened: cannot find a state directory\n"),
        "{report}"
    );
    assert!(!report.contains("== "), "{report}");
}
