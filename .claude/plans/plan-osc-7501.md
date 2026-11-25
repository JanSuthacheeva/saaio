# OSC 7501: read program status reports from every pane

Design page: https://claude.ai/artifact/GdHovoZk8Hero5v9uWYtpv (approaches: https://claude.ai/artifact/6AikhN7hknWzjDBqFPnvX1).
Research: https://claude.ai/artifact/BzrBTzq298WWg1Ny92hjYQ.

## Decision

- `Terminal::feed` runs a pure scanner (`src/program_status.rs`) over each pane's output, answers
  the OSC 7501 query in order with the emulator's own replies (before the DA1 answer that follows
  it), and passes the reports up through `Host::feed`. The server keeps the last report per pane in
  `PaneState` and acts on each new one as it arrives (`src/server/program_status.rs`).
- An interrupt is logged as `Idle { note: "interrupted" }`, the event Codex's Interrupt hook
  writes. No new event kind, no log format change.
- A report ends a turn only as `idle` right after `working` or `blocked`, in a harness without an
  Interrupt hook (Claude Code), while the busy file says `Busy::Turn`. `done` is ignored (Stop
  records it). A report never begins a turn. The master's pane gets the same check through
  `master.turn`.
- Not in this MR: acting on `error` (the Codex error-state flow does it for both harnesses);
  showing background work in the sidebar. Background `working` never touches typing or news.
- Later flows plug into `Server::program_status`: answered-in-pane, the dismiss.rs check (Esc only
  while the pane reports `blocked`), Codex's stalled start (`blocked` with its `Need`).

## Units

### `src/program_status.rs` (new, pure, top level; in `lib.rs` as `mod program_status;`)

```rust
/// What a terminal that supports the protocol answers to `ESC ] 7501 ; ? ESC \`.
pub const QUERY_ANSWER: &[u8] =
    b"\x1b]7501;?:states=idle,working,done,blocked,error:kinds=permission,question,auth\x1b\\";
/// The longest sequence the spec allows; a longer one is dropped whole.
const MAX_SEQUENCE: usize = 4096;

#[derive(Debug, Default)]
pub struct Scanner { state: Scan, body: Vec<u8> }

impl Scanner {
    /// The sequences that end in `bytes`, with the offset just past each one's terminator;
    /// a sequence cut off at the end of `bytes` is kept for the next call.
    pub fn scan(&mut self, bytes: &[u8]) -> Vec<Found>;
}

/// One OSC 7501 body (between `7501;` and the terminator): None when it breaks the spec
/// (no or unknown state, bad base64, a control character, an `id` of a child record).
pub fn parse(body: &[u8]) -> Option<Sequence>;

impl Report {
    /// Idle, done and error: the program waits for the person.
    pub fn at_rest(&self) -> bool;
}
```

Scanning rules: introducer `ESC ]`, body must start with `7501;`; terminators ST (`ESC \`) and BEL;
an ESC not followed by `\` aborts the OSC and starts a new escape; CAN (0x18) and SUB (0x1A) abort;
past `MAX_SEQUENCE`, or once its body cannot start with `7501;`, the sequence is skipped
(`Scan::Skip` until its terminator; built as `Skip` rather than `TooLong`, so other OSCs such as
OSC 52 are never buffered). Body: pairs
`key=value` split on `:`, whitespace trimmed, last repeated key wins, unknown keys ignored, a
malformed pair skipped. `?` alone is `Sequence::Query`. `state=clear` is `Sequence::Status(None)`.
`msg` is base64 (standard alphabet, padding optional), decoded to UTF-8; a decode failure or a
control character drops the report. Unknown `kind` is None. A report or clear with an `id` is
ignored (None).

### `src/server/terminal.rs` (changed)

```rust
pub struct Terminal { term, parser, listener, scanner: Scanner, statuses: Vec<Option<Report>> }

/// Feeds the emulator up to each OSC 7501 sequence's end, answering a query right there,
/// so its answer leaves before the DA1 answer that follows it; keeps each report.
pub fn feed(&mut self, bytes: &[u8]);

/// The program's status reports since the last call, drained; None for a `clear`.
pub fn statuses(&mut self) -> Vec<Option<Report>>;
```

The emulator still gets every byte.

### `src/server/host.rs` (changed)

```rust
/// What a pane's output carried besides its screen.
#[derive(Debug, Default)]
pub struct Fed {
    /// The text the program copied (OSC 52).
    pub copied: Option<String>,
    /// Its status reports (OSC 7501), in order; None for a `clear`.
    pub statuses: Vec<Option<Report>>,
}

pub fn feed(&mut self, key: &PaneKey, bytes: &[u8]) -> Fed; // was Option<String>
```

### `src/server/program_status.rs` (new)

```rust
impl Server {
    /// Keeps the pane's last report and acts on the change from the one before.
    pub(super) fn program_status(&mut self, key: &PaneKey, status: Option<Report>);

    /// Records the turn an Esc ended: Idle { note: "interrupted" } in a flow's log (when
    /// fold::changes says it shows), master.turn idle for the master, the session idle.
    fn interrupt(&mut self, key: &PaneKey);
}

/// True when `after` ends a turn the hooks say still runs: idle right after working or
/// blocked, in a harness that runs no hook on an interrupt.
fn interrupted(
    before: Option<&Report>,
    after: Option<&Report>,
    adapter: Option<Adapter>,
    busy: Busy,
) -> bool;
```

### `src/server/pane_state.rs`, `panes.rs`, `server.rs` (changed)

```rust
// PaneState (and UNTOUCHED):
/// The program's last status report (OSC 7501); None until it sends one, after a `clear`
/// and in a pane reopened since.
pub(super) status: Option<Report>,

// panes.rs open: pane.status = None; beside opened_at.

// server.rs HostEvent::Output:
let fed = self.host.feed(&key, &bytes);
for status in fed.statuses { self.program_status(&key, status); }
// clipboard from fed.copied as before. Server::stop's drain loop ignores the Fed.
```

### `src/adapter.rs` (changed)

```rust
/// True when the harness runs a hook on an interrupt (Codex's Interrupt); false when only
/// its status report says so (Claude Code).
pub fn hooks_interrupt(self) -> bool;
```

### `src/fuzzing.rs`, `src/strategies.rs`, `.fuzz/` (changed)

```rust
// fuzzing.rs
/// Any bytes as pane output: a scan in two reads, split anywhere, finds the same sequences
/// at the same places as one scan, every offset inside the input and in order.
pub fn program_status(bytes: &[u8]);

// strategies.rs
pub fn report() -> impl Strategy<Value = Report>;

// .fuzz/fuzz_targets/program_status.rs: one line; its [[bin]] in .fuzz/Cargo.toml
```

## Data

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status { Idle, Working, Blocked, Done, Error }

/// What a blocked program needs from the person (`kind`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Need { Permission, Question, Auth }

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub status: Status,
    /// Only with Blocked; an unknown kind is None.
    pub need: Option<Need>,
    /// `msg`, decoded.
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Sequence {
    Query,
    /// A report, or None for `clear`.
    Status(Option<Report>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found { pub end: usize, pub sequence: Sequence }

#[derive(Debug, Default)]
enum Scan { #[default] Ground, Escape, Osc, OscEscape, Skip }
```

These cross terminal -> host -> server only; never the wire or the log. Stored: nothing new.

## Call chains

### Claude Code asks at startup

1. Server -> Host::feed(&key, &bytes): Fed
2. Host -> Terminal::feed(bytes)
3. Terminal -> Scanner::scan(bytes): Vec<Found>
4. Terminal -> Processor::advance(term, bytes[from..found.end]); on Query -> listener.replies += QUERY_ANSWER
5. Terminal -> Processor::advance(term, bytes[last_end..]): alacritty's DA1 answer after ours
6. Host -> Terminal::replies(): Vec<u8> -> PaneInput::send
7. Host -> Terminal::copied(), Terminal::statuses() -> Fed

Errors: a query split across reads is held by the scanner and answered with the second read,
still before its DA1 answer. An over-long or broken sequence finds nothing; the emulator still gets
every byte.

### Esc mid-turn in a Claude Code worker

1. Server -> Server::program_status(&key, Some(report)): ()
2. program_status -> std::mem::replace(&mut self.pane_mut(key).status, status): before
3. program_status -> Store::busy(&Waiter::Flow(id)): Busy
4. program_status -> interrupted(before.as_ref(), after.as_ref(), pane.adapter, busy): bool
5. true -> Server::interrupt(&key)
6. interrupt -> Store::events(id): Result<Vec<Event>>
7. interrupt -> fold::changes(id, &events, &EventKind::Idle { note: "interrupted" }, now): bool
8. true -> Store::append(id, &Event::new(now, kind)): Result<()>
9. interrupt -> Store::set_busy(&waiter, Busy::Idle): Result<()>
10. interrupt -> server_log::write(now, "<name> was interrupted: idle")
11. Next tick: refresh folds the flow idle; type_lines types a queued line.

Errors: a log that cannot be read or appended to, or a busy file that cannot be written, goes to
server.log and the other step still runs. `done`, no `working`/`blocked` before, `Busy::Idle` or
`Busy::Typed`, a plain pane (adapter None) and Codex: `interrupted` is false, nothing happens. An
ended flow: `fold::changes` is false, nothing appended.

### Esc mid-turn in the master

1. Server -> Server::program_status(&PaneKey::Master, Some(idle))
2. program_status -> Store::busy(&Waiter::Master): Turn; interrupted(...) = true
3. interrupt -> Store::set_master_turn(&MasterTurn { state: State::Idle, at: now })
4. interrupt -> Store::set_busy(&Waiter::Master, Busy::Idle)
5. interrupt -> Server::send_master()

Errors as for a worker.

## Test seams

- `program_status.rs`: unit tests per state and kind, BEL and ST, base64 with and without padding,
  control characters, unknown keys, a child `id`, an over-long sequence, ESC/CAN aborts;
  `mod properties`: random reports (`strategies::report`) encoded between random bytes are all
  found at every split, through `fuzzing::program_status`.
- `terminal.rs`: query then DA1 in one feed and split in two answer in that order; a report drains
  through `statuses`; the screen is unchanged by the sequence.
- `server/program_status.rs`: `interrupted` as a truth table (before, after, adapter, busy).
- `server/tests.rs`: a Claude Code-adapter flow pane whose script prints working then idle with
  the busy file at Turn: the log gains `idle: interrupted`, busy is Idle, a queued
  `saaio send --type` line is typed. With busy Idle (after Stop) nothing is appended.
- End to end: a real Claude Code worker in a scratch store, driven through the UI in tmux.

## Assumptions

1. The query is answered in every pane, plain sessions included.
2. Reports with an `id` (child records) are ignored.
3. A `clear` forgets the pane's last report and acts on nothing.
4. The 8-bit OSC introducer (0x9D) is not scanned.
5. No bytes are removed from what the emulator gets.
6. An interrupted first turn (no Answer yet) leaves queued lines waiting (`takes_lines` wants an
   answer), as for Codex today.
7. Docs: the ROADMAP row "An interrupted Claude Code turn shown as idle" goes; the troubleshooting
   row becomes "Claude Code older than 2.1.295 sends no status reports: update it"; HANDOVER
   (lines on Esc), docs/repository.md (`waits turn` stays until the next prompt), CLAUDE.md's
   architecture and the CHANGELOG (Fixed) follow.
8. `Report::message` is kept but unused until the answered-in-pane and stalled-start flows.

## Tasks

- [x] Add `program_status.rs` with `Report`, `Status`, `Need`, `Sequence`, `parse` and `Report::at_rest`, with unit tests per state, kind, base64, control characters, unknown keys and child ids
- [x] Add `Scanner::scan` with `Found` and `QUERY_ANSWER`, with unit tests for ST, BEL, splits, aborts and over-long sequences
- [x] Add the `fuzzing::program_status` oracle, `strategies::report`, the `mod properties` test and the `.fuzz` target
- [x] Scan in `Terminal::feed`, answer the query in order and add `Terminal::statuses`, with tests for query-then-DA1 order whole and split
- [x] Return `Fed` from `Host::feed` and pass statuses to the server
- [x] Add `Adapter::hooks_interrupt`, `PaneState::status` (reset in open), and `server/program_status.rs` with `program_status`, `interrupt` and the `interrupted` truth-table tests
- [ ] Add server tests over a real socket: a working-then-idle pane under Busy::Turn logs `idle: interrupted` and types a queued line; under Busy::Idle nothing is appended; the master's turn goes idle
- [ ] Update ROADMAP, troubleshooting, HANDOVER, repository.md, CLAUDE.md architecture and CHANGELOG
- [ ] End to end with a real Claude Code worker in a scratch store, through the UI in tmux: Esc mid-turn shows idle, a queued `saaio send --type` line is typed, a `waits turn` clears; then `saaio --stop`
