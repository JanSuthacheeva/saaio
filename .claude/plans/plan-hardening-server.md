# Hardening the server and hooks (0.1.2)

Implementation page: https://claude.ai/artifact/2kpw3qywYiVznKS2fctZpH
(approaches: https://claude.ai/artifact/3dtRcAwRgtxHATrWdHF3qF)

## Decision

- 1A: each connection gets an `Outbox`, a queue written by a thread of its own. The server thread
  never writes to a socket; a connection with more than 16 MiB queued is closed. The 2 s write
  timeout stays, on the writer thread.
- 2A: every appender opens server.log through `server_log::open`, which rotates under an flock; the
  server re-points its stdout and stderr at the fresh log on its 500 ms tick with rustix
  `dup2_stdout`/`dup2_stderr` (feature `stdio`; `unsafe_code` stays forbidden).
- The findings from reading the code are fixed too, each proven by a test that fails first:
  hello deadline for the whole hello, read_msg buffer growing with the bytes, sizes clamped,
  peer texts cut, merge request URLs only `/pull/<n>` and `/-/merge_requests/<n>`, report texts
  printable, at most 8 questions and options, hook stdin capped at 16 MiB.
- No stored format changes: event log, `--json` view and `PROTOCOL_VERSION` unchanged.
- These are unannounced security fixes: CHANGELOG lines are plain user-facing fixes, no audit
  ids, no exploit detail; nothing from ROADMAP.internal.md goes into commits or the PR.

## Units

`src/server/outbox.rs` (new; `mod outbox;` in server.rs, a line in CLAUDE.md's Architecture)

```rust
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
    pub(super) fn start(conn: ConnId, stream: UnixStream, events: Sender<Event>) -> io::Result<Outbox>;

    /// Queues `message`; Err when it cannot be encoded or would put more than MAX_QUEUED
    /// bytes in the queue ("stopped reading: 16 MiB queued").
    pub(super) fn send(&self, message: &ServerMsg) -> Result<()>;
}
```

`src/server/conn.rs` (modified)

```rust
pub(super) struct Conn {
    /// Kept to shut its reading side when the connection is dropped.
    stream: UnixStream,
    outbox: Outbox,
    /// When it was accepted, for the hello deadline.
    opened: Instant,
    peer: Option<Peer>,
    role: Option<Role>,
}

impl Server {
    // changed: queues through the Outbox; a connection it cannot take is logged
    // ("dropped <peer>: stopped reading: 16 MiB queued") and dropped.
    pub(super) fn send_to(&mut self, conn: ConnId, message: &ServerMsg);
    // changed: shuts only the reading side and drops the Conn, so its Outbox still writes a
    // Refused or Detached queued before it.
    pub(super) fn drop_conn(&mut self, conn: ConnId);
    /// Refuses every connection without a hello opened more than timing.hello ago.
    pub(super) fn close_silent(&mut self);
    // changed: a connection already dropped (by close_silent) is let go silently.
    pub(super) fn refuse_unreadable(&mut self, conn: ConnId);
}
```

- `message()` clamps the size of `Hello { role: Attach { size } }` and `Resize` with
  `Size::clamped` before Host sees it.
- `accept()` starts the Outbox; no write timeout on the server's own stream.
- `read_in_background` drops its per-read hello timeout; `close_silent` keeps the deadline.

`src/server.rs` (modified)

```rust
struct Server {
    // ...
    /// server.log beside the socket, which stderr writes to in a server ensure_server started.
    log: PathBuf,
}
```

- `serve()`: on each WATCH_EVERY tick also `self.close_silent()` and
  `server_log::follow(&self.log, server_log::CAP)` (an error is let go: it cannot be logged).
- WRITE_TIMEOUT is the Outbox writer's; its doc says so.

`src/proto.rs` (modified)

```rust
/// The widest and tallest pane the server makes: past it a terminal's cells cost too much.
pub const MAX_SIZE: Size = Size { cols: 1000, rows: 500 };
/// The longest folder or build a peer keeps, in chars.
const PEER_TEXT_LIMIT: usize = 4096;

/// A message as its u32 length and postcard bytes, ready to write; Err past MAX_MESSAGE.
pub fn encode<T: Serialize>(message: &T) -> Result<Vec<u8>>;

impl Size {
    /// This size within 1..=MAX_SIZE.cols by 1..=MAX_SIZE.rows.
    pub fn clamped(self) -> Size;
}
```

- `write_msg` writes `encode(message)`.
- `read_msg` grows its buffer with the bytes that arrive (`reader.take(len)`), so a length alone
  never allocates.
- `Peer::printable` also cuts cwd and build to PEER_TEXT_LIMIT chars.

`src/server_log.rs` (modified)

```rust
/// Opens `log` to append, first moving it to `<log>.1` when it is past `cap`; under an flock
/// on the log, and again when another writer moved it meanwhile, so racing writers rotate once.
pub fn open(log: &Path, cap: u64) -> io::Result<File>;

/// Points stdout and stderr at `log` (through open, so past `cap` it rotates) once stderr is
/// a file that is no longer the one at `log` (a writer moved it to `<log>.1`) or is past
/// `cap`. Nothing when stderr is not a regular file, as for a server run in a terminal.
pub fn follow(log: &Path, cap: u64) -> io::Result<()>;
```

- `append` writes through `open(log, CAP)`.
- `rotate` is removed, folded into `open`.

`src/client.rs` (modified): `ensure_server` opens the log with `server_log::open(&log_path, CAP)`
instead of `rotate` + `private::options().open`.

`src/cli/commands/hook.rs` (modified)

```rust
/// The most of an event saaio hook reads.
const MAX_EVENT: u64 = 16 << 20;

/// The event `input` carries; Err when it is past MAX_EVENT ("the event is over 16 MiB"),
/// not UTF-8, or cannot be read.
fn read_event(input: impl Read) -> Result<String>;
```

- `run`: `read_event(io::stdin().lock())` replaces `read_to_string`; an Err is logged, Silent.

`src/adapter.rs`, `adapter/calls.rs`, `adapter/claude_code.rs`, `adapter/codex.rs` (modified)

```rust
// adapter.rs - changed: text::printable, then REPORT_TEXT_LIMIT chars.
pub fn cut(text: &str) -> String;
/// The most questions, and options per question, an event keeps.
pub const MAX_QUESTIONS: usize = 8;

// calls.rs
/// `word` when it is an http(s) URL of printable ASCII whose path ends in /pull/<n> or
/// /-/merge_requests/<n>, n digits.
fn merge_request(word: &str) -> Option<&str>;
```

- `created_merge_request` takes the last word `merge_request` accepts.
- `in_repo` requires the rest after the repo path to be `pull/<n>` or `-/merge_requests/<n>`.
- claude_code / codex: every header, question, label and description passes through `cut`; at
  most MAX_QUESTIONS questions and options each. Audit: every other payload text that reaches an
  event goes through `cut`.

## Data

- `Outbox`: `Vec<u8>` frames (length prefix included) over an mpsc channel, server thread ->
  writer thread; `queued` counts bytes across both.
- `Conn.opened: Instant`, `Server.log: PathBuf`.
- Constants: MAX_QUEUED 16 MiB, MAX_EVENT 16 MiB, MAX_SIZE 1000 x 500, PEER_TEXT_LIMIT 4096,
  MAX_QUESTIONS 8.

## Call chains

A frame to a UI

1. `Server::flush -> Server::send_to(conn, &ServerMsg::Frame(frame))`
2. `send_to -> Outbox::send(&message): Result<()>`
3. `Outbox::send -> proto::encode(message): Result<Vec<u8>>`, `queued += len`, `frames.send(bytes)`
4. writer thread: `frames.recv() -> stream.write_all(&bytes)`, `queued -= len`

- Queue would pass 16 MiB, or the message does not encode: `send` Err -> `send_to` logs
  "dropped <peer>: stopped reading: 16 MiB queued" -> `drop_conn`. The UI sees its connection
  end, as after a write timeout today.
- A write fails or times out (2 s): writer sends `Event::Gone(conn)`, ends -> `Server::drop_conn`.

A connection closed with a last message (Refused, Detached)

1. `Server::hello -> send_to(old, &ServerMsg::Detached { by }) -> drop_conn(old)`
2. `drop_conn -> conn.stream.shutdown(Read)`; the Conn and its Outbox are dropped; the reader
   thread ends with Ok(None) -> `Event::Gone` for a conn already gone: nothing.
3. writer thread: writes what is queued until the channel ends, then `shutdown(Both)`.

The hello deadline

1. `Server::serve` tick (WATCH_EVERY) `-> Server::close_silent()`
2. each Conn with `role == None` and `opened.elapsed() >= timing.hello`: log "closed a
   connection that said no hello in 5 s", `send_to(Refused)`, `drop_conn`.

- Undecodable first message: unchanged, `Event::Unreadable -> refuse_unreadable`. A message
  before the hello: unchanged, `proto::allowed` refuses it, the connection is dropped.

A hook appends to server.log

1. `hook::Log::error -> server_log::append(path, now, line)`
2. `append -> server_log::open(path, CAP): File`: open 0600 append, flock exclusive; when the
   file at `path` is no longer this one, open again; when past CAP, `rename(path, path.1)` and
   open again.
3. `append -> file.write_all(line)`; the lock goes with the File.

- Any io error: returned; callers let it go as today.

The server's own log follows a rotation

1. `Server::serve` tick `-> server_log::follow(&self.log, CAP)`
2. `follow`: fstat(stderr); not a regular file -> Ok. Same file as `log` and under cap -> Ok.
3. else `server_log::open(log, cap) -> rustix::stdio::dup2_stdout(&file), dup2_stderr(&file)`

- Err: the server keeps its old stderr and tries again next tick.

saaio hook reads an event

1. `hook::run -> read_event(io::stdin().lock()): Result<String>` (`take(MAX_EVENT + 1)`, then the
   length decides)
2. `serde_json::from_str(&text)` (its recursion limit refuses deep nesting) `-> hook(...)`
3. adapters translate; every text through `adapter::cut`; a merge request only from
   `calls::merge_request`.

- Over 16 MiB, not UTF-8, unreadable, not JSON: logged to server.log ("the event is over
  16 MiB" or the error), exit 0, nothing on stdout or stderr.

## Test seams

- outbox.rs: socketpair whose far end never reads: `send` never blocks and errs once 16 MiB are
  queued; a dropped Outbox still delivers what it queued.
- server/tests.rs: a UI that never reads while its pane floods output: a control connection's
  CloseFlow is still answered at once and the UI is dropped (fails today).
- server/tests.rs: 4 GiB prefix (Refused, closed, server still takes a UI); silent client
  closed; trickled hello (one byte per 100 ms, hello 300 ms) closed (fails today); Input before
  Hello closed, nothing typed into the master; seeded fuzz of random frames before and after a
  hello, no panic, the server still serves; Resize past MAX_SIZE gives frames of at most 500
  rows (fails today); a hello with a 1 MiB folder gives a cut server.log line (fails today).
- proto.rs: read_msg of a big length with few bytes never hands the reader a buffer over
  64 KiB (fails today); 2000 seeded random byte strings decode or err, never panic;
  `Size::clamped`.
- server_log.rs: `open` rotates past cap, keeps under it; two threads racing past cap rotate
  once (server.log.1 keeps the big log).
- tests/cli/server.rs: E2E, a running server, server.log filled past 1 MiB, a hook append
  rotates it; the server's next line lands in the new server.log (fails today).
- tests/cli/hooks.rs: 17 MiB payload, binary, malformed JSON, 10 000-deep nesting: exit 0,
  stdout and stderr empty, server.log names why (the size case fails today).
- calls.rs, claude_code, codex: URL with control characters or text after the number gives no
  merge request (fails today); questions with ESC and 50 options are cleaned and cut (fails
  today).

## Assumptions (approved)

1. Limits: 16 MiB per hook event, 16 MiB queued per connection, panes at most 1000 x 500, peer
   texts 4096 chars, at most 8 questions and 8 options.
2. A UI past the queue limit is closed, never sent a partial stream.
3. The 2 s write timeout stays, on the writer thread.
4. The server's log may run up to one 500 ms tick past its cap.
5. A hello-less connection is closed within half a second after its deadline (the tick).
6. A hook event over the limit is skipped whole, a PreToolUse included.
7. 16 idle Control connections that said hello still fill the slots; unchanged.
8. CHANGELOG under Unreleased, Fixed, four plain lines: hook events over 16 MiB are ignored; a
   UI that stops reading no longer slows the server; server.log stays bounded while the server
   runs; the server copes with very large windows and malformed messages.

## Tasks

### Protocol

- [ ] `proto::encode`, `write_msg` through it, and `read_msg` growing its buffer with the bytes (unit test: a big length never hands the reader a buffer over 64 KiB, failing first); seeded decoder fuzz unit test
- [ ] `Size::clamped` and `MAX_SIZE`, clamped in `Server::message` for Hello and Resize (server test: Resize past MAX_SIZE gives frames of at most 500 rows, failing first)
- [ ] `Peer::printable` cuts to PEER_TEXT_LIMIT (unit test, and server test: a hello with a 1 MiB folder gives a cut server.log line, failing first)

### Connections

- [ ] `Conn.opened` and `Server::close_silent` on the tick, per-read hello timeout removed, `refuse_unreadable` ignores a gone conn (server test: trickled hello closed, failing first; silent client still closed)
- [ ] `server/outbox.rs` `Outbox::start`/`send` with unit tests (never blocks, errs past MAX_QUEUED, delivers what it queued when dropped)
- [ ] `Conn` writes through its Outbox: `send_to`, `drop_conn`, `accept` (server test: a UI that never reads while its pane floods does not delay a control connection's CloseFlow and is dropped, failing first)
- [ ] Protocol robustness tests in server/tests.rs: 4 GiB prefix, Input before Hello types nothing, seeded fuzz before and after a hello; fix whatever they find

### Log

- [ ] `server_log::open` (flock, rotation, moved-file reopen) replacing `rotate`; `append` and `ensure_server` through it (unit tests: rotates past cap, racing writers rotate once)
- [ ] `server_log::follow` with rustix `stdio`, `Server.log`, called on the tick (tests/cli/server.rs E2E: after a hook append rotates, the server's next line lands in the new server.log, failing first)

### Hook

- [ ] `hook::read_event` with MAX_EVENT (tests/cli/hooks.rs: 17 MiB payload logged and silent, failing first; binary, malformed JSON, 10 000-deep nesting)
- [ ] `calls::merge_request` for `created_merge_request` and `in_repo` (unit tests: control characters or text after the number, failing first)
- [ ] `adapter::cut` through `text::printable`; questions and options cut and capped at MAX_QUESTIONS in claude_code and codex; audit the other payload texts (unit tests, failing first)

### Finish

- [ ] CHANGELOG four lines under Unreleased, Fixed; CLAUDE.md Architecture line for outbox.rs and the server_log changes
- [ ] End-to-end check: scratch SAAIO_HOME, real server and UI in a PTY, a UI that stops reading (SIGSTOP) while the master prints, a second UI attaches at once; server.log rotation by hook appends; a huge hook payload; `saaio --stop` at the end
