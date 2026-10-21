use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use crate::event::{Event, EventKind, Mode};
use crate::fold;
use crate::view::{FlowView, Workspace};

/// Writes an executable script through a child process, so this process never holds it open for
/// writing while another test thread forks: exec of such a file fails with ETXTBSY on Linux.
pub fn executable(path: &Path, body: &str) {
    let mut writer = Command::new("sh")
        .args(["-c", "cat > \"$1\" && chmod 755 \"$1\"", "sh"])
        .arg(path)
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    writer
        .stdin
        .take()
        .unwrap()
        .write_all(body.as_bytes())
        .unwrap();
    let status = writer.wait().unwrap();
    assert!(status.success(), "could not write {}", path.display());
}

/// `name`'s first event: started at 0 in auto mode with the built-in phases.
pub fn started(name: &str, request: &str) -> Event {
    Event::new(
        0,
        EventKind::Started {
            name: name.into(),
            request: request.into(),
            mode: Mode::Auto,
            phases: None,
        },
    )
}

/// A worktree for `name` at /repo/.saaio/worktrees/<name> on flow/<name>, slot 1, run by `agent`.
pub fn workspace(name: &str, agent: &str) -> Workspace {
    Workspace {
        path: format!("/repo/.saaio/worktrees/{name}"),
        branch: Some(format!("flow/{name}")),
        slot: Some(1),
        window: None,
        agent: agent.into(),
    }
}

/// `id` started (named after it), then `events`, folded at `now`.
pub fn flow(id: &str, events: Vec<Event>, now: u64) -> FlowView {
    let mut all = vec![started(id, "x")];
    all.extend(events);
    fold::fold(id, &all, None, now).expect("a started flow folds")
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    use tempfile::TempDir;

    use super::*;

    #[test]
    fn writes_a_script_anyone_may_run() {
        let dir = TempDir::new().unwrap();
        let script = dir.path().join("hello");

        executable(&script, "#!/bin/sh\necho \"hello $1\"\n");

        let mode = fs::metadata(&script).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o755);
        let output = Command::new(&script).arg("there").output().unwrap();
        assert_eq!(String::from_utf8(output.stdout).unwrap(), "hello there\n");
    }
}
