use std::io::{self, BufRead, BufReader, ErrorKind};
use std::process::{ChildStdout, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

/// The tools whose versions a report names.
pub const TOOLS: [&str; 4] = ["claude", "codex", "gh", "glab"];
/// How long a tool gets to print its version.
const DEADLINE: Duration = Duration::from_secs(1);

/// The system saaio runs on: the OS and the versions of the tools around it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct System {
    /// `uname -srm`: `Darwin 25.6.0 arm64`; None when it did not answer.
    pub os: Option<String>,
    pub tools: Vec<(&'static str, Probe)>,
}

/// What `<tool> --version` gave.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Probe {
    /// From the first word that starts with a digit: `2.73.0 (2025-05-19)` of
    /// `gh version 2.73.0 (2025-05-19)`; the whole first line when no word does.
    Version(String),
    /// Not on PATH.
    Missing,
    /// It ran but failed, printed nothing or took longer than DEADLINE: why.
    Failed(String),
}

impl System {
    /// Asks the OS and every tool of TOOLS at once, waiting at most DEADLINE.
    pub fn probe() -> System {
        let deadline = Instant::now() + DEADLINE;
        let os = thread::spawn(move || run("uname", &["-srm"], deadline));
        let tools: Vec<_> = TOOLS
            .iter()
            .map(|&tool| (tool, thread::spawn(move || version(tool, deadline))))
            .collect();
        let os = match os.join() {
            Ok(Probe::Version(os)) => Some(os),
            _ => None,
        };
        System {
            os,
            tools: tools
                .into_iter()
                .map(|(tool, probe)| {
                    let probe = probe
                        .join()
                        .unwrap_or_else(|_| Probe::Failed("the probe panicked".to_string()));
                    (tool, probe)
                })
                .collect(),
        }
    }

    /// `Darwin 25.6.0 arm64, claude 2.1.302 (Claude Code), codex 0.163.0, gh 2.73.0 (2025-05-19), glab missing`.
    pub fn summary(&self) -> String {
        let os = self.os.as_deref().unwrap_or("an unknown OS").to_string();
        let tools = self
            .tools
            .iter()
            .map(|(tool, probe)| format!("{tool} {}", probe.text()));
        std::iter::once(os)
            .chain(tools)
            .collect::<Vec<_>>()
            .join(", ")
    }
}

impl Probe {
    /// `2.73.0`, `missing` or `failed: <why>`.
    pub fn text(&self) -> String {
        match self {
            Probe::Version(version) => version.clone(),
            Probe::Missing => "missing".to_string(),
            Probe::Failed(why) => format!("failed: {why}"),
        }
    }
}

/// `<tool> --version`, from the first word that starts with a digit.
fn version(tool: &str, deadline: Instant) -> Probe {
    match run(tool, &["--version"], deadline) {
        Probe::Version(line) => Probe::Version(version_of(&line)),
        other => other,
    }
}

/// Runs `program args`, killed at `deadline`; the first line it prints as Probe::Version. The
/// line is read on its own thread: a child it leaves running can hold its output open.
fn run(program: &str, args: &[&str], deadline: Instant) -> Probe {
    let spawned = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn();
    let mut child = match spawned {
        Ok(child) => child,
        Err(error) if error.kind() == ErrorKind::NotFound => return Probe::Missing,
        Err(error) => return Probe::Failed(error.to_string()),
    };
    let first_line = child.stdout.take().map(read_first_line);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(5)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Probe::Failed(NO_ANSWER.to_string());
            }
            Err(error) => return Probe::Failed(error.to_string()),
        }
    };
    if !status.success() {
        return Probe::Failed(format!("{status}"));
    }
    let left = deadline.saturating_duration_since(Instant::now());
    match first_line.map(|line| line.recv_timeout(left)) {
        Some(Ok(Some(line))) => Probe::Version(line),
        Some(Err(_)) => Probe::Failed(NO_ANSWER.to_string()),
        Some(Ok(None)) | None => Probe::Failed("printed nothing".to_string()),
    }
}

/// Why a probe that took longer than its deadline failed.
const NO_ANSWER: &str = "no answer in time";

/// The first non-empty line `out` gives, trimmed (None at its end without one), sent once it is
/// read; the rest is drained so the program never blocks on a full pipe.
fn read_first_line(out: ChildStdout) -> Receiver<Option<String>> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut out = BufReader::new(out);
        let mut line = String::new();
        loop {
            line.clear();
            match out.read_line(&mut line) {
                Ok(0) | Err(_) => {
                    let _ = sender.send(None);
                    return;
                }
                Ok(_) if !line.trim().is_empty() => break,
                Ok(_) => {}
            }
        }
        let _ = sender.send(Some(line.trim().to_string()));
        let _ = io::copy(&mut out, &mut io::sink());
    });
    receiver
}

/// The version part of a `--version` line.
fn version_of(line: &str) -> String {
    let words: Vec<&str> = line.split_whitespace().collect();
    match words
        .iter()
        .position(|word| word.starts_with(|c: char| c.is_ascii_digit()))
    {
        Some(at) => words[at..].join(" "),
        None => line.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};

    use tempfile::TempDir;

    use super::*;

    fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    fn soon() -> Instant {
        Instant::now() + Duration::from_secs(5)
    }

    #[test]
    fn takes_the_version_from_the_first_word_with_a_digit() {
        assert_eq!(version_of("2.1.302 (Claude Code)"), "2.1.302 (Claude Code)");
        assert_eq!(version_of("codex-cli 0.163.0"), "0.163.0");
        assert_eq!(
            version_of("gh version 2.73.0 (2025-05-19)"),
            "2.73.0 (2025-05-19)"
        );
        assert_eq!(version_of("glab 1.80.4 (f4b518e9)"), "1.80.4 (f4b518e9)");
        assert_eq!(version_of("development build"), "development build");
    }

    #[test]
    fn sums_up_the_os_and_every_tool() {
        let system = System {
            os: Some("Darwin 25.6.0 arm64".to_string()),
            tools: vec![
                (
                    "claude",
                    Probe::Version("2.1.302 (Claude Code)".to_string()),
                ),
                ("glab", Probe::Missing),
                ("gh", Probe::Failed("no answer in time".to_string())),
            ],
        };
        assert_eq!(
            system.summary(),
            "Darwin 25.6.0 arm64, claude 2.1.302 (Claude Code), glab missing, gh failed: no answer in time"
        );
        assert_eq!(System::default().summary(), "an unknown OS");
    }

    #[test]
    fn reads_the_first_line_a_tool_prints() {
        let dir = TempDir::new().unwrap();
        let tool = script(
            dir.path(),
            "gh",
            "echo; echo 'gh version 2.73.0'; echo more",
        );
        assert_eq!(
            run(tool.to_str().unwrap(), &[], soon()),
            Probe::Version("gh version 2.73.0".to_string())
        );
        assert_eq!(
            version(tool.to_str().unwrap(), soon()),
            Probe::Version("2.73.0".to_string())
        );
    }

    #[test]
    fn a_missing_tool_is_missing() {
        assert_eq!(
            run("saaio-no-such-tool", &["--version"], soon()),
            Probe::Missing
        );
    }

    #[test]
    fn a_tool_that_fails_or_prints_nothing_failed() {
        let dir = TempDir::new().unwrap();
        let failing = script(dir.path(), "failing", "echo 1.0; exit 3");
        assert!(matches!(
            run(failing.to_str().unwrap(), &[], soon()),
            Probe::Failed(why) if why.contains('3')
        ));
        let silent = script(dir.path(), "silent", "true");
        assert_eq!(
            run(silent.to_str().unwrap(), &[], soon()),
            Probe::Failed("printed nothing".to_string())
        );
    }

    #[test]
    fn a_tool_past_the_deadline_is_killed() {
        let dir = TempDir::new().unwrap();
        let tool = script(dir.path(), "slow", "exec sleep 5");
        let started = Instant::now();
        let probe = run(
            tool.to_str().unwrap(),
            &[],
            Instant::now() + Duration::from_millis(100),
        );
        assert_eq!(probe, Probe::Failed("no answer in time".to_string()));
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn a_tool_whose_child_keeps_its_output_open_still_answers_in_time() {
        let dir = TempDir::new().unwrap();
        let tool = script(dir.path(), "forks", "sleep 30 & echo 1.0");
        let started = Instant::now();
        let probe = run(tool.to_str().unwrap(), &[], soon());
        assert_eq!(probe, Probe::Version("1.0".to_string()));
        assert!(started.elapsed() < Duration::from_secs(10));
    }
}
