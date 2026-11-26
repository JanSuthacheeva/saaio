# Codex status reports: failed turns and dialogs before the first turn

Design page: https://claude.ai/artifact/Jdc2VdWYVKC7ZnaUgYAWVg (approaches: https://claude.ai/artifact/GKvZ7icTaJKr8DVHaaoVcc).
Research: https://claude.ai/artifact/BzrBTzq298WWg1Ny92hjYQ. Follows `plan-osc-7501.md` (PR #130).

## Decision

- Approach A: `Server::program_status` gets two more checks beside `interrupted`, each a pure
  function plus a method that records it.
- **Failed turn** (every harness with an adapter, Claude Code included): an `error` report right
  after `working` or `blocked` appends `Blocked { reason: "turn failed: <message>" }` to the flow
  (through `fold::changes`) and sets the session's busy to idle; for the master, master.turn idle.
  The message comes from Codex's rollout (its newest turn edge is a `task_complete` with
  `error.message`); otherwise, and always for Claude Code, `turn failed: see its pane`.
- **Dialog before the first turn** (Codex only): a `blocked` report, not blocked before, while the
  flow is working with no session, appends `Waiting { dialog_question(need) }` at once.
- A login sends no report on any Codex version (its start screen is outside the chat), so the
  stalled-start guess (`startup.rs`, 30 s, `STALLED_START`) stays unchanged. With no reports
  nothing new runs.
- The rollout reader moves from `cli/commands/hook.rs` to `src/tail.rs`; the adapter stays pure,
  given the reader as a closure (like `Adapter::approver`).
- Build against Codex 0.172.0 (installed). No `-c features.tui_workspace=false`: the sidebar did
  not show in any recorded run; add it in this flow only if the end-to-end run shows it.

### What Codex 0.172.0 sent under saaio (recorded, the fixtures below)

| Case | Reports | Hooks |
|---|---|---|
| Login screen (empty `CODEX_HOME`) | none | none |
| Turn fails (HTTP 400, unknown model) | `idle` `working` `error` | SessionStart, UserPromptSubmit; no Stop, no Interrupt |
| Normal turn ("ok") | `idle` `working` `done` | Stop: Answer, Idle |
| Esc mid-turn | `idle` `working` `idle` | Interrupt: Idle "interrupted" |

Codex never sends `msg`. Rate limits end the turn through the same `finalize_turn` (source,
`turn_runtime.rs` at `rust-v0.172.0`), so they report `error` too.

## Units

### `src/tail.rs` (new, moved from `cli/commands/hook.rs`; `mod tail;` in `lib.rs`)

```rust
/// How much of a file is read at a time, from its end back.
pub const CHUNK: u64 = 64 * 1024;

/// The last line of `file` that `matches`, read back from the end `chunk` bytes at a time.
pub fn last_line(file: &Path, matches: &dyn Fn(&str) -> bool, chunk: u64)
    -> std::io::Result<Option<String>>;
```

Body and tests move unchanged. No crate imports.

### `src/cli/commands/hook.rs` (changed)

`rollout_line` calls `tail::last_line(file, matches, tail::CHUNK)`; its own `last_line`,
`ROLLOUT_CHUNK` and their tests go.

### `src/adapter.rs` (changed)

```rust
/// What the turn that just failed failed on, read from the session's transcript; None when the
/// harness writes no reason there or the turn's end is not written yet.
pub fn turn_error(
    self,
    transcript: &Path,
    last_line: impl FnOnce(&Path, &LineMatch) -> Option<String>,
) -> Option<String>;   // ClaudeCode: None; Codex: codex::turn_error
```

### `src/adapter/codex.rs` (changed)

```rust
/// The error message of the rollout's newest turn when it ended on one: its last task_started or
/// task_complete line is a task_complete with error.message.
pub fn turn_error(
    transcript: &Path,
    last_line: impl FnOnce(&Path, &LineMatch) -> Option<String>,
) -> Option<String>;

/// True for a rollout line that starts (task_started) or ends (task_complete) a turn.
fn at_turn_edge(line: &str) -> bool;

/// error.message of a task_complete line; None for any other line or a turn that ended well.
fn failure(line: &str) -> Option<String>;
```

### `src/server/program_status.rs` (changed)

`interrupted` / `interrupt` unchanged. `program_status` no longer returns early on anything but
Idle; it asks all three checks.

```rust
/// Keeps the pane's last report and acts on the change from the one before: an interrupt, a
/// failed turn, a dialog before the first turn.
pub(super) fn program_status(&mut self, key: &PaneKey, status: Option<Report>);

/// Records the turn that failed: Blocked { "turn failed: …" } in a flow's log (when
/// fold::changes says it shows), master.turn idle for the master; the session idle.
fn fail_turn(&mut self, key: &PaneKey);

/// Records the dialog a worker shows before its first turn: Waiting { dialog_question(need) }.
fn wait_on_dialog(&mut self, id: FlowId, need: Option<Need>);

/// The flow behind a pane, folded from its log now (the board may be a tick behind the hooks).
fn flow_now(&self, key: &PaneKey) -> Option<FlowView>;

/// True when `after` ends a running turn on an error: error right after working or blocked, in
/// a session with an adapter (any harness; a plain session never).
fn failed_turn(before: Option<&Report>, after: Option<&Report>, adapter: Option<Adapter>) -> bool;

/// True when `after` is a dialog a Codex worker shows before its first turn: blocked, not blocked
/// before, its flow working with no session yet.
fn blocked_at_start(
    before: Option<&Report>,
    after: Option<&Report>,
    adapter: Option<Adapter>,
    flow: Option<&FlowView>,
) -> bool;
```

### `src/server/startup.rs` (changed)

```rust
/// The question a dialog before the first turn waits on, by what it needs.
pub(super) fn dialog_question(need: Option<Need>) -> &'static str;
// Permission: "a permission dialog holds the first turn: answer it in its pane"
// Question:   "a question holds the first turn: answer it in its pane"
// Auth:       "a sign-in holds the first turn: sign in in its pane"
// None:       "a dialog holds the first turn: answer it in its pane"
```

`STALLED_START` and `stalled` unchanged.

## Data

No new types, fields or event kinds.

- In: `Report { status: Error | Blocked, need, .. }` from the existing scanner; a rollout line
  `{"type":"event_msg","payload":{"type":"task_complete","error":{"message":…}}}`.
- Out: `EventKind::Blocked { reason }`, reason `turn failed: <message>` cut by `adapter::cut`, or
  `turn failed: see its pane`; `EventKind::waiting(dialog_question(need))`; `Busy::Idle`.
- Fixtures (committed with this plan, home and scratch paths replaced):
  - `src/program_status/fixtures/codex-0.172-turn-failed.raw`, `codex-0.172-login.raw`,
    `codex-0.172-turn-done.raw`, `codex-0.172-interrupted.raw`: raw pane output.
  - `src/adapter/fixtures/codex/rollout_turn_failed.jsonl`, `rollout_turn_ok.jsonl`: the
    task_started and task_complete lines of the failed and the normal turn.

## Call chains

### A turn fails

1. `Server::handle -> Host::feed(&key, &bytes): Fed`, then `Server::program_status(&key, status)`
   for each status
2. `program_status -> failed_turn(before, after, adapter): bool` (working or blocked, then error)
3. `program_status -> Server::fail_turn(&key)`
4. `fail_turn -> Server::flow_now(&key): Option<FlowView>` via `Store::events(id)` and `fold::fold`
5. `fail_turn -> Adapter::turn_error(transcript, |file, m| tail::last_line(file, m, tail::CHUNK) …): Option<String>`
6. `codex::turn_error -> last_line(transcript, &at_turn_edge)`, then `failure(&line): Option<String>`
7. `fail_turn -> fold::changes(id, &events, &Blocked { reason }, now): bool`, then
   `Store::append(id, &Event)`
8. `fail_turn -> Store::set_busy(&waiter_of(key), Busy::Idle)`, then
   `server_log::write("<key>'s turn failed: <reason>")`
9. Master pane: `Store::set_master_turn(Idle)` and `send_master()` in place of 7.

Errors: no session or transcript, an unreadable rollout (to server.log), or a newest turn edge
that is a `task_started` -> reason `turn failed: see its pane`. Log unreadable or append fails ->
server.log "could not record <key> as failed: …", no retry. Busy goes idle regardless. The next
prompt's `UserPromptSubmit` appends `Working`, which clears the blocker.

### A dialog before a Codex worker's first turn

1. `Server::program_status(&key, Some(Report { Blocked, need, .. }))`
2. `program_status -> Server::flow_now(&key)` (only for a flow pane whose report is blocked)
3. `program_status -> blocked_at_start(before, after, adapter, flow.as_ref()): bool`
4. `program_status -> Server::wait_on_dialog(id, need)`
5. `wait_on_dialog -> startup::dialog_question(need): &'static str`
6. `wait_on_dialog -> fold::changes(..)`, then `Store::append(id, &Event::new(now, EventKind::waiting(question)))`

Errors: log read or append fails -> server.log; the 30 s stall guess still fires. Later ticks:
`stalled` needs state Working, so it stays quiet once the flow waits.

### No reports

Neither check runs; `stalled` and the hooks behave as today (always for a login screen).

## Test seams

- `tail`: the moved tests.
- `codex::turn_error`: both rollout fixtures, a task_started after an earlier failed
  task_complete, a missing file; the reader faked as a closure over the fixture text.
- Scanner: each recorded `.raw` fed in random splits -> failed: idle, working, error; done: idle,
  working, done; interrupted: idle, working, idle; login: nothing.
- `failed_turn`, `blocked_at_start`: tables of before/after/adapter/flow, like the `interrupted`
  tests; the done and interrupted sequences never fail a turn.
- `dialog_question`: one line per need.
- Server over a real socket (`server/tests.rs`): a fake codex that `cat`s
  `codex-0.172-turn-failed.raw`, with a Session event whose transcript is the failed rollout
  fixture -> Blocked once with the fixture's message, busy idle; a fake that prints
  `blocked kind=auth` before any session -> waits at once with the sign-in question, and the stall
  never fires after it.
- End to end in a scratch store with real Codex 0.172: a worker on an unknown model ends blocked
  with Codex's message (no tokens); watch for the workspace sidebar.

## Assumptions

1. A failed turn becomes blocked (needs you), not idle with a note; the next prompt clears it.
2. Busy goes idle on a failed turn, so the master's news can be typed into a Codex pane again.
3. A rate limit is handled like any other error; Codex resumes only through a new prompt.
4. The master pane gets the failed turn too: master.turn idle, no blocker.
5. Claude Code's failed turn reads `turn failed: see its pane`.
6. The stalled-start text and its 30 s grace stay unchanged.
7. Docs: `docs/troubleshooting.md` gets a "turn failed" row, CHANGELOG an Unreleased Fixed line,
   HANDOVER D1 a note on the dialog check.

## Decisions made during review

- The failed-turn check covers every harness (Claude Code too).
- Two real 0.172 turns were spent (normal, Esc); recorded above and as fixtures.
- `~/.codex/config.toml` sets `gpt-6-sol`, which 0.172 rejects for a ChatGPT account; the config is
  left alone; scratch runs pass `-m gpt-5.6-luna`.

## Tasks

- [x] Move `last_line` from `cli/commands/hook.rs` to `src/tail.rs` (`tail::last_line`, `tail::CHUNK`) with its tests; `rollout_line` uses it
- [x] Add `codex::turn_error`, `at_turn_edge`, `failure` and `Adapter::turn_error` with tests over `rollout_turn_failed.jsonl`, `rollout_turn_ok.jsonl`, an earlier failed turn and a missing file
- [x] Add scanner tests over the four recorded `.raw` fixtures in random splits
- [x] Add `failed_turn`, `Server::flow_now` and `Server::fail_turn` in `server/program_status.rs` (flow Blocked with the rollout's reason, busy idle, master turn idle) with table tests and a server test with a fake codex replaying `codex-0.172-turn-failed.raw`
- [x] Add `startup::dialog_question`, `blocked_at_start` and `Server::wait_on_dialog` with table tests and a server test with a fake codex reporting `blocked kind=auth` before any session
- [x] Update `docs/troubleshooting.md`, CHANGELOG (Unreleased, Fixed), HANDOVER D1 and CLAUDE.md's `server/program_status.rs`, `tail.rs` and adapter entries
- [ ] End to end in a scratch store with real Codex 0.172: a worker with `-m saaio-no-such-model` ends blocked with Codex's message, `saaio status` shows it, busy is idle; check for the workspace sidebar; `saaio --stop`
