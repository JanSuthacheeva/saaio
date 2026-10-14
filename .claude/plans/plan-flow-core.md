# flow core: event format and core commands

Implementation page (review surface): `~/projects/claude-code-mods/.lavish/flow-core-implementation.html`

Piece 1+2 of 6 of the flows system (event format + core CLI). Later pieces: the Claude Code
mod viewer (3) and adapter (4), `flow start` (5), auto mode in agent-skills (6).

## Decision

- `flow` is a Rust AXI (https://axi.md) in this repo. Its main user is an agent.
- It is the only writer of one append-only JSONL log per flow, and the only place that folds
  logs into what a flow looks like. The log format is private.
- Agents read TOON (default output). Programs (the mod) read `flow status --json`, the public
  contract, versioned with `VIEW_VERSION`.
- AXI rules: TOON output, minimal default fields, truncation with `--full`, aggregates
  (`count`, `needs_you`), definitive empty states, structured errors on stdout, exit codes
  0 ok / 1 error / 2 usage, idempotent mutations, no prompts, home view with no arguments
  (`bin`, `description`, live data), `help[]` next steps on every output, concise `--help`.
- Built in ship mode with implement-plan.

## Commands

```
flow                                   home view
flow new --name <n> --request <t> [--mode guided|auto]
flow report [--flow <name|id>] <event> flow from --flow or $FLOW_ID
  phase <design|implement|review|merge-request>
  working | delegated <description> | waiting <question> | blocked <reason>
  decision <text> | tasks <done> <total> | review --fixed <n> --open <n>
  merge-request <url> | ended | heartbeat
flow status [<name|id>] [--json] [--full]
flow clear
```

Logs: `$FLOW_HOME`, else `$XDG_STATE_HOME/flow`, else `$HOME/.local/state/flow`; files
`flows/<id>.jsonl` (events) and `flows/<id>.alive` (heartbeat mtime).

## Units

Crate and binary `flow`, edition 2024. Dependencies: `anyhow`, `clap` (derive, env), `serde`
(derive), `serde_json`, `serde_toon_format`, `uuid` (v4). Dev: `tempfile`.
`event.rs`, `fold.rs`, `view.rs`, `output.rs` are pure; `store.rs` owns all file I/O;
`main.rs` alone reads env and clock and writes stdout.

### src/main.rs

```rust
/// Tracks parallel feature flows: what each is doing, where it is blocked, how far it got.
#[derive(Debug, Parser)]
#[command(version, about)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,   // None: the home view
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Track a new flow, or return the unfinished flow of that name
    New {
        #[arg(long)] name: String,
        #[arg(long)] request: String,
        #[arg(long, value_enum, default_value = "guided")] mode: Mode,
    },
    /// Append one event to a flow
    Report {
        /// Flow name or id [default: $FLOW_ID]
        #[arg(long, env = "FLOW_ID")] flow: Option<String>,
        #[command(subcommand)] event: ReportEvent,
    },
    /// Every flow, or one flow with its phases, timeline and decisions
    Status {
        /// Flow name or id
        flow: Option<String>,
        /// Print the versioned JSON programs read, instead of TOON
        #[arg(long)] json: bool,
        /// Do not truncate long texts
        #[arg(long)] full: bool,
    },
    /// Forget flows that are done or ended
    Clear,
}

#[derive(Debug, Subcommand)]
enum ReportEvent {
    Phase { phase: Phase },
    Working,
    Delegated { description: String },
    Waiting { question: String },
    Blocked { reason: String },
    Decision { text: String },
    Tasks { done: u32, total: u32 },
    Review { #[arg(long)] fixed: u32, #[arg(long)] open: u32 },
    MergeRequest { url: String },
    Ended,
    /// Mark the flow alive without adding an event
    Heartbeat,
}

fn main() -> ExitCode;
fn run(cli: Cli, store: &Store, now: u64) -> Result<Document, Failure>;
fn home(store: &Store, now: u64) -> Result<Document, Failure>;
fn new_flow(store: &Store, name: String, request: String, mode: Mode, now: u64) -> Result<Document, Failure>;
fn report(store: &Store, flow: Option<String>, event: ReportEvent, now: u64) -> Result<Document, Failure>;
fn status(store: &Store, flow: Option<String>, json: bool, full: bool, now: u64) -> Result<Document, Failure>;
fn clear(store: &Store, now: u64) -> Result<Document, Failure>;
fn views(store: &Store, now: u64) -> (Vec<FlowView>, Vec<String>); // folded flows + warnings
fn now_ms() -> u64;

impl ReportEvent {
    /// The event to append; None for Heartbeat.
    fn into_kind(self) -> Option<EventKind>;
}
```

### src/event.rs

```rust
pub const EVENT_VERSION: u32 = 1;
pub const TEXT_LIMIT: usize = 500;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub v: u32,
    pub at: u64,
    #[serde(flatten)]
    pub kind: EventKind,
}

/// One line per event: {"v":1,"at":1791945698308,"kind":"blocked","reason":"..."}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum EventKind {
    Started { name: String, request: String, mode: Mode },
    Phase { phase: Phase },
    Working,
    Delegated { description: String },
    Waiting { question: String },
    Blocked { reason: String },
    Decision { text: String },
    Tasks { done: u32, total: u32 },
    Review { fixed: u32, open: u32 },
    MergeRequest { url: String },
    Ended,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum Phase { Design, Implement, Review, MergeRequest }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum Mode { Guided, Auto }

impl Event {
    /// Current version at `at` (Unix ms), each text cut to TEXT_LIMIT chars.
    pub fn new(at: u64, kind: EventKind) -> Event;
    /// One line of JSON, no trailing newline.
    pub fn to_line(&self) -> String;
    /// None for a broken line, another version or an unknown kind.
    pub fn from_line(line: &str) -> Option<Event>;
}

impl Phase {
    /// design, implement, review, merge-request - in flow order.
    pub const ALL: [Phase; 4];
    /// The name the command line and the logs use (added while building the fold).
    pub fn name(self) -> &'static str;
}
```

### src/store.rs

```rust
pub struct Store {
    root: PathBuf, // flows/<id>.jsonl, flows/<id>.alive
}

impl Store {
    /// $FLOW_HOME, else $XDG_STATE_HOME/flow, else $HOME/.local/state/flow.
    pub fn locate(flow_home: Option<PathBuf>, xdg_state: Option<PathBuf>, home: Option<PathBuf>) -> Result<Store>;
    pub fn at(root: PathBuf) -> Store;
    /// New flow with a fresh UUID v4 and its first event; returns the id.
    pub fn create(&self, first: &Event) -> Result<String>;
    /// Appends one event to an existing log (OpenOptions::append, one write_all per line).
    pub fn append(&self, id: &str, event: &Event) -> Result<()>;
    /// Every readable event of one flow, in order.
    pub fn events(&self, id: &str) -> Result<Vec<Event>>;
    /// Ids of all flows, by log file name.
    pub fn ids(&self) -> Result<Vec<String>>;
    /// Touches <id>.alive; its mtime is the last heartbeat.
    pub fn beat(&self, id: &str) -> Result<()>;
    /// Unix ms of the last heartbeat; None when never sent.
    pub fn last_beat(&self, id: &str) -> Result<Option<u64>>;
    /// Deletes the log and the heartbeat file.
    pub fn remove(&self, id: &str) -> Result<()>;
}
```

### src/fold.rs

```rust
/// A flow with a heartbeat older than this, not done or ended, is not responding.
pub const SILENT_AFTER_MS: u64 = 90_000;

/// One flow's view at `now`; None when the log has no Started event.
pub fn fold(id: &str, events: &[Event], last_beat: Option<u64>, now: u64) -> Option<FlowView>;
/// Sorted: needs you, running, finished; newest change first within each group.
pub fn board(flows: Vec<FlowView>, now: u64) -> Board;
/// Exact id, else the unfinished flow of that name, else the most recent finished one.
pub fn resolve<'a>(flows: &'a [FlowView], name_or_id: &str) -> Option<&'a FlowView>;
```

### src/view.rs (the `--json` contract)

```rust
pub const VIEW_VERSION: u32 = 1;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Board { pub version: u32, pub now: u64, pub count: usize, pub needs_you: usize, pub flows: Vec<FlowView> }

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FlowView {
    pub id: String,
    pub name: String,
    pub request: String,
    pub mode: Mode,
    pub phase: Option<Phase>,
    pub state: State,
    pub blocker: Option<String>,
    pub delegate: Option<String>,
    pub tasks: Option<Tasks>,
    pub review: Option<ReviewCount>,
    pub merge_request: Option<String>,
    pub decisions: Vec<String>,
    pub steps: Vec<Step>,           // always the four phases, in order
    pub timeline: Vec<TimelineEntry>,
    pub started_at: u64,
    pub changed_at: u64,            // last event that changed state, phase or blocker
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum State { Working, Delegated, Waiting, Blocked, NotResponding, Done, Ended }

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Step { pub phase: Phase, pub status: StepStatus, pub duration_ms: Option<u64> }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum StepStatus { Done, Current, Open }

#[derive(Debug, Clone, Serialize)]
pub struct Tasks { pub done: u32, pub total: u32 }

#[derive(Debug, Clone, Serialize)]
pub struct ReviewCount { pub fixed: u32, pub open: u32 }

#[derive(Debug, Clone, Serialize)]
pub struct TimelineEntry { pub at: u64, pub text: String, pub tone: Tone }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Tone { Neutral, Progress, Attention, Problem, Success }

impl Board {
    /// Pretty JSON, camelCase keys, `version` first, nothing truncated.
    pub fn to_json(&self) -> String;
}

impl FlowView {
    pub fn needs_you(&self) -> bool;   // waiting, blocked or not responding
    pub fn is_finished(&self) -> bool; // done or ended
}
```

### src/output.rs

```rust
/// List views cut long texts to this many chars, with a hint; --full lifts it.
pub const LIST_TEXT_LIMIT: usize = 80;

pub enum Document { Toon(String), Json(String) }

pub struct Failure {
    pub message: String,
    pub help: Vec<String>,
    pub code: u8, // 1 error, 2 usage
}

impl Document {
    pub fn text(&self) -> &str;
}

impl Failure {
    pub fn error(message: impl Into<String>, help: Vec<String>) -> Failure; // code 1
    pub fn usage(clap_error: &clap::Error) -> Failure;                      // code 2
    pub fn from_io(error: anyhow::Error) -> Failure;                        // code 1
    pub fn to_toon(&self) -> String;                                        // error: ..., help[n]: ...
}

/// bin, description, count, needs_you, flows[n]{name,state,phase,tasks,age}, blockers[n]{name,blocker}, warnings, help.
/// blockers lists the open question or block reason of every flow that needs you (renamed from blocked while building).
pub fn home(board: &Board, bin: &str, warnings: &[String]) -> Document;
/// count, needs_you, flows table, blockers, warnings, help; "flows: none tracked" when empty.
pub fn status_list(board: &Board, warnings: &[String], full: bool) -> Document;
/// name, request, mode, state, phase, blocker, tasks, review, merge_request,
/// steps[4]{phase,status,duration}, decisions[n], timeline[n]{ago,text}, help. Absent values are left out, not null.
pub fn status_detail(flow: &FlowView, now: u64, full: bool) -> Document;
/// flow, id, existing, help with `export FLOW_ID=<id>`.
pub fn created(flow: &FlowView, existing: bool) -> Document;
/// flow, state, phase, next step for the new state.
pub fn reported(flow: &FlowView) -> Document;
/// skipped: no flow id; help: set FLOW_ID or pass --flow.
pub fn skipped() -> Document;
/// cleared: n, names[n], help.
pub fn cleared(names: &[String]) -> Document;
/// 45s, 12m, 3h, 2d.
pub fn short_duration(ms: u64) -> String;
/// Cut to `limit` chars with " (truncated, N chars - use --full)" unless `full`.
pub fn truncated(text: &str, limit: usize, full: bool) -> String;
```

TOON keys are snake_case; help texts keep fixed flags and use `<name>` placeholders.

## Fold rules

| Event | State | Other fields | Timeline |
| --- | --- | --- | --- |
| started | working | name, request, mode, startedAt | started (neutral) |
| phase p | working | phase = p; blocker, delegate cleared; previous step done with duration; same phase again: no change | phase: p (progress) |
| working | working | blocker, delegate cleared | answered (progress), only after waiting/blocked |
| delegated d | delegated | delegate = d | - |
| waiting q | waiting | blocker = q | waiting: q (attention); same q repeated: no entry |
| blocked r | blocked | blocker = r | blocked: r (problem); same r repeated: no entry |
| decision t | unchanged | decisions += t unless present | decided: t (neutral) |
| tasks d t | unchanged | tasks = d/t | - |
| review f o | unchanged | review = f/o | review: f fixed, o open (progress; attention if o > 0) |
| merge-request u | done | phase = merge-request, url, all steps done | MR opened (success) |
| ended | ended, unless done | current step stops counting | ended (neutral) |

After the events: a heartbeat older than `SILENT_AFTER_MS` turns any state except done/ended
into not-responding; a flow that never sent a heartbeat never does. The current step's
duration runs until `now`. Repeated reports fold to the same view.

## Call chains

### Every invocation
1. main -> Cli::try_parse(): Cli
2. Err(clap::Error): --help/--version print as clap does, exit 0; any other -> Failure::usage(&e).to_toon() on stdout, exit 2
3. Store::locate(env FLOW_HOME, env XDG_STATE_HOME, env HOME); none -> Failure "cannot find a state directory", help "Set FLOW_HOME to a directory", exit 1
4. run(cli, &store, now_ms()) -> home() when no command, else the command fn
5. Ok(doc) -> print doc.text(), exit 0; Err(f) -> print f.to_toon(), exit f.code

### flow new
1. new_flow -> views(&store, now) -> fold::resolve(&views, name)
2. unfinished flow of that name -> output::created(flow, existing: true), nothing written
3. else Event::new(now, Started { name, request, mode }) -> store.create(&event): id -> fold::fold(&id, &[event], None, now)
4. create fails -> Failure::from_io ("could not create flow in <dir>: <error>"), exit 1
5. output::created(&flow, false): flow, id, existing, help "Run `export FLOW_ID=<id>` in the worker", "Run `flow report phase design` when design starts"

### flow report
1. flow None (no --flow, no $FLOW_ID) -> output::skipped(), exit 0
2. views(&store, now) -> fold::resolve(&views, &flow)
3. None -> Failure "no flow named <x>", help "Run `flow` to list the flows", "Run `flow new --name <x> --request <text>` to start it", exit 1
4. into_kind(): Some(kind) -> store.append(&id, &Event::new(now, kind)); None -> store.beat(&id)
5. io error -> Failure::from_io, exit 1
6. fold::fold(&id, &store.events(&id)?, store.last_beat(&id)?, now) -> output::reported(&flow) with help by state (blocked: "Run `flow report working` once the block is resolved"; waiting: "... once answered"; done: "Run `flow clear` after the MR is merged")

### flow / flow status
1. views(&store, now): per store.ids() -> events, last_beat -> fold::fold; a flow whose events() fails is left out and added to warnings ("cannot read <id>: <error>")
2. fold::board(views, now)
3. --json -> Document::Json(board.to_json()) (named: board with only that flow), never truncated
4. no command -> output::home(&board, bin with $HOME as ~, &warnings)
5. status without name -> output::status_list(&board, &warnings, full); empty -> "count: 0", "flows: none tracked", help to `flow new`
6. status <name> -> fold::resolve -> output::status_detail(flow, now, full); None -> Failure "no flow named <x>", exit 1

### flow clear
1. views(&store, now) -> finished ones -> store.remove(&id) each
2. output::cleared(&names); nothing finished -> "cleared: 0"
3. remove fails -> Failure naming the id, the rest stays removed, exit 1

## Test seams

- event.rs: round trip per kind; 500-char cut; from_line None for broken JSON, version 2, unknown kind.
- fold.rs: every fold rule, idempotent repeats, durations with fixed `now`, not-responding only with a heartbeat, board order, resolve by id / unfinished name / finished name.
- store.rs (tempfile::TempDir): create/append/events, append to missing flow fails, beat/last_beat, ids, remove, locate precedence.
- output.rs: exact TOON for home, empty status, list with truncation hint, detail, created (new/existing), reported per state, skipped, error, usage error.
- tests/cli.rs: built binary with FLOW_HOME in a temp dir: new twice (same id), reports, `status --json` parsed with serde_json, home, unknown flag exits 2 with TOON on stdout, missing flow exits 1, clear.

## Assumptions

1. Ids are UUID v4 (reused later as the worker's `--session-id`); names work wherever ids do.
2. `flow new` with an unfinished flow's name returns that flow (`existing: true`).
3. Stored texts cut to 500 chars; list views to 80 with a hint; `--json` and `--full` never truncate.
4. Unknown event kinds and broken lines are skipped when folding (forward compatible).
5. Not-responding after 90 s without a heartbeat, only for flows that ever sent one.
6. TOON via `serde_toon_format` 0.2; if it misencodes, `output.rs` gets a small encoder for scalars, inline arrays and tabular arrays behind the same functions.
7. `--json` keys camelCase; TOON keys snake_case.
8. Ambient context (AXI principle 7) comes later: the mod board in Claude Code, a `flow setup` hook and skill stub with piece 6.

## Decided in review

- AXI instead of a plain CLI; TOON by default, `--json` for programs.
- The CLI folds; readers never parse the log.
- Command surface and log location as above.
- `flow report` without a flow id: `skipped`, exit 0.
- Ship mode: implement-plan builds it.
- Deviations from wordstamp: `main` returns `ExitCode` (errors as TOON on stdout); no `dirs` crate (`state_dir()` is None on macOS).

## Tasks

### Skeleton

- [x] Create the crate: Cargo.toml with the dependencies, src/main.rs with an empty `Cli`, .gitignore, CLAUDE.md (build/test commands, module map); `cargo build`, `cargo clippy -- -D warnings`, `cargo fmt --check` pass

### Events and store

- [x] Add event.rs: `Event`, `EventKind`, `Phase`, `Mode`, `Event::new`, `to_line`, `from_line`, with unit tests for round trips, the 500-char cut and broken, version-2 and unknown-kind lines
- [x] Add store.rs: `Store::locate`, `at`, `create`, `append`, `events`, `ids`, with unit tests on a TempDir including append to a missing flow and locate precedence
- [x] Add `Store::beat`, `last_beat`, `remove` with unit tests

### Fold

- [x] Add view.rs types, `Board::to_json`, `FlowView::needs_you` and `is_finished`, with a test for the JSON shape (camelCase, version first)
- [x] Add `fold::fold` with a unit test per fold rule, idempotent repeats and phase durations
- [x] Add not-responding, `fold::board` ordering and `fold::resolve`, with unit tests

### Output

- [x] Add output.rs `Document`, `Failure` (error, usage, from_io, to_toon), `truncated`, `short_duration`, with unit tests; confirm `serde_toon_format` encodes tabular arrays as in the plan, else add the fallback encoder
- [x] Add `output::home`, `status_list`, `status_detail` with exact TOON tests, including the empty state, truncation hint and warnings
- [ ] Add `output::created`, `reported`, `skipped`, `cleared` with exact TOON tests

### Commands

- [ ] Wire main.rs: `try_parse` with `Failure::usage` and exit 2, `Store::locate`, `run`, `home`, `status` (TOON and `--json`), `views`, with tests/cli.rs cases for home, status, `--json` and an unknown flag
- [ ] Add `new_flow` (idempotent), `report` (events, heartbeat, skipped) and `clear`, with tests/cli.rs cases for each and the missing-flow error
- [ ] Write README.md: install, commands, the TOON views, the `--json` contract
- [ ] End-to-end: `cargo install --path .`, two shells reporting into one flow at once, `flow` and `flow status --json` in a third; then a fresh agent session told only "use `flow` to report your progress" drives it from the home view and help lines alone
