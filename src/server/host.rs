use std::collections::BTreeMap;
use std::fmt;
use std::io::{ErrorKind, Read, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use portable_pty::{CommandBuilder, MasterPty, PtySize, native_pty_system};

use crate::launch::pane::PaneSpec;
use crate::proto::{Frame, PaneInfo, PaneKey, PaneStatus, Size};
use crate::server::spawn_named;
use crate::server::terminal::Terminal;

/// Lines of scrollback each pane keeps.
const HISTORY: usize = 10_000;
/// How long a closed pane gets to exit after SIGHUP before SIGKILL.
const CLOSE_GRACE: Duration = Duration::from_secs(3);
/// How long an exit waits for the last output still in the PTY.
const DRAIN: Duration = Duration::from_millis(500);

/// What the pane threads report to the thread that owns the Host.
#[derive(Debug, PartialEq, Eq)]
pub enum HostEvent {
    Output(PaneKey, Vec<u8>),
    Exited(PaneKey, Option<u32>),
}

/// Every pane: a PTY, the process in it and the Terminal showing its screen.
#[derive(Debug)]
pub struct Host {
    panes: BTreeMap<PaneKey, Pane>,
    size: Size,
    events: Sender<HostEvent>,
}

#[derive(Debug)]
struct Pane {
    term: Terminal,
    process: Option<Process>,
    status: PaneStatus,
    closing: bool,
}

struct Process {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    pid: libc::pid_t,
    exited: Arc<AtomicBool>,
}

impl fmt::Debug for Process {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Process")
            .field("pid", &self.pid)
            .field("exited", &self.exited)
            .finish_non_exhaustive()
    }
}

impl Host {
    pub fn new(size: Size, events: Sender<HostEvent>) -> Host {
        Host {
            panes: BTreeMap::new(),
            size,
            events,
        }
    }

    /// Spawns the child in a new PTY with a reader and a waiter thread; on Err the pane is kept as Failed.
    pub fn open(&mut self, spec: PaneSpec) -> Result<()> {
        if let Some(pane) = self.panes.get(&spec.key)
            && pane.status == PaneStatus::Running
        {
            bail!("{} is already running", spec.key);
        }
        let key = spec.key.clone();
        match self.spawn(spec) {
            Ok(process) => {
                self.panes.insert(
                    key,
                    Pane {
                        term: Terminal::new(self.size, HISTORY),
                        process: Some(process),
                        status: PaneStatus::Running,
                        closing: false,
                    },
                );
                Ok(())
            }
            Err(error) => {
                self.fail(key, format!("{error:#}"));
                Err(error)
            }
        }
    }

    /// Keeps a pane that could not be opened, with the reason shown in it.
    pub fn fail(&mut self, key: PaneKey, reason: String) {
        let mut term = Terminal::new(self.size, 0);
        term.feed(format!("\x1b[?25l{reason}").as_bytes());
        self.panes.insert(
            key,
            Pane {
                term,
                process: None,
                status: PaneStatus::Failed { reason },
                closing: false,
            },
        );
    }

    /// Feeds output into the pane's Terminal and writes the Terminal's replies (DA, DSR) back to the PTY.
    pub fn feed(&mut self, key: &PaneKey, bytes: &[u8]) {
        let Some(pane) = self.panes.get_mut(key) else {
            return;
        };
        pane.term.feed(bytes);
        let replies = pane.term.replies();
        if let Some(process) = pane.process.as_mut()
            && !replies.is_empty()
        {
            let _ = process.writer.write_all(&replies);
        }
    }

    /// Writes input to a running pane; Ok without writing to an exited one.
    pub fn write(&mut self, key: &PaneKey, bytes: &[u8]) -> Result<()> {
        let Some(pane) = self.panes.get_mut(key) else {
            return Ok(());
        };
        if pane.status != PaneStatus::Running {
            return Ok(());
        }
        if let Some(process) = pane.process.as_mut() {
            process
                .writer
                .write_all(bytes)
                .with_context(|| format!("could not write to {key}"))?;
        }
        Ok(())
    }

    /// Every pane to the same size, PTY and Terminal.
    pub fn resize(&mut self, size: Size) {
        if size == self.size {
            return;
        }
        self.size = size;
        for pane in self.panes.values_mut() {
            pane.term.resize(size);
            if let Some(process) = &pane.process {
                let _ = process.master.resize(pty_size(size));
            }
        }
    }

    pub fn exited(&mut self, key: &PaneKey, code: Option<u32>) {
        let Some(pane) = self.panes.get_mut(key) else {
            return;
        };
        if pane.closing {
            self.panes.remove(key);
            return;
        }
        pane.status = PaneStatus::Exited { code };
        pane.process = None;
    }

    /// SIGHUP to the child's process group, SIGKILL after 3 s; the pane is gone once it exits.
    pub fn close(&mut self, key: &PaneKey) {
        let Some(pane) = self.panes.get_mut(key) else {
            return;
        };
        let Some(process) = &pane.process else {
            self.panes.remove(key);
            return;
        };
        pane.closing = true;
        let pid = process.pid;
        let exited = process.exited.clone();
        signal_group(pid, libc::SIGHUP);
        spawn_named(format!("pane {key} close"), move || {
            thread::sleep(CLOSE_GRACE);
            if !exited.load(Ordering::SeqCst) {
                signal_group(pid, libc::SIGKILL);
            }
        });
    }

    /// The pane's rows: all when full, else the damaged ones; scrolled back by history lines,
    /// at most as far as the history goes (the frame says how far that was).
    pub fn frame(&mut self, key: &PaneKey, full: bool, history: Option<u32>) -> Option<Frame> {
        let pane = self.panes.get_mut(key)?;
        let damaged = pane.term.damaged();
        let rows: Vec<u16> = if full || history.is_some() {
            (0..self.size.rows).collect()
        } else {
            damaged
        };
        let history = history.map(|back| back.min(pane.term.history_len()));
        let back = history.unwrap_or(0);
        Some(Frame {
            key: key.clone(),
            full: full || history.is_some(),
            rows: rows
                .into_iter()
                .map(|row| (row, pane.term.row(row, back)))
                .collect(),
            cursor: if history.is_some() {
                None
            } else {
                pane.term.cursor()
            },
            modes: pane.term.modes(),
            history,
        })
    }

    /// Where the pane stands; None when there is no such pane.
    pub fn status(&self, key: &PaneKey) -> Option<&PaneStatus> {
        self.panes.get(key).map(|pane| &pane.status)
    }

    /// A bounded plain-text tail of one pane's visible output.
    pub fn output_tail(&self, key: &PaneKey, max_lines: usize, max_chars: usize) -> Option<String> {
        self.panes
            .get(key)
            .map(|pane| pane.term.output_tail(max_lines, max_chars))
    }

    pub fn keys(&self) -> impl Iterator<Item = &PaneKey> {
        self.panes.keys()
    }

    pub fn any_running(&self) -> bool {
        self.panes
            .values()
            .any(|pane| pane.status == PaneStatus::Running)
    }

    pub fn panes(&self) -> Vec<PaneInfo> {
        self.panes
            .iter()
            .map(|(key, pane)| PaneInfo {
                key: key.clone(),
                status: pane.status.clone(),
            })
            .collect()
    }

    /// Ends synchronized updates their programs left open too long.
    pub fn expire_sync(&mut self) {
        for pane in self.panes.values_mut() {
            pane.term.expire_sync();
        }
    }

    fn spawn(&self, spec: PaneSpec) -> Result<Process> {
        let (program, args) = spec.argv.split_first().context("empty command")?;
        let pair = native_pty_system()
            .openpty(pty_size(self.size))
            .context("could not open a pseudo-terminal")?;
        let mut command = CommandBuilder::new(program);
        command.args(args);
        command.cwd(&spec.cwd);
        for (name, value) in &spec.env {
            command.env(name, value);
        }
        let mut child = pair
            .slave
            .spawn_command(command)
            .with_context(|| format!("could not start {program}"))?;
        drop(pair.slave);
        let id = child
            .process_id()
            .context("the started process has no id")?;
        let pid = libc::pid_t::try_from(id).with_context(|| format!("pid {id} is out of range"))?;
        let mut reader = pair.master.try_clone_reader()?;
        let writer = pair.master.take_writer()?;
        let exited = Arc::new(AtomicBool::new(false));

        let (drained_tx, drained_rx) = mpsc::channel::<()>();
        let events = self.events.clone();
        let key = spec.key.clone();
        spawn_named(format!("pane {key} output"), move || {
            let mut buf = vec![0u8; 64 * 1024];
            loop {
                match reader.read(&mut buf) {
                    Err(error) if error.kind() == ErrorKind::Interrupted => {}
                    Ok(0) | Err(_) => break,
                    Ok(read) => {
                        if events
                            .send(HostEvent::Output(key.clone(), buf[..read].to_vec()))
                            .is_err()
                        {
                            break;
                        }
                    }
                }
            }
            let _ = drained_tx.send(());
        });

        let events = self.events.clone();
        let key = spec.key;
        let done = exited.clone();
        spawn_named(format!("pane {key} wait"), move || {
            let code = child.wait().ok().map(|status| status.exit_code());
            let _ = drained_rx.recv_timeout(DRAIN);
            done.store(true, Ordering::SeqCst);
            let _ = events.send(HostEvent::Exited(key, code));
        });

        Ok(Process {
            master: pair.master,
            writer,
            pid,
            exited,
        })
    }
}

fn pty_size(size: Size) -> PtySize {
    PtySize {
        rows: size.rows.max(1),
        cols: size.cols.max(1),
        pixel_width: 0,
        pixel_height: 0,
    }
}

/// The child leads its own session (portable-pty calls setsid), so its group id is its pid.
fn signal_group(pid: libc::pid_t, signal: libc::c_int) {
    // SAFETY: kill only sends a signal; a stale group id fails with ESRCH and changes nothing.
    unsafe {
        libc::kill(-pid, signal);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc::Receiver;
    use std::time::Instant;

    use super::*;

    fn host() -> (Host, Receiver<HostEvent>) {
        let (events, rx) = mpsc::channel();
        (Host::new(Size { cols: 40, rows: 5 }, events), rx)
    }

    fn spec(key: PaneKey, script: &str) -> PaneSpec {
        PaneSpec {
            key,
            argv: vec!["sh".into(), "-c".into(), script.into()],
            cwd: std::env::temp_dir(),
            env: vec![("GREETING".into(), "hi there".into())],
            adapter: None,
        }
    }

    /// Feeds output into the host until the pane exits or the time is up; the exit code.
    fn pump(host: &mut Host, rx: &Receiver<HostEvent>, limit: Duration) -> Option<Option<u32>> {
        let deadline = Instant::now() + limit;
        while let Some(left) = deadline.checked_duration_since(Instant::now()) {
            match rx.recv_timeout(left) {
                Ok(HostEvent::Output(key, bytes)) => host.feed(&key, &bytes),
                Ok(HostEvent::Exited(key, code)) => {
                    host.exited(&key, code);
                    return Some(code);
                }
                Err(_) => break,
            }
        }
        None
    }

    fn screen(host: &mut Host, key: &PaneKey) -> String {
        host.frame(key, true, None)
            .unwrap()
            .rows
            .iter()
            .map(|(_, cells)| {
                cells
                    .iter()
                    .map(|cell| cell.text.as_str())
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn shows_the_output_and_exit_code_of_a_process_in_its_environment() {
        let (mut host, rx) = host();
        let key = PaneKey::Flow("a".into());

        host.open(spec(key.clone(), "printf \"$GREETING\"; exit 3"))
            .unwrap();

        assert_eq!(pump(&mut host, &rx, Duration::from_secs(5)), Some(Some(3)));
        assert!(screen(&mut host, &key).starts_with("hi there"));
        assert_eq!(host.panes()[0].status, PaneStatus::Exited { code: Some(3) });
        host.write(&key, b"ignored").unwrap();
    }

    #[test]
    fn output_tail_reads_an_exited_pane() {
        let (mut host, rx) = host();
        let key = PaneKey::Flow("tail".into());
        host.open(spec(key.clone(), "printf 'one\\ntwo\\n'; exit 23"))
            .unwrap();

        assert_eq!(pump(&mut host, &rx, Duration::from_secs(5)), Some(Some(23)));
        assert_eq!(host.output_tail(&key, 2, 20).as_deref(), Some("one\ntwo"));
        assert_eq!(host.output_tail(&PaneKey::Master, 2, 20), None);
    }

    #[test]
    fn passes_input_through_and_follows_a_resize() {
        let (mut host, rx) = host();
        let key = PaneKey::Master;
        host.open(spec(
            key.clone(),
            "read line; stty size; echo \"got $line\"",
        ))
        .unwrap();

        host.resize(Size { cols: 70, rows: 8 });
        host.write(&key, b"abc\r").unwrap();

        assert_eq!(pump(&mut host, &rx, Duration::from_secs(5)), Some(Some(0)));
        let screen = screen(&mut host, &key);
        assert!(screen.contains("8 70"), "{screen}");
        assert!(screen.contains("got abc"), "{screen}");
        assert_eq!(host.frame(&key, true, None).unwrap().rows.len(), 8);
    }

    #[test]
    fn close_ends_a_process_that_ignores_hangup() {
        let (mut host, rx) = host();
        let key = PaneKey::Flow("stubborn".into());
        host.open(spec(key.clone(), "trap '' HUP; echo ready; sleep 30"))
            .unwrap();
        let started = Instant::now();
        while !screen(&mut host, &key).contains("ready") {
            if let Ok(HostEvent::Output(key, bytes)) = rx.recv_timeout(Duration::from_secs(5)) {
                host.feed(&key, &bytes);
            }
            assert!(started.elapsed() < Duration::from_secs(5));
        }

        host.close(&key);

        assert!(pump(&mut host, &rx, Duration::from_secs(6)).is_some());
        assert!(host.panes().is_empty());
    }

    #[test]
    fn keeps_a_pane_that_could_not_start_with_its_reason() {
        let (mut host, _rx) = host();
        let key = PaneKey::Flow("broken".into());

        let error = host
            .open(PaneSpec {
                key: key.clone(),
                argv: vec!["/no/such/program".into()],
                cwd: std::env::temp_dir(),
                env: vec![],
                adapter: None,
            })
            .unwrap_err();

        assert!(
            format!("{error:#}").contains("/no/such/program"),
            "{error:#}"
        );
        assert!(matches!(host.panes()[0].status, PaneStatus::Failed { .. }));
        assert!(screen(&mut host, &key).contains("/no/such/program"));
        host.fail(PaneKey::Master, "no agent named x".into());
        assert!(screen(&mut host, &PaneKey::Master).contains("no agent named x"));
    }

    #[test]
    fn sends_only_changed_rows_after_the_first_frame() {
        let (mut host, rx) = host();
        let key = PaneKey::Master;
        host.open(spec(
            key.clone(),
            "printf 'one\\ntwo'; read x; printf '\\033[1;1Hxx'; read y",
        ))
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !screen(&mut host, &key).contains("two") && Instant::now() < deadline {
            if let Ok(HostEvent::Output(key, bytes)) = rx.recv_timeout(Duration::from_millis(200)) {
                host.feed(&key, &bytes);
            }
        }
        host.frame(&key, true, None);

        host.write(&key, b"\r").unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut rows = Vec::new();
        while Instant::now() < deadline {
            if let Ok(HostEvent::Output(key, bytes)) = rx.recv_timeout(Duration::from_millis(200)) {
                host.feed(&key, &bytes);
                let frame = host.frame(&key, false, None).unwrap();
                rows.extend(frame.rows.iter().map(|(row, _)| *row));
                if screen_row(&frame, 0).starts_with("xx") {
                    break;
                }
            }
        }

        assert!(rows.contains(&0), "{rows:?}");
        assert!(!rows.contains(&4), "{rows:?}");
        host.close(&key);
    }

    fn screen_row(frame: &Frame, row: u16) -> String {
        frame
            .rows
            .iter()
            .find(|(index, _)| *index == row)
            .map(|(_, cells)| cells.iter().map(|cell| cell.text.as_str()).collect())
            .unwrap_or_default()
    }

    #[test]
    fn scrolls_back_no_further_than_the_history_goes() {
        let (mut host, rx) = host();
        let key = PaneKey::Master;
        host.open(spec(key.clone(), "seq 1 40; read x")).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !screen(&mut host, &key).contains("40") && Instant::now() < deadline {
            if let Ok(HostEvent::Output(key, bytes)) = rx.recv_timeout(Duration::from_millis(200)) {
                host.feed(&key, &bytes);
            }
        }

        let top = host.frame(&key, false, Some(u32::MAX)).unwrap();

        let back = top.history.unwrap();
        assert!(back > 0 && back < 40, "{back}");
        assert_eq!(screen_row(&top, 0).trim(), "1");
        assert!(top.full && top.cursor.is_none());
        host.close(&key);
    }
}
