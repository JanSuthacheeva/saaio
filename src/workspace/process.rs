//! Commands run with a deadline, their output read even past a process they left behind

use std::io::{ErrorKind, Read};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};

use crate::text::short_duration;

/// Runs an argv in `dir` with GH_PROMPT_DISABLED=1 and no stdin; its stdout, or an error with
/// the last line it printed. Past `limit` it is killed and fails.
pub fn output_in(dir: &Path, argv: &[String], limit: Duration) -> Result<String> {
    let (program, args) = argv.split_first().context("empty command")?;
    let mut command = Command::new(program);
    command
        .args(args)
        .current_dir(dir)
        .env("GH_PROMPT_DISABLED", "1");
    let stdout = finish_within(command, program, limit)?;
    Ok(String::from_utf8_lossy(&stdout).into_owned())
}

/// Runs an argv with no stdin; its stdout, or an error with the last line it printed. Past `limit`
/// it is killed and fails.
pub fn output(argv: &[String], limit: Duration) -> Result<String> {
    let (program, args) = argv.split_first().context("empty command")?;
    let mut command = Command::new(program);
    command.args(args);
    let stdout = finish_within(command, program, limit)?;
    Ok(String::from_utf8_lossy(&stdout).into_owned())
}

/// Runs `command` with no stdin; its stdout, or an error with the last line it printed. Past
/// `limit` it is killed and fails.
pub(super) fn finish_within(
    mut command: Command,
    program: &str,
    limit: Duration,
) -> Result<Vec<u8>> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("could not run {program}"))?;
    // Read in the background, so a full pipe never stops the command before it ends.
    let stdout = read_in_background(child.stdout.take());
    let stderr = read_in_background(child.stderr.take());
    let deadline = Instant::now() + limit;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            let millis = u64::try_from(limit.as_millis()).unwrap_or(u64::MAX);
            bail!("no answer within {}", short_duration(millis));
        }
        thread::sleep(WAIT_STEP);
    };
    let read_until = Instant::now() + READ_GRACE;
    let stdout = stdout.collect(read_until);
    let stderr = stderr.collect(read_until);
    if !status.success() {
        bail!("{}", last_line(&stderr, &stdout));
    }
    Ok(stdout)
}

/// How often finish_within looks whether its command ended.
const WAIT_STEP: Duration = Duration::from_millis(20);

/// How long the output of a command that ended is read: a process it left behind may hold its
/// pipes open for as long as it runs.
const READ_GRACE: Duration = Duration::from_millis(500);

/// A pipe read on a thread of its own, into bytes the reader shares.
struct Pipe {
    bytes: Arc<Mutex<Vec<u8>>>,
    reader: JoinHandle<()>,
}

impl Pipe {
    /// All the pipe gives until it closes, or what it gave by `deadline`.
    fn collect(self, deadline: Instant) -> Vec<u8> {
        while !self.reader.is_finished() && Instant::now() < deadline {
            thread::sleep(WAIT_STEP);
        }
        std::mem::take(&mut *self.bytes.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

fn read_in_background(pipe: Option<impl Read + Send + 'static>) -> Pipe {
    let bytes = Arc::new(Mutex::new(Vec::new()));
    let shared = Arc::clone(&bytes);
    let reader = thread::spawn(move || {
        let Some(mut pipe) = pipe else {
            return;
        };
        let mut chunk = [0; 8192];
        loop {
            match pipe.read(&mut chunk) {
                Ok(0) => break,
                // Taken already: drained, never closed, so the process left behind can write on.
                Ok(_) if Arc::strong_count(&shared) == 1 => {}
                Ok(read) => shared
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .extend_from_slice(&chunk[..read]),
                Err(error) if error.kind() == ErrorKind::Interrupted => {}
                Err(_) => break,
            }
        }
    });
    Pipe { bytes, reader }
}

/// Runs an argv; its stdout, or an error with the last line it printed.
pub(super) fn run(argv: &[String]) -> Result<String> {
    let (program, args) = argv.split_first().context("empty command")?;
    let mut command = Command::new(program);
    command.args(args);
    stdout_of(command, program)
}

pub(super) fn stdout_of(mut command: Command, program: &str) -> Result<String> {
    let output = command
        .output()
        .with_context(|| format!("could not run {program}"))?;
    if !output.status.success() {
        bail!("{}", last_line(&output.stderr, &output.stdout));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

pub(super) fn last_line(stderr: &[u8], stdout: &[u8]) -> String {
    let text = String::from_utf8_lossy(if stderr.is_empty() { stdout } else { stderr });
    text.lines()
        .map(str::trim)
        .rfind(|line| !line.is_empty())
        .unwrap_or("failed without a message")
        .to_string()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;
    use crate::fixtures::executable;
    use crate::workspace::fixtures::{LIMIT, SOON};

    #[test]
    fn runs_a_command_in_a_folder_without_prompts() {
        let dir = TempDir::new().unwrap();
        let script = dir.path().join("ask");
        executable(
            &script,
            "#!/bin/sh\nread -r line\necho \"$(basename \"$PWD\") $GH_PROMPT_DISABLED $1 [$line]\"\n",
        );
        let folder = dir.path().join("csv");
        fs::create_dir(&folder).unwrap();
        let argv = [script.display().to_string(), "list".to_string()];

        assert_eq!(output_in(&folder, &argv, LIMIT).unwrap(), "csv 1 list []\n");

        let missing = ["saaio-test-no-such-program".to_string()];
        let error = output_in(&folder, &missing, LIMIT).unwrap_err();
        assert!(format!("{error:#}").starts_with("could not run saaio-test-no-such-program"));
    }

    #[test]
    fn runs_a_command_wherever_saaio_runs() {
        let argv = ["printf".to_string(), "%s".to_string(), "v0.1.1".to_string()];

        assert_eq!(output(&argv, LIMIT).unwrap(), "v0.1.1");
        let failing = [
            "sh".to_string(),
            "-c".to_string(),
            "echo gone >&2; exit 6".to_string(),
        ];
        assert_eq!(
            format!("{:#}", output(&failing, LIMIT).unwrap_err()),
            "gone"
        );
    }

    #[test]
    fn a_command_that_gives_no_answer_in_time_is_killed() {
        let dir = TempDir::new().unwrap();
        let script = dir.path().join("hang");
        executable(&script, "#!/bin/sh\necho started\nexec sleep 30\n");
        let argv = [script.display().to_string()];
        let started = Instant::now();

        let error = output_in(dir.path(), &argv, Duration::from_millis(200)).unwrap_err();

        assert!(started.elapsed() < SOON);
        assert!(
            format!("{error:#}").ends_with("no answer within 0s"),
            "{error:#}"
        );
        let failing = dir.path().join("fail");
        executable(&failing, "#!/bin/sh\necho 'not logged in' >&2\nexit 4\n");
        let error = output_in(dir.path(), &[failing.display().to_string()], LIMIT).unwrap_err();
        assert_eq!(format!("{error:#}"), "not logged in");
    }
}
