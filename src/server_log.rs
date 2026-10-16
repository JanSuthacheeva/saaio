use std::backtrace::Backtrace;
use std::fs;
use std::fs::OpenOptions;
use std::io::{self, Write};
use std::panic::Location;
use std::path::{Path, PathBuf};
use std::{process, thread};

use crate::text;

/// The size past which ensure_server moves server.log to server.log.1.
pub const CAP: u64 = 1 << 20;

/// server.log beside the socket.
pub fn path(socket: &Path) -> PathBuf {
    socket.with_extension("log")
}

/// Moves a log over `cap` bytes to `<log>.1`, replacing the one before; nothing when under it or
/// missing.
pub fn rotate(log: &Path, cap: u64) -> io::Result<()> {
    match fs::metadata(log) {
        Ok(meta) if meta.len() > cap => {
            let mut old = log.as_os_str().to_owned();
            old.push(".1");
            fs::rename(log, old)
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

/// One line: `2026-10-16 13:39:54 +07:00 [2739] text`.
pub fn line(at_ms: u64, offset_secs: i64, pid: u32, text: &str) -> String {
    format!("{} [{pid}] {text}", text::stamp(at_ms, offset_secs))
}

/// Writes a line to stderr, which is server.log in a server ensure_server started.
pub fn write(now: u64, text: &str) {
    let line = line(now, text::local_offset_secs(), process::id(), text) + "\n";
    let _ = io::stderr().write_all(line.as_bytes());
}

/// Appends a line to `log`, for a process that is not the server (flow ui --stop).
pub fn append(log: &Path, now: u64, text: &str) -> io::Result<()> {
    let line = line(now, text::local_offset_secs(), process::id(), text) + "\n";
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(log)?
        .write_all(line.as_bytes())
}

/// From now on a panic in any thread writes its line and backtrace, then ends the process (exit
/// 101): a server that lost a thread would keep its panes but stop listening.
pub fn exit_on_panic(clock: fn() -> u64) {
    std::panic::set_hook(Box::new(move |info| {
        let message = info.payload_as_str().unwrap_or("a panic without a message");
        write(
            clock(),
            &panic_text(thread::current().name(), info.location(), message),
        );
        let _ = writeln!(io::stderr(), "{}", Backtrace::force_capture());
        process::exit(101);
    }));
}

/// `panic in thread 'accept' at src/server/conn.rs:212:9: message`.
fn panic_text(thread: Option<&str>, location: Option<&Location>, message: &str) -> String {
    let thread = match thread {
        Some(name) => format!("in thread '{name}'"),
        None => "in an unnamed thread".to_string(),
    };
    match location {
        Some(at) => format!("panic {thread} at {at}: {message}"),
        None => format!("panic {thread}: {message}"),
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    #[test]
    fn moves_a_log_over_the_cap_aside_and_keeps_one_under_it() {
        let dir = TempDir::new().unwrap();
        let log = dir.path().join("server.log");
        let old = dir.path().join("server.log.1");

        rotate(&log, 10).unwrap();
        assert!(!log.exists());

        fs::write(&log, "short").unwrap();
        rotate(&log, 10).unwrap();
        assert_eq!(fs::read_to_string(&log).unwrap(), "short");

        fs::write(&old, "older").unwrap();
        fs::write(&log, "longer than ten").unwrap();
        rotate(&log, 10).unwrap();
        assert!(!log.exists());
        assert_eq!(fs::read_to_string(&old).unwrap(), "longer than ten");
    }

    #[test]
    fn appends_a_line_and_creates_the_log() {
        let dir = TempDir::new().unwrap();
        let log = dir.path().join("server.log");

        append(&log, 0, "one").unwrap();
        append(&log, 0, "two").unwrap();

        let lines = fs::read_to_string(&log).unwrap();
        let ends: Vec<_> = lines
            .lines()
            .map(|line| line.rsplit("] ").next().unwrap())
            .collect();
        assert_eq!(ends, ["one", "two"]);
    }

    #[test]
    fn stamps_a_line_with_time_and_pid() {
        assert_eq!(
            line(1_792_132_794_000, 7 * 3_600, 2739, "stopped"),
            "2026-10-16 13:39:54 +07:00 [2739] stopped"
        );
    }

    #[test]
    fn names_the_thread_and_place_of_a_panic() {
        let at = Location::caller();
        assert_eq!(
            panic_text(Some("accept"), Some(at), "boom"),
            format!("panic in thread 'accept' at {at}: boom")
        );
        assert_eq!(
            panic_text(None, None, "boom"),
            "panic in an unnamed thread: boom"
        );
    }

    #[test]
    fn lives_beside_the_socket() {
        assert_eq!(
            path(Path::new("/state/server.sock")),
            Path::new("/state/server.log")
        );
    }
}
