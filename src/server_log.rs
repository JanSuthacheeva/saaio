//! server.log: rotation, stamped lines, and the panic hooks of the server and the UI

use std::backtrace::Backtrace;
use std::fs::{self, File};
use std::io::{self, Write};
use std::os::fd::AsFd;
use std::os::unix::fs::MetadataExt;
use std::panic::Location;
use std::path::{Path, PathBuf};
use std::{process, thread};

use crate::private;
use crate::text;

/// The size past which server.log moves to server.log.1.
pub const CAP: u64 = 1 << 20;

/// server.log beside the socket.
pub fn path(socket: &Path) -> PathBuf {
    socket.with_extension("log")
}

/// Opens `log` to append, first moving it to `<log>.1` when it is past `cap` (one that cannot be
/// moved is only longer). Under an flock on the log (none where the file system refuses one), and
/// again when another writer moved it meanwhile, so racing writers move it once. The lock is let
/// go before it returns: the file may become a server's stderr, which would hold it for good.
pub fn open(log: &Path, cap: u64) -> io::Result<File> {
    open_with(log, cap, File::lock)
}

/// `open`, taking the lock with `lock`.
fn open_with(log: &Path, cap: u64, lock: impl Fn(&File) -> io::Result<()>) -> io::Result<File> {
    loop {
        let file = private::options().create(true).append(true).open(log)?;
        // A lock the file system refuses is no lock: it only keeps writers from rotating twice.
        let locked = lock(&file).is_ok();
        let meta = file.metadata()?;
        let moved = match fs::metadata(log) {
            Ok(now) => (now.dev(), now.ino()) != (meta.dev(), meta.ino()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => true,
            Err(error) => return Err(error),
        };
        if moved {
            continue;
        }
        // A log that cannot be moved aside is only longer.
        if meta.len() <= cap || fs::rename(log, older(log)).is_err() {
            if locked {
                file.unlock()?;
            }
            return Ok(file);
        }
    }
}

/// Whether stderr is the file at `log`: a server ensure_server started, not one in a terminal.
pub fn is_stderr(log: &Path) -> bool {
    let (Ok(stderr), Ok(file)) = (stderr_meta(), fs::metadata(log)) else {
        return false;
    };
    (stderr.dev(), stderr.ino()) == (file.dev(), file.ino())
}

/// Points stdout and stderr at `log` (through open, so past `cap` it moves aside first) once
/// stderr is a file no longer at `log` (another writer moved it to `<log>.1`) or past `cap`.
/// Nothing when stderr is not a regular file.
pub fn follow(log: &Path, cap: u64) -> io::Result<()> {
    let stderr = stderr_meta()?;
    if !stderr.is_file() || (is_stderr(log) && stderr.len() <= cap) {
        return Ok(());
    }
    let file = open(log, cap)?;
    rustix::stdio::dup2_stdout(&file)?;
    rustix::stdio::dup2_stderr(&file)?;
    Ok(())
}

/// What stderr is: a file, a terminal, a pipe.
fn stderr_meta() -> io::Result<fs::Metadata> {
    File::from(io::stderr().as_fd().try_clone_to_owned()?).metadata()
}

/// `<log>.1`, where a log past its cap goes.
fn older(log: &Path) -> PathBuf {
    let mut old = log.as_os_str().to_owned();
    old.push(".1");
    PathBuf::from(old)
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

/// Appends a line to `log` through `open`, for a process that is not the server (saaio --stop).
pub fn append(log: &Path, now: u64, text: &str) -> io::Result<()> {
    let line = line(now, text::local_offset_secs(), process::id(), text) + "\n";
    open(log, CAP)?.write_all(line.as_bytes())
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

/// From now on a panic appends `<source>: panic ...` and its backtrace to `log`, then runs the hook
/// before it; for a process whose stderr is not server.log (the UI).
pub fn append_on_panic(log: PathBuf, source: &'static str, clock: fn() -> u64) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let message = info.payload_as_str().unwrap_or("a panic without a message");
        let text = panic_text(thread::current().name(), info.location(), message);
        let backtrace = Backtrace::force_capture();
        let _ = append(&log, clock(), &format!("{source}: {text}\n{backtrace}"));
        previous(info);
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
    fn opens_a_log_under_the_cap_and_moves_one_past_it_aside() {
        let dir = TempDir::new().unwrap();
        let log = dir.path().join("server.log");
        let old = dir.path().join("server.log.1");

        writeln!(open(&log, 10).unwrap(), "short").unwrap();
        writeln!(open(&log, 10).unwrap(), "more").unwrap();
        assert_eq!(fs::read_to_string(&log).unwrap(), "short\nmore\n");
        assert!(!old.exists());

        writeln!(open(&log, 10).unwrap(), "fresh").unwrap();
        assert_eq!(fs::read_to_string(&old).unwrap(), "short\nmore\n");
        assert_eq!(fs::read_to_string(&log).unwrap(), "fresh\n");
    }

    #[test]
    fn a_lock_the_file_system_refuses_is_no_lock() {
        let dir = TempDir::new().unwrap();
        let log = dir.path().join("server.log");
        let refused = |_: &File| Err(io::Error::from(io::ErrorKind::Unsupported));
        fs::write(&log, "x".repeat(11)).unwrap();

        writeln!(open_with(&log, 10, refused).unwrap(), "fresh").unwrap();

        assert_eq!(
            fs::read_to_string(dir.path().join("server.log.1")).unwrap(),
            "x".repeat(11)
        );
        assert_eq!(fs::read_to_string(&log).unwrap(), "fresh\n");
    }

    #[test]
    fn writers_racing_past_the_cap_move_the_log_aside_once() {
        let dir = TempDir::new().unwrap();
        let log = dir.path().join("server.log");
        let old = dir.path().join("server.log.1");
        let full = "x".repeat(101);
        for _ in 0..50 {
            fs::write(&log, &full).unwrap();
            let start = std::sync::Barrier::new(4);
            thread::scope(|scope| {
                for _ in 0..4 {
                    scope.spawn(|| {
                        start.wait();
                        writeln!(open(&log, 100).unwrap(), "line").unwrap();
                    });
                }
            });
            assert_eq!(fs::read_to_string(&old).unwrap(), full);
            assert_eq!(fs::read_to_string(&log).unwrap(), "line\n".repeat(4));
        }
    }

    #[test]
    fn knows_a_file_that_is_not_stderr() {
        let dir = TempDir::new().unwrap();
        let log = dir.path().join("server.log");

        assert!(!is_stderr(&log));
        fs::write(&log, "").unwrap();
        assert!(!is_stderr(&log));
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
