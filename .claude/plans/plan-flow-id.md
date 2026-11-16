# FlowId and SessionId

Review page: https://claude.ai/artifact/DS8sv3btjKTyMTjamyrdS9 (approaches: https://claude.ai/artifact/SkkNqqwxe5r3CJY24qP1zh)

Item 5 of the rust-quality-audit report (https://claude.ai/artifact/SmextdM2HEKjvHEwSJv4gu).

## Decision

- Flow ids become `FlowId(Uuid)`, harness session ids `SessionId(Uuid)`; both `Copy`, built only by
  `generate()` or a strict `parse` (canonical lowercase hyphenated text only). `event::is_uuid` is deleted.
- `From<FlowId> for SessionId`: a Claude worker's session is named after its flow.
- Serde through the text form (`#[serde(try_from = "String", into = "String")]`, not `transparent`), so
  the JSON, TOON, the logs, slots/turns/waits files and the postcard wire stay byte for byte the same.
  `PROTOCOL_VERSION` and `VIEW_VERSION` stay.
- `fold::resolve` keeps returning `&FlowView`; it matches the id after parsing the text as a `FlowId`.

## Units

### `src/id.rs` (new, private in lib.rs, depends on nothing in the crate)

The UUIDs saaio names flows and harness sessions by, each its own type.

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct FlowId(Uuid);

impl FlowId {
    pub fn generate() -> FlowId;
    pub fn parse(text: &str) -> Result<FlowId, NotAnId>;
}
impl fmt::Display for FlowId;      // lowercase hyphenated
impl FromStr for FlowId;           // Err = NotAnId; clap's parser for saaio turn --flow
impl TryFrom<String> for FlowId;   // Error = NotAnId
impl From<FlowId> for String;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct SessionId(Uuid);

impl SessionId {
    pub fn generate() -> SessionId;
    pub fn parse(text: &str) -> Result<SessionId, NotAnId>;
}
impl From<FlowId> for SessionId;
impl fmt::Display for SessionId;
impl TryFrom<String> for SessionId;
impl From<SessionId> for String;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0:?} is no id")]
pub struct NotAnId(String);

fn canonical(text: &str) -> Result<Uuid, NotAnId>; // Uuid::try_parse and text == its hyphenated form
```

Invariants: `parse(id.to_string()) == Ok(id)`, `parse(text)?.to_string() == text`. `Ord` on the bytes
equals the old text order (lowercase hex, fixed hyphens).

### `src/store.rs`

```rust
pub struct FlowLog { pub id: FlowId, pub events: Vec<Event>, pub beat: Option<u64> }
pub enum Waiter { Master, Flow(FlowId) }

pub fn create(&self, first: &Event) -> Result<FlowId>;
pub fn ids(&self) -> Result<Vec<FlowId>>;                 // stems that do not parse are skipped
pub fn stamps(&self) -> Result<Vec<(FlowId, Stamp)>>;
pub fn append(&self, id: FlowId, event: &Event) -> Result<()>;
pub fn events(&self, id: FlowId) -> Result<Vec<Event>>;
pub fn beat(&self, id: FlowId) -> Result<()>;
pub fn last_beat(&self, id: FlowId) -> Result<Option<u64>>;
pub fn delivered(&self, id: FlowId) -> usize;
pub fn set_delivered(&self, id: FlowId, count: usize) -> Result<()>;
pub fn typed(&self, id: FlowId) -> usize;
pub fn set_typed(&self, id: FlowId, count: usize) -> Result<()>;
pub fn set_agent(&self, id: FlowId, pid: u32) -> Result<()>;
pub fn agent(&self, id: FlowId) -> Option<u32>;
pub fn remove(&self, id: FlowId) -> Result<()>;
pub fn set_master_session(&self, id: SessionId) -> Result<()>;
pub fn master_session(&self) -> Option<SessionId>;       // a file that does not parse: None
fn log(&self, id: FlowId) -> PathBuf;
fn alive(&self, id: FlowId) -> PathBuf;
```

### `src/event.rs`, `src/view.rs`

```rust
// event.rs: is_uuid removed
EventKind::Session { id: SessionId, transcript: Option<String> }
// view.rs
pub struct FlowView { pub id: FlowId, /* rest unchanged */ }
pub struct Session { pub id: SessionId, pub transcript: Option<String> }
```

### `src/fold.rs`

```rust
pub fn resolve<'a>(flows: &'a [FlowView], name_or_id: &str) -> Option<&'a FlowView>; // FlowId::parse(..).ok() == flow.id, else names
pub fn changes(id: FlowId, events: &[Event], kind: &EventKind, now: u64) -> bool;
fn Folding::new(id: FlowId, started: &EventKind, at: u64) -> Folding;
```

### `src/proto.rs`, `src/client.rs`

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum PaneKey { Master, Flow(FlowId) }
ClientMsg::CloseFlow(FlowId)
ServerMsg::Closed(FlowId)
ServerMsg::NotOpen(FlowId)
pub fn close_flow(socket: &Path, id: FlowId, timeout: Duration) -> Result<Closing>; // client.rs
```

### `src/wake.rs`, `src/deliver.rs`, `src/slots.rs`, `src/turns.rs`

```rust
pub type Seen = BTreeMap<FlowId, Mark>;
pub fn worker_news(store: &Store, id: FlowId, now: u64) -> WorkerNews;
pub struct Claim { pub flow: FlowId, /* rest unchanged */ }
pub struct Holder { pub flow: FlowId, pub name: String, pub command: String }
```

### `src/launch/pane.rs`, `src/launch/worktree.rs`

```rust
pub fn agent_argv(agent: &Agent, prompt: &str, id: SessionId, name: &str, request: &str) -> Vec<String>;
pub fn plain_argv(agent: &Agent, id: SessionId, name: &str) -> Vec<String>;
pub fn resume_argv(agent: &Agent, id: SessionId, name: &str, session: SessionId) -> Option<Vec<String>>;
pub fn hook_env(id: FlowId, name: &str, repo: &Path, workspace: &Workspace) -> Vec<(String, String)>;
```

A worker's spec passes `own = SessionId::from(flow.id)` as `id`, and as `session` its recorded
`Session.id`, else `own`. The `is_uuid` filter in the spec goes.

### `src/adapter/codex.rs`, `src/server/master.rs`

```rust
// codex.rs: payload["session_id"] -> SessionId::parse(..).ok()?
pub struct MasterRecord { pub session: SessionId, pub cwd: PathBuf }  // SessionId::generate when new
pub fn spec(&self, start: Start, agent: &Agent, env: &LaunchEnv,
            instructions: &Instructions, session: Option<SessionId>) -> Option<PaneSpec>;
```

### `src/server/*`

```rust
known: HashSet<FlowId>                                         // server.rs, reconcile.rs
pub(super) fn close_flow(&mut self, conn: ConnId, id: FlowId); // panes.rs; is_uuid check gone
pub(super) fn reopen_worker(&mut self, id: FlowId);
fn flow(&self, id: FlowId) -> Option<&FlowView>;
fn current_flow(&self, id: FlowId) -> Result<FlowView>;
pub fn done(&mut self, id: FlowId);                            // pause_watch.rs, and append_services
// startup.rs, forge_watch.rs: id lists Vec<FlowId>
```

### `src/cli/*`

```rust
// env.rs unchanged: Env::flow_id() -> Option<&str>, the raw SAAIO_FLOW_ID
enum Hooked { Master, Flow(FlowId), Nobody }                   // hook.rs
fn wait_for_services(store: &Store, id: FlowId, works: bool, log: &Log, clock: fn() -> u64);
fn wait_for_turn(store: &Store, id: FlowId, line: &str, log: &Log, clock: fn() -> u64);
fn heavy(store: &Store, id: FlowId, line: &str, log: &Log) -> Option<(FlowView, String)>;
fn agent_of(store: &Store, id: FlowId, log: &Log) -> Option<(u32, Vec<u32>)>;
fn keep_turn(turn: turns::Turn, id: FlowId, agent: u32, harness: &[u32]) -> Result<()>;
fn of_flow_repo(store: &Store, id: FlowId, url: &str, now: u64) -> bool;
pub struct Args { #[arg(long)] flow: FlowId, ... }             // turn.rs
fn exclusive(store: &Store, id: FlowId) -> Result<Option<Exclusive>>;
fn asks(store: &Store, id: FlowId) -> bool;
fn Rollback::new(store: &'a Store, registry: &Registry, id: FlowId) -> Rollback<'a>;  // start.rs
fn append_workspace(store: &Store, id: FlowId, space: &Workspace, now: u64) -> Result<(), Failure>;
// stop.rs: refuse_inside_own_flow parses env.flow_id() and compares FlowIds
// remove, send, report, status, new: unchanged, through fold::resolve
```

### `src/ui/*`

```rust
fn worker(&self, id: FlowId) -> Option<&FlowView>;            // app.rs
fn in_view(&self, id: FlowId) -> bool;
// keys.rs move_cursor: ids Vec<FlowId>
pub(super) fn transcript(claude_home: &Path, session: SessionId) -> Option<PathBuf>; // peek/claude.rs
```

### `src/fixtures.rs`

```rust
pub fn flow_id(name: &str) -> FlowId;       // name hashed into a0000000-0000-4000-8000-<hash>
pub fn session_id(name: &str) -> SessionId; // the same, b0000000-...
pub fn flow(name: &str, events: Vec<Event>, now: u64) -> FlowView; // id flow_id(name)
```

Tests that used `"7f3c"`, `"a"`, `"01a1"` as ids take `fixtures::flow_id("7f3c")` or
`fixtures::session_id("01a1")`, so they keep their readable names.

## Data

| Type | Fields | Crosses |
|---|---|---|
| `FlowId` | `Uuid` | log file names, `SAAIO_FLOW_ID`, `--json`, TOON, postcard, slots/, turns/, waits/seen |
| `SessionId` | `Uuid` | master.json, waits master session, `session` event lines, `--json` `session.id`, resume argv |
| `NotAnId` | the text | server.log (hook), serde errors |

All written as the same 36-character text as today.

## Call chains

saaio start:
1. `start::run -> Store::create(&first): Result<FlowId>`
2. `Store::create -> FlowId::generate(): FlowId -> Store::log(id): PathBuf`
3. `start::run -> append_workspace(store, id, &space, now) -> Store::append(id, &event)`
4. `start::run -> worktree::hook_env(id, name, repo, &space)` (SAAIO_FLOW_ID = id.to_string())
5. Error: create_new fails -> "could not create flow in ..." -> exit 1 (unchanged)

A hook names its flow:
1. `hook::run -> Env::flow_id(): Option<&str> -> FlowId::parse(text): Result<FlowId, NotAnId>`
2. Ok -> `Hooked::Flow(id) -> Store::append(id, &event)`
3. Err -> `Log::error("SAAIO_FLOW_ID \"x\" is no flow id")`, `Document::Silent` (text unchanged)
4. `wait::run`: Err -> `Document::Silent`. `stop.rs`: Err -> not inside a flow of this store.
5. `turn::run`: clap parses `--flow` via `FromStr`; Err -> usage error, exit 2

saaio remove:
1. `remove::run -> fold::resolve(&flows, &args.flow): Option<&FlowView>`
2. `fold::resolve -> FlowId::parse(name_or_id).ok()` compared with `flow.id`; then names
3. `remove::run -> client::close_flow(socket, flow.id, timeout) -> ClientMsg::CloseFlow(id)`
4. `Server::handle -> Panes::close_flow(conn, id) -> Store::events(id)` -> `Closed(id)` / `NotOpen(id)`
5. Error: resolve None -> `no_flow(text)`, exit 1 (unchanged)
6. Error: a CloseFlow whose id text does not parse fails to decode -> `proto::Error`, connection dropped (was NotOpen)

The server reads the store:
1. `Server -> Store::logs() -> Store::ids(): Result<Vec<FlowId>>` (non-id stems skipped)
2. `Store::logs -> Store::events(id), Store::last_beat(id) -> FlowLog { id, .. }`
3. `fold::views(&logs, now) -> FlowView { id, .. } -> Board`
4. Board -> serde_json / postcard / TOON, each writing `id.to_string()`

A worker's pane:
1. `Panes::open -> pane::spec(flow, agent, env, trusted)`
2. `spec -> SessionId::from(flow.id): own`
3. resume: `resume_argv(agent, own, &flow.name, flow.session.map(|s| s.id).unwrap_or(own))`
4. fresh: `agent_argv(agent, &prompt, own, &flow.name, &flow.request)`; plain: `plain_argv(agent, own, &flow.name)`

Codex and master sessions:
1. `codex::translate -> SessionId::parse(payload["session_id"]).ok()? -> EventKind::Session { id, .. }`
2. `hook::record_master_session -> Store::set_master_session(id)`
3. `Panes (master) -> Store::master_session(): Option<SessionId> -> MasterRecord::spec(start, .., session)`
4. `MasterRecord::load_or_create`: missing -> `SessionId::generate()`
5. Error: payload without a parsable session_id -> no Session event (unchanged)
6. Error: master.json whose session does not parse -> "could not read master.json" like broken JSON

## Test seams

- `id.rs` unit tests: generate round-trips; parse refuses uppercase, braced, simple, urn, short, a name;
  JSON and postcard bytes equal those of the same `String`; `Ord` matches text order;
  `SessionId::from(FlowId)` keeps the text.
- `store.rs`: `ids` skips a stray `notes.jsonl`; create/append on `FlowId`.
- `fold`: resolve by full id, by a name, by an uppercase id (no match).
- `view`: a FlowView's JSON is byte for byte what it was with a `String` id.
- `proto`: `CloseFlow` and a Board frame encode to the same bytes as with a `String` id.
- `tests/cli`: unchanged, must pass as is.

## Assumptions

1. One file `id.rs` holds both types (shared canonical parse and error).
2. Only canonical lowercase hyphenated text parses.
3. `Store::ids` silently skips a `*.jsonl` whose stem is no id.
4. The argv builders' `{id}` is a `SessionId`; for a worker `SessionId::from(flow.id)`.
5. `Env` keeps `SAAIO_FLOW_ID` as raw text; hook, wait and stop parse it.
6. A master.json with an unparsable session is an error like broken JSON.
7. No CHANGELOG entry; CLAUDE.md's module list gains `id.rs` and drops `is_uuid`.
8. `PaneKey` becomes `Copy`.

## Build notes

- Tasks 2 to 5 landed as one commit: typing `FlowView.id` breaks every module at once, and
  shims between the tasks would have been thrown away.
- `fixtures::flow_id` takes a name, not a number (above), and `fixtures::flow` keeps naming
  the flow, so tests read as before.
- `Waiter` is `Copy` too; `Waiter::key` returns a `String`.
- hook.rs `report(store, id: FlowId, ..) -> bool` and `workflow_for` lost their name-lookup
  fallback: `SAAIO_FLOW_ID` is now always a parsed id, so it never named a flow by name.
- `Host::open` and `Host::spawn` take `&PaneSpec` (clippy: the spec is no longer consumed once
  `PaneKey` is `Copy`).
- tests/cli seeded logs and slot claims under ids that are no UUID (`cccc`, `8a1b`,
  `scratch-id`, `f1`); they now use UUID names, since `Store::ids` skips the others
  (assumption 3). What each test checks is unchanged.
- Tests that sent `../7f3c` or a forged session id now cannot build one; proto's
  `writes_a_flow_id_as_the_text_it_was_before` shows such a frame fails to decode, and
  `a_session_that_is_no_id_does_not_parse` covers master.json.

## Tasks

- [x] Add `src/id.rs` with `FlowId`, `SessionId`, `NotAnId` and `canonical`, registered in lib.rs, with unit tests for round-trip, refused forms, JSON and postcard bytes equal to a `String`'s, `Ord` versus text order and `SessionId::from(FlowId)`
- [x] Type the store and the log: `Store` id arguments, `create`, `ids` (skipping non-id stems), `stamps`, `FlowLog.id`, `Waiter::Flow`, `master_session`/`set_master_session`; `EventKind::Session.id: SessionId`; `FlowView.id`, `view::Session.id`; `fold::changes`, `Folding::new`, `resolve` by parsed id; `fixtures::flow_id(n)` and `fixtures::flow(id: FlowId, ..)`; every caller and test compiling, with tests for `ids` skipping `notes.jsonl`, resolve by id/name/uppercase id and a FlowView's JSON unchanged
- [x] Type the protocol and client: `PaneKey::Flow(FlowId)` (Copy), `ClientMsg::CloseFlow`, `ServerMsg::Closed`/`NotOpen`, `client::close_flow`, server `known`, `Panes::close_flow` (is_uuid check gone), `reopen_worker`, `flow`, `current_flow`, reconcile, pause_watch, startup, forge_watch; UI `worker`, `in_view`, keys; with a test that `CloseFlow` and a Board frame encode to the same bytes as before
- [x] Type sessions and launch: `agent_argv`/`plain_argv`/`resume_argv` with `SessionId`, the worker spec through `SessionId::from(flow.id)`, `MasterRecord.session`, `MasterRecord::spec(.., Option<SessionId>)`, codex.rs `SessionId::parse`, `record_master_session`, peek `transcript(.., SessionId)`, `hook_env(id: FlowId, ..)`
- [x] Type the remaining flow ids: `Seen`, `deliver::worker_news`, `slots::Claim.flow`, `turns::Holder.flow`, hook.rs `Hooked::Flow` and helpers, wait.rs, stop.rs, turn.rs `Args.flow: FlowId`, start.rs `Rollback::new` and `append_workspace`; delete `event::is_uuid`; no `.id.clone()` left on a flow id
- [x] Docs: CLAUDE.md gains `id.rs` in the architecture list and `plan-flow-id.md` in the plan list, and loses `is_uuid`; `docs/HANDOVER.md` if it names `is_uuid`
- [x] Full checks: `cargo fmt`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test --all-features`, `cargo deny check`
- [x] End to end in a scratch store (`SAAIO_HOME=$(mktemp -d)`, `XDG_STATE_HOME` inside it): `saaio new`, `saaio report`, `saaio status` and `saaio status --json` show the same id text as the log file name; `saaio remove <id>` and `<name>` resolve; a UI in a PTY shows the flow; end with `SAAIO_HOME=<store> saaio --stop`
