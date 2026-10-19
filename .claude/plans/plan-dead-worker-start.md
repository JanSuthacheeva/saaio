# Plan: block an exited working worker

Implementation review: [dead-worker-start-implementation.html](../../.lavish/dead-worker-start-implementation.html)

## Decision

Handle `HostEvent::Exited` immediately after drained output has reached the pane. If a worker pane has exited and its latest flow state is `working`, append an existing `Blocked` event with the exit status and up to four nonempty visible pane lines. This applies to every agent. A failed resume first gets the existing fresh start retry. An exit during a later active turn also blocks; the initial `Working` hook may be omitted from the log, so the event history cannot reliably identify the first turn. No event or JSON schema changes.

## Units

### `src/server.rs` (modified)

```rust
impl Server {
    fn handle(&mut self, event: Event) -> ControlFlow<Stop>;
}
```

In `HostEvent::Exited`, call `Host::exited`, perform the existing resume fallback, and call `Server::notice_exited_worker` only when no fallback was attempted. Retain `tell_closed`, pane update, and focus redraw.

### `src/server/startup.rs` (modified)

```rust
impl Server {
    pub(super) fn notice_exited_worker(&mut self, key: &PaneKey, code: Option<u32>);
}
fn exited_while_working(flow: &FlowView, status: Option<&PaneStatus>) -> bool;
```

Only a `PaneKey::Flow` with `PaneStatus::Exited`, a non-plain current flow and `State::Working` qualifies. Read current events and fold them at the time of exit, rather than using a possibly stale board. Build a reason beginning `agent exited (code 23)` or `agent exited (exit status unavailable)`; append up to four nonempty pane lines with a 400 character output budget. `Event::new` applies the existing 500 character blocker limit. Log store read or append errors to `server.log`. Leave the 30 second running Codex grace check intact.

### `src/server/host.rs` (modified)

```rust
impl Host {
    pub fn output_tail(&self, key: &PaneKey, max_lines: usize, max_chars: usize) -> Option<String>;
}
```

Read the exited pane's terminal rendering; return `None` for a missing pane.

### `src/server/terminal.rs` (modified)

```rust
impl Terminal {
    pub fn output_tail(&self, max_lines: usize, max_chars: usize) -> String;
}
```

Inspect visible rows only, from bottom to top. Join cell text, trim blank rows, preserve the final `max_lines` meaningful rows in display order, and cap the result at `max_chars` Unicode characters. Do not scan or persist scrollback.

### `README.md` (modified)

Document that an agent process exiting while a flow is working makes the flow blocked with its exit status and a short pane output tail.

## Data and call chains

Existing `PaneStatus::Exited { code: Option<u32> }`, `EventKind::Blocked { reason: String }`, and `FlowView.blocker` carry all data. No new DTO or migration.

1. PTY reader -> `HostEvent::Output` -> `Server::handle`: terminal receives output.
2. Waiter, after output drains -> `HostEvent::Exited(key, code)` -> `Server::handle`: exit is processed.
3. `Server::handle` -> `Host::exited(&key, code)`: status becomes exited; a deliberately closing pane is removed.
4. If resume fallback applies, `Server::handle` -> `Server::open_fresh(&key)` and stop the blocker path.
5. Otherwise `Server::handle` -> `Server::notice_exited_worker(&key, code)`: read and fold the current log. Skip master, removed pane, missing flow, plain flow, and non-working flow.
6. `Server::notice_exited_worker` -> `Host::output_tail(&key, 4, 400)` -> `Terminal::output_tail(4, 400)`: bounded visible output. An empty pane contributes no extra line.
7. `Server::notice_exited_worker` -> `Store::append(id, Blocked { reason })`: normal refresh/fold delivers blocker to status, the master and sidebar. Store errors are logged; the pane still shows exited.

## Tests

- Terminal unit tests prove ANSI rendering, blank row removal, line and Unicode character bounds.
- Server socket tests prove immediate exit blocks once with code and output, resume fallback does not prematurely block, and an idle or deliberately closing flow is preserved.
- CLI end-to-end test uses a scratch `SAAIO_HOME` and a configured `sh` agent that exits 23; `saaio status` and `--json` show blocked and its short output tail.

## Assumptions

- Any worker exit while its latest flow is `working` is a blocker, including after a previous turn. This was approved explicitly.
- An `idle`, `waiting`, `blocked`, or finished flow retains its state.
- Deliberate pane closure does not block.
- A failed resume keeps the current fresh retry behavior.
- Only visible terminal rows are inspected; large scrollback remains untouched.

## Tasks

- [x] Add `Terminal::output_tail` and `Host::output_tail` with focused terminal tests for bounded rendered output.
- [ ] Add `Server::notice_exited_worker` and `exited_while_working`, wire the exit event after resume fallback, and add server socket tests for state guards and reason text.
- [ ] Add the scratch-home CLI end-to-end regression and update `README.md` for exited working workers.
- [ ] Run the end-to-end check with a configured agent that exits immediately and verify status and JSON show blocked with code and pane output.
