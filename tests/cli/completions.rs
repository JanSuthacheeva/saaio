use std::fs;
use std::process::Command;

use tempfile::TempDir;

use crate::support::*;

#[test]
fn completions_print_a_script_for_every_shell_without_the_hidden_commands() {
    let home = TempDir::new().unwrap();

    for shell in ["bash", "zsh", "fish", "elvish", "powershell"] {
        let output = saaio(home.path(), &["completions", shell]);

        assert!(output.status.success(), "{shell}: {}", stdout(&output));
        let script = stdout(&output);
        assert!(script.contains("bug-report"), "{shell}: {script}");
        // wait's description, and bash's name for hook, which has none.
        assert!(!script.contains("asyncRewake"), "{shell}: {script}");
        assert!(!script.contains("subcmd__hook"), "{shell}: {script}");
        assert!(!script.ends_with("\n\n"), "{shell}: {script}");
    }
}

#[test]
fn the_bash_completions_are_valid_bash() {
    let home = TempDir::new().unwrap();
    let script = home.path().join("saaio.bash");
    let printed = saaio(home.path(), &["completions", "bash"]);
    assert!(
        stdout(&printed).contains("complete -F _saaio"),
        "{}",
        stdout(&printed)
    );
    fs::write(&script, printed.stdout).unwrap();

    let output = Command::new("bash")
        .arg("-n")
        .arg(&script)
        .output()
        .unwrap();

    assert!(output.status.success(), "{}", stderr(&output));
}

#[test]
fn completions_need_no_store() {
    let home = TempDir::new().unwrap();

    let output = saaio_command(home.path())
        .args(["completions", "zsh"])
        .env_remove("SAAIO_HOME")
        .env_remove("XDG_STATE_HOME")
        .env_remove("HOME")
        .output()
        .unwrap();

    assert!(output.status.success(), "{}", stdout(&output));
    assert!(stdout(&output).starts_with("#compdef saaio"));
}

#[test]
fn completions_for_an_unknown_shell_name_the_shells() {
    let home = TempDir::new().unwrap();

    let output = saaio(home.path(), &["completions", "tcsh"]);

    assert_eq!(output.status.code(), Some(2));
    let text = stdout(&output);
    assert!(
        text.starts_with(
            "error: invalid value 'tcsh' for '<SHELL>'; one of bash | elvish | fish | powershell | zsh\n"
        ),
        "{text}"
    );
    assert!(
        text.contains("Run `saaio completions --help` for the usage"),
        "{text}"
    );
}
