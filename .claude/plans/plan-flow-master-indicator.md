# Plan: the master's row shows when it needs you

Implementation page: https://claude.ai/artifact/D81sgyQuXjHyS2uwY8i3Qv (approaches: https://claude.ai/artifact/UdDsxtkVhL7VX7tE723H1N, approach A).

## Decision

The master's hook keeps one small state in `waits/master.turn`; the server sends it with
`ServerMsg::Master`; the UI decides whether the user has seen it. Row 0 draws the glyph a worker
row draws, in the same column (` 0 ◓ master` like ` 1 ● shop-221`), with `Sidebar::glyph` and
`Theme::state_style`; the rail draws ` 0 ●` like ` 1 ●`.

- Working or Delegated: the spinner, also while the master is focused.
- An ended turn, a question or a permission prompt while the master is not focused: the waiting
  dot (an unseen Blocked: the blocked dot) until its pane gets focus.
- Seen, or no turn yet: blank, as today.

Decided with the user: the Claude Code master always gets the worker's report hooks, also with
`[master] wake = false`; a permission prompt or a question shows the dot like an ended turn.

## Units

`src/view.rs` (modified): not part of `Board`; `VIEW_VERSION` and `--json` stay.

```rust
/// Where the master's turn stands, as its hooks last said, and since when.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MasterTurn {
    pub state: State,
    pub at: u64,
}
```

`src/fold.rs` (modified):

```rust
/// The state the last of `kinds` that says one puts the master in: Working, Delegated,
/// Waiting, Idle, Blocked or Ended; None when none says one (Answer, Session, Phase, ...).
pub fn master_state(kinds: &[EventKind]) -> Option<State>;
```

`src/store.rs` (modified): beside `master.session`, JSON.

```rust
const MASTER_TURN_FILE: &str = "master.turn";

/// The master's turn; None before its first hook or when the file does not parse.
pub fn master_turn(&self) -> Option<MasterTurn>;
pub fn set_master_turn(&self, turn: &MasterTurn) -> Result<()>;
/// Forgets it, as a master process starts without a turn.
pub fn forget_master_turn(&self) -> Result<()>;
```

`src/adapter/claude_code.rs` (modified):

```rust
/// The master's --settings JSON: report_hooks, and when it `wake`s, `<flow_exe> wait --master`
/// on each event in WAIT_EVENTS.
pub fn master_settings(flow_exe: &Path, wake: bool) -> String;

/// Adds `waiter` to each event in WAIT_EVENTS; settings and master_settings share it.
fn arm(hooks: &mut serde_json::Map<String, Value>, waiter: &Value);
```

`src/adapter.rs` (modified): `wire_master` keeps its signature; ClaudeCode always adds
`--settings master_settings(flow_exe, wake)`. Doc comments there and on `MasterRecord::spec`
(`server/master.rs`) follow.

`src/cli/commands/hook.rs` (modified): the `Hooked::Master` arm of `hook()` calls, beside
`record_master_session`:

```rust
/// Keeps the state the master's kinds put it in, with `now`: always when it comes to rest (a
/// turn woken by `flow wait` runs no prompt hook, so Idle can follow Idle), else when it differs
/// from the one kept: tool calls repeat Working, and an unchanged file keeps the server from
/// resending.
fn record_master_turn(store: &Store, kinds: &[EventKind], now: u64);
```

`src/server/panes.rs` (modified): `open_pane` also calls `store.forget_master_turn()` when it
opens `PaneKey::Master`; an error goes to server.log and the pane still opens.

`src/proto.rs` (modified): `PROTOCOL_VERSION` 5, so a running older server refuses the new UI.

```rust
/// Where the master runs, and where its turn stands.
Master {
    cwd: String,
    branch: Option<String>,
    turn: Option<MasterTurn>,
},
```

`src/server/conn.rs` (modified): `send_master` fills `turn` from `store.master_turn()`.

`src/ui/app.rs`, `app/messages.rs`, `app/keys.rs` (modified):

```rust
// App fields
master_turn: Option<MasterTurn>,
/// A turn of the master ended, asked or wants permission while its pane was not in view.
master_unseen: bool,
/// When each pane's state last changed while this UI watched: its row stands out a moment.
changed: HashMap<PaneKey, u64>, // was HashMap<String, u64> by flow id

/// Takes the master's turn: lights row 0 when its state changed, and marks it unseen when it
/// came to rest (Waiting, Idle, Blocked) while the master is not focused.
fn on_master_turn(&mut self, turn: Option<MasterTurn>);

/// What row 0's glyph shows: Working or Delegated as they are; Waiting for an unseen Idle or
/// Waiting, Blocked for an unseen Blocked; None once seen.
fn master_state(&self) -> Option<State>;
```

- `focus(PaneKey::Master)` clears `master_unseen`.
- `on_tick` spins also while `master_state()` is Working or Delegated.

`src/ui/sidebar.rs`, `sidebar/rows.rs`, `rail.rs` (modified):

```rust
// Sidebar fields
/// Row 0's glyph: App::master_state.
pub master_state: Option<State>,
pub changed: &'a HashMap<PaneKey, u64>,
```

- `master_rows` writes `" 0 "`, then `glyph(state)` in `theme.state_style(state)` (or a space),
  then `" master"`. Mark: Focus when focused, else Lit while `changed` holds `PaneKey::Master`.
- Worker rows look up `changed` by `PaneKey::Flow(id)`.
- `Rail::rows` draws `" 0 "` and the same glyph.

## Data

- `MasterTurn { state: State, at: u64 }`: hook to store (JSON), server to UI (postcard).
- `waits/master.turn`: `{"state":"waiting","at":1792157011000}`.
- `ServerMsg::Master` + `turn: Option<MasterTurn>`. `PROTOCOL_VERSION` 4 -> 5.
- `App.changed`, `Sidebar.changed`: keyed by `PaneKey`.

## Call chains

The master calls a tool, gets a prompt, stops, asks permission:

1. `hook::run -> hook(store, config, adapter, Hooked::Master, payload, now)`
2. `hook -> Adapter::translate(payload, phases): Vec<EventKind>`
3. `hook -> record_master_session(store, &kinds)` (unchanged)
4. `hook -> record_master_turn(store, &kinds, now)`
5. `record_master_turn -> fold::master_state(&kinds): Option<State>`; None returns
6. `record_master_turn -> Store::master_turn(): Option<MasterTurn>`; the same Working or Delegated returns (a resting state is always written: a woken turn starts with no prompt hook)
7. `record_master_turn -> Store::set_master_turn(&MasterTurn { state, at: now }): Result<()>`
8. Error: a failed write goes to stderr; the hook stays silent and exits 0.

The server tells the UI:

1. `Server::refresh -> Server::send_master()`, every 500 ms
2. `send_master -> Store::master_turn(): Option<MasterTurn>`
3. `send_master -> send_to(conn, &ServerMsg::Master { cwd, branch, turn })`, only when it differs
   from `sent_master`
4. `App::on_server(ServerMsg::Master { .. }) -> App::on_master_turn(turn)`
5. State differs from the last turn's (and there was one): `changed.insert(PaneKey::Master, now)`
6. New state Waiting, Idle or Blocked and `focus != PaneKey::Master`: `master_unseen = true`
7. Error: a missing or unreadable file is None; row 0 stays blank.

Drawing:

1. `App::sidebar -> Sidebar { master_state: self.master_state(), changed: &self.changed, .. }`
2. `Sidebar::master_rows -> Sidebar::glyph(state)`, `Theme::state_style(state)`
3. `Rail::rows` the same from `self.sidebar.master_state`
4. `App::on_tick`: spins while `master_state()` is Working or Delegated; drops `changed` entries
   after `CHANGE_SHOWN_MS`

Looking at the master:

1. C-space 0, a click on row 0, or the fallback after a removed worker -> `App::focus(PaneKey::Master)`
2. `focus` sets `master_unseen = false`, sends `ClientMsg::Focus` as before
3. A turn that ends while the master is focused never sets `master_unseen`

The master's pane opens again:

1. `Server::open_pane(spec)` with `PaneKey::Master -> Store::forget_master_turn()`
2. Error: logged to server.log, the pane still opens.

## Test seams

- `fold::master_state`: last-kind-wins, None for Answer or Session alone.
- `Store`: round trip, forget twice is no error, an unparsable file is None.
- `claude_code::master_settings`: report hooks with and without wake, waiter only with wake;
  `wire_master` test updated.
- `flow hook`: `tests/cli.rs` with `FLOW_MASTER=1` and recorded payloads: a prompt writes
  working, a Stop writes idle, a second tool call leaves the file untouched.
- Server: `server/tests.rs`, the Master message carries the turn written to the store.
- `App`: `app/tests/sidebar.rs`, unseen when unfocused, not when focused, cleared on focus,
  spinner ticks; a golden for row 0 working and waiting.

## Assumptions

- The spinner shows while the master works, also when focused, as on worker rows.
- Row 0 lights up for 3 s when the master's state changes, like a worker row.
- An unseen Blocked shows the blocked dot, not the waiting one.
- "Seen" lives in the running UI; a new `flow ui` opens on the master, so it starts out seen.
- The master does not count in the "needs you" heading or count.
- An Esc-interrupted Claude master keeps its spinner until its next prompt (no hook runs on an
  interrupt, as for workers today); Codex reports it as Idle.
- The turn reaches the UI within the 500 ms refresh.

## Tasks

- [x] Add `view::MasterTurn` and `fold::master_state(kinds)` with unit tests for last-kind-wins and kinds that say no state
- [x] Add `Store::master_turn`, `set_master_turn`, `forget_master_turn` (`waits/master.turn`) with round-trip, forget-twice and unparsable-file tests
- [x] Give the Claude Code master the report hooks: `claude_code::master_settings(flow_exe, wake)` with `arm`, `Adapter::wire_master` always wiring it, doc comments in `adapter.rs` and `server/master.rs`; update the settings and wire_master tests
- [x] Add `record_master_turn` to `flow hook`'s master arm, with `tests/cli.rs` cases: a prompt writes working, a Stop writes idle, a repeated tool call leaves the file untouched
- [x] Send the turn: `ServerMsg::Master { turn }` from `send_master`, `open_pane` forgets the master's turn; server test over the socket
- [x] Key `App.changed` and `Sidebar.changed` by `PaneKey`; existing tests pass unchanged in behaviour
- [x] Add `App::on_master_turn`, `master_state`, `master_unseen`, the focus clear and the master spin in `on_tick`, with `app/tests/sidebar.rs` cases for unseen, focused, cleared and spinning
- [x] Draw row 0 and the rail's 0 with the glyph and the Lit mark; golden for the master working and waiting; `FLOW_UPDATE_GOLDEN=1` only for the meant change
- [x] Update CLAUDE.md's architecture lines (store, hook, proto) and docs/HANDOVER.md where they describe the master's hooks
- [x] End to end: scratch `FLOW_HOME`, `flow ui` with a Claude Code master: the spinner while it works, the dot after its turn while a worker is focused, cleared on C-space 0; also with `[master] wake = false`

## Review

Reviewed 2026-10-17 against this plan, in a context that did not write the code.

**Fixed**
- A master turn woken by `flow wait` (asyncRewake, no UserPromptSubmit) that ended without a tool
  call went Idle -> Idle; `record_master_turn` kept the old file, so row 0 showed no dot for the
  news the master just relayed. Shown by `tests/cli.rs`
  `a_woken_master_turn_that_ends_idle_again_is_a_new_turn` failing. Now only a repeated Working
  or Delegated is skipped; every resting state is written with its `at` (f8f5539).

**Open**
- "Not focused" is `focus != PaneKey::Master`, as planned: while the board or a history view
  covers the screen with the master still focused, a turn that ends there shows no dot.
  Recommendation: leave as planned unless it bites; the fix would be to also count
  `View::Board`/`View::History` as not in view.
- The fix was reviewed in a separate pass by the same reviewer, not by an independent agent.

**Where to look**
1. `src/cli/commands/hook.rs` `record_master_turn`: which hook kinds rewrite `waits/master.turn`.
2. `src/ui/app/messages.rs` `on_master_turn` with `src/ui/app.rs` `master_state` and
   `src/ui/app/keys.rs` `focus`: the seen/unseen logic.
3. `src/adapter/claude_code.rs` `master_settings`/`arm` and `adapter.rs` `wire_master`: the Claude
   master now always runs the report hooks (`turn` is None for Claude Code, so no Stop handover).
- Routine, skim: `changed` keyed by `PaneKey`, the `turn: None` test fixture lines, goldens,
  `Store::remove_wait` extraction, `PROTOCOL_VERSION` 5.

**Checks**: `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check` green
at f8f5539.
