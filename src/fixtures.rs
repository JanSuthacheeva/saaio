//! What tests across the crate share: `executable`, `started`, `workspace`, `flow` and `Noise`

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
        base: None,
    }
}

/// `id` started (named after it), then `events`, folded at `now`.
pub fn flow(id: &str, events: Vec<Event>, now: u64) -> FlowView {
    let mut all = vec![started(id, "x")];
    all.extend(events);
    fold::fold(id, &all, None, now).expect("a started flow folds")
}

/// Bytes that look random but repeat for a seed (xorshift64), for the decoder fuzz tests.
pub struct Noise(u64);

impl Noise {
    pub fn new(seed: u64) -> Noise {
        Noise(seed | 1)
    }

    pub fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    /// Up to `max` bytes.
    pub fn bytes(&mut self, max: usize) -> Vec<u8> {
        let len = self.next() as usize % (max + 1);
        (0..len).map(|_| self.next() as u8).collect()
    }

    /// `bytes` with up to four of them changed, then cut or grown by a few.
    pub fn mutate(&mut self, bytes: &[u8]) -> Vec<u8> {
        let mut out = bytes.to_vec();
        for _ in 0..self.next() % 5 {
            if !out.is_empty() {
                let at = self.next() as usize % out.len();
                out[at] = self.next() as u8;
            }
        }
        match self.next() % 4 {
            0 => out.truncate(self.next() as usize % (out.len() + 1)),
            1 => out.extend(self.bytes(8)),
            _ => {}
        }
        out
    }
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
