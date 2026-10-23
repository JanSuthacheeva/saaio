//! `PaneInput`: the bytes queued for a pane's PTY and the thread that writes them, so the server
//! never waits on a program that stopped reading its input

use std::io::{self, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Sender};

use anyhow::{Result, anyhow, bail};

use super::spawn_named;
use crate::proto::PaneKey;

/// Past this many bytes queued for one pane, more input is refused: its program stopped reading.
const MAX_QUEUED: usize = 16 << 20;

#[derive(Debug)]
pub(super) struct PaneInput {
    bytes: Sender<Vec<u8>>,
    /// Bytes sent to the writer and not yet written.
    queued: Arc<AtomicUsize>,
}

impl PaneInput {
    /// Starts the "pane <key> input" thread on `writer`; a write that fails ends it. Dropped, the
    /// PaneInput lets the thread write what is queued, then drop the writer.
    pub(super) fn start(key: &PaneKey, mut writer: Box<dyn Write + Send>) -> io::Result<PaneInput> {
        let (bytes, queue) = mpsc::channel::<Vec<u8>>();
        let queued = Arc::new(AtomicUsize::new(0));
        let written = Arc::clone(&queued);
        spawn_named(format!("pane {key} input"), move || {
            for chunk in queue {
                if writer.write_all(&chunk).is_err() {
                    return;
                }
                written.fetch_sub(chunk.len(), Ordering::Relaxed);
            }
        })?;
        Ok(PaneInput { bytes, queued })
    }

    /// Queues `bytes`; Err when the thread ended or more than MAX_QUEUED bytes would wait.
    pub(super) fn send(&self, bytes: &[u8]) -> Result<()> {
        let len = bytes.len();
        if self.queued.fetch_add(len, Ordering::Relaxed) + len > MAX_QUEUED {
            self.queued.fetch_sub(len, Ordering::Relaxed);
            bail!("it stopped reading: {} MiB queued", MAX_QUEUED >> 20);
        }
        self.bytes
            .send(bytes.to_vec())
            .map_err(|_| anyhow!("it takes no more input"))
    }
}

#[cfg(test)]
mod tests {
    use std::io::Read;
    use std::os::unix::net::UnixStream;
    use std::time::{Duration, Instant};

    use super::*;

    #[test]
    fn never_waits_on_a_program_that_stopped_reading_and_refuses_past_the_limit() {
        let (ours, _theirs) = UnixStream::pair().unwrap();
        let input = PaneInput::start(&PaneKey::Master, Box::new(ours)).unwrap();
        let megabyte = vec![b'x'; 1 << 20];

        let started = Instant::now();
        let taken = (0..20)
            .take_while(|_| input.send(&megabyte).is_ok())
            .count();

        assert!(
            started.elapsed() < Duration::from_secs(1),
            "{:?}",
            started.elapsed()
        );
        assert!((15..=16).contains(&taken), "{taken} taken");
        let error = input.send(&megabyte).unwrap_err();
        assert!(error.to_string().contains("stopped reading"), "{error}");
    }

    #[test]
    fn writes_what_it_queued_in_order_even_once_dropped() {
        let (ours, mut theirs) = UnixStream::pair().unwrap();
        theirs
            .set_read_timeout(Some(Duration::from_secs(30)))
            .unwrap();
        let input = PaneInput::start(&PaneKey::Master, Box::new(ours)).unwrap();

        let long = vec![b'x'; 300_000];
        for chunk in [&b"one "[..], &long, b" two"] {
            input.send(chunk).unwrap();
        }
        drop(input);

        let mut all = Vec::new();
        theirs.read_to_end(&mut all).unwrap();
        assert_eq!(all.len(), 300_008);
        assert!(all.starts_with(b"one x") && all.ends_with(b"x two"));
    }

    #[test]
    fn says_so_once_its_writer_failed() {
        let (ours, theirs) = UnixStream::pair().unwrap();
        let input = PaneInput::start(&PaneKey::Master, Box::new(ours)).unwrap();
        drop(theirs);

        let refused = (0..1000).any(|_| {
            std::thread::sleep(Duration::from_millis(10));
            input.send(b"x").is_err()
        });

        assert!(refused);
    }
}
