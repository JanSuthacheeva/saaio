//! Commands run with a deadline, their output read even past a process they left behind

use std::fs::File;
use std::io::{self, ErrorKind, Read, Seek, SeekFrom};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use rustix::process::{Pid, Signal, kill_process_group};

use crate::text::short_duration;
use crate::threads;

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

/// Runs `command` with no stdin in a process group of its own; its stdout, or an error with the
/// last line it printed. Past `limit` the whole group is killed, with what the command started,
/// and it fails.
pub(super) fn finish_within(
    mut command: Command,
    program: &str,
    limit: Duration,
) -> Result<Vec<u8>> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .spawn()
        .with_context(|| format!("could not run {program}"))?;
    // Read in the background, so a full pipe never stops the command before it ends.
    let pipes =
        read_in_background(child.stdout.take(), format!("{program} stdout")).and_then(|stdout| {
            let stderr = read_in_background(child.stderr.take(), format!("{program} stderr"))?;
            Ok((stdout, stderr))
        });
    let (stdout, stderr) = match pipes {
        Ok(pipes) => pipes,
        Err(error) => {
            kill(&mut child);
            return Err(error).with_context(|| format!("could not read what {program} prints"));
        }
    };
    let status = wait_within(&mut child, limit)?;
    let read_until = Instant::now() + READ_GRACE;
    let stdout = stdout.collect(read_until);
    let stderr = stderr.collect(read_until);
    if !status.success() {
        bail!("{}", last_line(&stderr, &stdout));
    }
    Ok(stdout)
}

/// Runs `command` with no stdin in a process group of its own, stdout and stderr both into `log`;
/// Err with the last non-empty line `path` holds on a non-zero exit, or "no answer within ..." past
/// `limit`, the whole group killed (what it printed until then stays in the file).
pub(super) fn finish_into(
    mut command: Command,
    log: File,
    path: &Path,
    program: &str,
    limit: Duration,
) -> Result<()> {
    let errors = log
        .try_clone()
        .with_context(|| format!("could not write {}", path.display()))?;
    let mut child = command
        .stdin(Stdio::null())
        .stdout(log)
        .stderr(errors)
        .process_group(0)
        .spawn()
        .with_context(|| format!("could not run {program}"))?;
    let status = wait_within(&mut child, limit)?;
    if !status.success() {
        bail!("{}", last_line_of(path));
    }
    Ok(())
}

/// How much of a log's end `last_line_of` reads.
const TAIL: u64 = 64 * 1024;

/// The last non-empty line of the file's last 64 KiB, or "failed without a message".
fn last_line_of(path: &Path) -> String {
    let mut tail = Vec::new();
    if let Ok(mut file) = File::open(path) {
        let length = file.metadata().map(|meta| meta.len()).unwrap_or(0);
        let _ = file.seek(SeekFrom::Start(length.saturating_sub(TAIL)));
        let _ = file.take(TAIL).read_to_end(&mut tail);
    }
    last_line(&[], &tail)
}

/// The child's exit status, or an error once `limit` passed, its whole group killed.
fn wait_within(child: &mut Child, limit: Duration) -> Result<ExitStatus> {
    let deadline = Instant::now() + limit;
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        if Instant::now() >= deadline {
            kill(child);
            let millis = u64::try_from(limit.as_millis()).unwrap_or(u64::MAX);
            bail!("no answer within {}", short_duration(millis));
        }
        thread::sleep(WAIT_STEP);
    }
}

/// Kills the child's whole process group and waits for the child.
fn kill(child: &mut Child) {
    match Pid::from_raw(child.id().cast_signed()) {
        Some(group) => {
            let _ = kill_process_group(group, Signal::KILL);
        }
        None => {
            let _ = child.kill();
        }
    }
    let _ = child.wait();
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

fn read_in_background(pipe: Option<impl Read + Send + 'static>, name: String) -> io::Result<Pipe> {
    let bytes = Arc::new(Mutex::new(Vec::new()));
    let shared = Arc::clone(&bytes);
    let reader = threads::spawn(name, move || {
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
    })?;
    Ok(Pipe { bytes, reader })
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

    use rustix::process::{Pid, Signal, kill_process, test_kill_process};

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

    #[test]
    fn a_command_killed_past_its_limit_takes_what_it_started_with_it() {
        let dir = TempDir::new().unwrap();
        let pid_file = dir.path().join("pid");
        // sh itself, not a script written for the test: a new executable can take seconds to
        // start under load on macOS, past the limit.
        let argv = [
            "sh".to_string(),
            "-c".to_string(),
            format!(
                "sleep 30 & echo $! > '{}'; exec sleep 30",
                pid_file.display()
            ),
        ];

        let error = output(&argv, Duration::from_secs(2));

        let error = error.unwrap_err();
        let pid = fs::read_to_string(&pid_file).unwrap_or_else(|_| panic!("no pid: {error:#}"));
        let pid = Pid::from_raw(pid.trim().parse().unwrap()).unwrap();
        let started = Instant::now();
        while test_kill_process(pid).is_ok() && started.elapsed() < SOON {
            thread::sleep(WAIT_STEP);
        }
        let alive = test_kill_process(pid).is_ok();
        if alive {
            let _ = kill_process(pid, Signal::KILL);
        }
        assert!(!alive, "the background sleep outlived its command");
    }

    fn into_log(script: &str, limit: Duration) -> (Result<()>, String) {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("hook.log");
        let log = crate::private::create_fresh(&path).unwrap();
        let mut command = Command::new("sh");
        command.args(["-c", script]);
        let result = finish_into(command, log, &path, "sh", limit);
        (result, fs::read_to_string(&path).unwrap())
    }

    #[test]
    fn writes_stdout_and_stderr_to_the_log_in_order() {
        let (result, log) = into_log("echo one; echo two >&2; echo three", LIMIT);

        result.unwrap();
        assert_eq!(log, "one\ntwo\nthree\n");
    }

    #[test]
    fn a_failing_command_reports_the_last_line_of_its_log() {
        let script = "echo 'requires ext-zip' >&2; echo 'Please run composer update'; echo; exit 2";
        let (result, log) = into_log(script, LIMIT);

        assert_eq!(
            format!("{:#}", result.unwrap_err()),
            "Please run composer update"
        );
        assert!(log.contains("requires ext-zip"), "{log}");
        let (silent, _) = into_log("exit 1", LIMIT);
        assert_eq!(
            format!("{:#}", silent.unwrap_err()),
            "failed without a message"
        );
    }

    #[test]
    fn a_command_killed_past_its_limit_leaves_its_output_in_the_log() {
        let started = Instant::now();
        let (result, log) = into_log("echo installing; exec sleep 30", Duration::from_secs(2));

        assert!(started.elapsed() < SOON);
        assert!(format!("{:#}", result.unwrap_err()).starts_with("no answer within"),);
        assert_eq!(log, "installing\n");
    }

    #[test]
    fn a_process_left_behind_does_not_hold_the_command_up() {
        let started = Instant::now();
        let (result, log) = into_log("sleep 30 & echo done", LIMIT);

        result.unwrap();
        assert!(started.elapsed() < Duration::from_secs(10));
        assert_eq!(log, "done\n");
    }

    #[test]
    fn reads_only_the_end_of_a_long_log() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("long.log");
        let long = format!("{}\nthe cause\n\n", "x".repeat(200_000));
        fs::write(&path, long).unwrap();

        assert_eq!(last_line_of(&path), "the cause");
        assert_eq!(
            last_line_of(&dir.path().join("missing")),
            "failed without a message"
        );
    }
}
