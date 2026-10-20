use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

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
