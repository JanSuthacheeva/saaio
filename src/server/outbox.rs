//! `Outbox`: a connection's messages queued for a thread that writes them, so the server never
//! waits on a client that stopped reading

use std::io::{self, Write};
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Sender};

use anyhow::{Result, anyhow, bail};

use super::{ConnId, Event, WRITE_TIMEOUT, spawn_named};
use crate::proto::{self, ServerMsg};

/// Past this many bytes queued for one connection it is closed: its client stopped reading.
pub(super) const MAX_QUEUED: usize = 2 * proto::MAX_MESSAGE;

#[derive(Debug)]
pub(super) struct Outbox {
    frames: Sender<Vec<u8>>,
    /// Bytes sent to the writer and not yet written.
    queued: Arc<AtomicUsize>,
}

impl Outbox {
    /// Starts the "conn <n> writer" thread on `stream`, whose writes time out after
    /// WRITE_TIMEOUT; a write that fails sends Event::Gone(conn) and ends it. Dropped, the
    /// Outbox lets the thread write what is queued, then shut the stream down.
    pub(super) fn start(
        conn: ConnId,
        mut stream: UnixStream,
        events: Sender<Event>,
    ) -> io::Result<Outbox> {
        let (frames, queue) = mpsc::channel::<Vec<u8>>();
        let queued = Arc::new(AtomicUsize::new(0));
        let written = Arc::clone(&queued);
        // macOS refuses socket options once the client hung up (EINVAL); the write says so.
        let _ = stream.set_write_timeout(Some(WRITE_TIMEOUT));
        spawn_named(format!("conn {conn} writer"), move || {
            for bytes in queue {
                if stream.write_all(&bytes).is_err() {
                    let _ = events.send(Event::Gone(conn));
                    return;
                }
                written.fetch_sub(bytes.len(), Ordering::Relaxed);
            }
            let _ = stream.shutdown(Shutdown::Both);
        })?;
        Ok(Outbox { frames, queued })
    }

    /// Queues `message`; Err when it cannot be encoded or would put more than MAX_QUEUED bytes
    /// in the queue.
    pub(super) fn send(&self, message: &ServerMsg) -> Result<()> {
        let bytes = proto::encode(message)?;
        let len = bytes.len();
        if self.queued.fetch_add(len, Ordering::Relaxed) + len > MAX_QUEUED {
            self.queued.fetch_sub(len, Ordering::Relaxed);
            bail!("stopped reading: {} MiB queued", MAX_QUEUED >> 20);
        }
        self.frames
            .send(bytes)
            .map_err(|_| anyhow!("the connection is closed"))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    use super::*;

    fn megabyte() -> ServerMsg {
        ServerMsg::Clipboard("x".repeat(1 << 20))
    }

    #[test]
    fn never_waits_on_a_client_that_stopped_reading_and_refuses_past_the_limit() {
        let (ours, _theirs) = UnixStream::pair().unwrap();
        let (events, _inbox) = mpsc::channel();
        let outbox = Outbox::start(1, ours, events).unwrap();

        let started = Instant::now();
        let taken = (0..20)
            .take_while(|_| outbox.send(&megabyte()).is_ok())
            .count();

        assert!(
            started.elapsed() < Duration::from_secs(1),
            "{:?}",
            started.elapsed()
        );
        assert!((14..=16).contains(&taken), "{taken} taken");
        let error = outbox.send(&megabyte()).unwrap_err();
        assert!(error.to_string().contains("stopped reading"), "{error}");
    }

    #[test]
    fn a_dropped_outbox_still_delivers_what_it_queued() {
        let (ours, mut theirs) = UnixStream::pair().unwrap();
        theirs
            .set_read_timeout(Some(Duration::from_secs(30)))
            .unwrap();
        let (events, _inbox) = mpsc::channel();
        let outbox = Outbox::start(1, ours, events).unwrap();
        let messages = [
            ServerMsg::Closed("a".into()),
            megabyte(),
            ServerMsg::Refused {
                reason: "bye".into(),
            },
        ];

        for message in &messages {
            outbox.send(message).unwrap();
        }
        drop(outbox);

        for message in messages {
            assert_eq!(proto::read_msg(&mut theirs).unwrap(), Some(message));
        }
        assert_eq!(proto::read_msg::<_, ServerMsg>(&mut theirs).unwrap(), None);
    }

    #[test]
    fn a_write_that_fails_says_the_connection_is_gone() {
        let (ours, theirs) = UnixStream::pair().unwrap();
        let (events, inbox) = mpsc::channel();
        let outbox = Outbox::start(7, ours, events).unwrap();
        drop(theirs);

        outbox.send(&ServerMsg::Closed("a".into())).unwrap();

        let event = inbox.recv_timeout(Duration::from_secs(30)).unwrap();
        assert!(matches!(event, Event::Gone(7)), "{event:?}");
    }
}
