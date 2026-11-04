# One heavy run per project

Review pages: approaches https://claude.ai/artifact/5P1NTZSBz9e7Gn48nB18RM (`docs/design/exclusive-runs-approaches.html`),
implementation https://claude.ai/artifact/Gqofo9iZoZ2syECQ7fMEfi (`docs/design/exclusive-runs-implementation.html`).

## Decision

- Approach A: a flow's PreToolUse hook takes the project's turn, an flock on
  `<store>/turns/0.lock`, before a Bash line that runs a command the repo lists under
  `[exclusive] commands` in `.saaio/config.toml`. While another flow holds it, the hook waits
  (appending `TurnWaiting` once, `TurnTaken` at the end) and the sidebar says so.
- Once taken, the hook hands the locked file as stdin to a detached hidden `saaio turn --flow <id>
  --agent <pid>`, which keeps it while any process under the flow's agent runs a listed command.
  The kernel drops the flock when its holder dies: no stale lock.
- The agent's pid: the server records it in `waits/<id>.agent` when it opens a worker pane
  (Claude Code runs its shells in a process group of their own, so `getsid` in the hook is not
  reliable). The hook checks that pid is its own ancestor.
- The person's switch: `[exclusive] enabled` in their config.toml, `true` unless set. The repo
  says what is heavy; the person decides whether their machine takes turns.
- A call waits at most 30 minutes (`TURN_WAIT`), then runs anyway, logged. PreToolUse's timeout
  in both adapters rises from 150 s to 1980 s.
- `saaio status --json`: `FlowView.turn: { command, holder, since }`; TOON: a `turn` column, empty
  or `waiting`. `VIEW_VERSION` stays 3 (additive).
- Scope is the project (the store). Machine-wide later: `Turns` in the shared folder's `turns/`
  with a `limit`, and `[exclusive] scope`/`limit` in the person's config. Not built now.
- Every error fails open: the call runs unheld, the reason goes to server.log, stdout stays silent.
- Decided after the harness check (docs/HANDOVER.md, 2026-11-04): Codex holds like Claude Code,
  though its model may call a held command slow, send it again or give up; a flow's own second
  heavy call waits for its first (no `Take::Own`). The docs say so.

## Units

### src/repo_config.rs (new)

`RepoConfig` and `CONFIG_FILE` move here from `branching.rs`; start.rs and ui.rs call
`RepoConfig::load(&repo)?.branch`.

```rust
pub const CONFIG_FILE: &str = ".saaio/config.toml";

#[derive(Debug, Default, Deserialize)]
pub struct RepoConfig {
    pub branch: Option<Branching>,
    pub exclusive: Option<Exclusive>,
}

impl RepoConfig {
    /// <repo>/.saaio/config.toml; the default without the file; Err when it cannot be read or parsed.
    pub fn load(repo: &Path) -> Result<RepoConfig>;
}
```

### src/exclusive.rs (new, pure)

```rust
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Exclusive {
    #[serde(deserialize_with = "commands")]
    pub commands: Vec<String>,
}

impl Exclusive {
    /// The first listed command `line` runs: its words in a row, on word boundaries, anywhere in
    /// the line. A boundary: not a letter, digit, `_`, `.` or `-` on either side.
    pub fn runs(&self, line: &str) -> Option<&str>;
}

/// Refuses an empty entry or one of only spaces.
fn commands<'de, D: Deserializer<'de>>(de: D) -> Result<Vec<String>, D::Error>;
```

### src/config.rs + examples/config.toml (modified)

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ExclusiveConfig {
    /// Calls that run a command the repo lists wait their turn. true unless set.
    pub enabled: bool,
}

pub struct Config { ..., pub exclusive: ExclusiveConfig }
```

```toml
#[exclusive]
# A tool call that runs a command the repo lists under [exclusive] in .saaio/config.toml
# waits while another worker of the project runs one. false lets them run at once.
#enabled = true
```

### src/turns.rs (new)

Opens its files through `private::options()` like `Slots::lock`; std `File::try_lock`. The holder
is written into the locked file in place (`set_len(0)` + write), never `write_atomic`: a new
inode would be a second lock.

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Holder {
    pub flow: String,
    pub name: String,
    pub command: String,
}

#[derive(Debug, Clone)]
pub struct Turns { dir: PathBuf, limit: u8 }

#[derive(Debug)]
pub struct Turn { file: File }

pub enum Take {
    Taken(Turn),
    /// Every turn is held; the holder of the first.
    Held(Option<Holder>),
}

impl Turns {
    /// <store>/turns, one turn.
    pub fn of_project(store: &Store) -> Turns;
    /// Tries every lock file without waiting.
    pub fn try_take(&self, holder: &Holder) -> Result<Take>;
}

impl Turn {
    pub fn into_file(self) -> File;
}
```

### src/processes.rs (new)

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Process { pub pid: u32, pub ppid: u32, pub command: String }

/// `ps -A -o pid=,ppid=,command=` through workspace::process::output, 1 s deadline.
pub fn list() -> Result<Vec<Process>>;
fn parse(text: &str) -> Vec<Process>;
/// The processes below `root`.
pub fn under(all: &[Process], root: u32) -> Vec<&Process>;
/// True when `root` is `pid` or one of its ancestors.
pub fn descends(all: &[Process], pid: u32, root: u32) -> bool;
```

### src/turn_watch.rs (new, pure)

```rust
#[derive(Debug, Clone, Copy)]
pub struct Look { pub agent_alive: bool, pub runs: bool, pub asks: bool }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Keep { Hold, Release(Release) }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Release { AgentGone, RunEnded, NeverRan }

pub const GRACE: Duration = Duration::from_secs(10);

pub struct TurnWatch { /* since, seen, quiet looks, time spent asking */ }

impl TurnWatch {
    pub fn new(now: u64) -> TurnWatch;
    /// Hold or release after `look` at `now`: AgentGone at once; RunEnded after two quiet looks in
    /// a row once a run was seen; NeverRan past GRACE with no run seen, the time `asks` not counted.
    pub fn step(&mut self, now: u64, look: Look) -> Keep;
}
```

### src/cli/commands/turn.rs (new, hidden)

```rust
#[derive(Debug, clap::Args)]
pub struct Args {
    #[arg(long)]
    flow: String,
    #[arg(long)]
    agent: u32,
    /// The processes between the hook and the agent: the harness itself, never a run
    #[arg(long, value_delimiter = ',')]
    harness: Vec<u32>,
}

/// Looks every LOOK_STEP (500 ms): processes::list, under(agent) without `harness` (Codex's
/// native binary under the node agent carries the prompt in its command line), Exclusive::runs
/// on each command, the flow's permission from its fold; TurnWatch::step until it releases; logs why to
/// server.log ("turn watch flow <id>: ..."). A failed list counts as a run until 3 in a row fail,
/// then it releases. Exiting closes stdin, which releases the turn.
pub fn run(args: &Args, env: &Env, clock: fn() -> u64) -> Document;
```

cli.rs: `#[command(hide = true)] Turn(turn::Args)`, dispatched before the store check like Hook.

### src/cli/commands/hook.rs (modified)

```rust
const TURN_WAIT: Duration = Duration::from_mins(30);
const TURN_STEP: Duration = Duration::from_millis(500);

/// Takes the project's turn for `line` when its repo lists a command it runs; waits while
/// another flow holds it; hands it to `saaio turn`. Fails open, logged.
fn wait_for_turn(store: &Store, id: &str, line: &str, log: &Log, clock: fn() -> u64);

/// `current_exe() turn --flow id --agent pid --harness <the hook's ancestors below the agent>`,
/// process_group(0), stdin = the turn, stdout and stderr null.
fn keep_turn(turn: Turn, id: &str, agent: u32, harness: &[u32]) -> Result<()>;
```

In `run`, after `wait_for_services`: `if config.exclusive.enabled && let Some(line) =
adapter.command(&payload) { wait_for_turn(..) }`.

### src/adapter.rs, claude_code.rs, codex.rs (modified)

```rust
impl Adapter {
    /// The shell line of a PreToolUse Bash call; None for any other event or tool.
    pub fn command(self, payload: &Value) -> Option<&str>;
}
const TOOL_HOOK_TIMEOUT_SECS: u64 = 1_980; // both adapters
```

### src/store.rs (modified)

```rust
impl Store {
    /// waits/<id>.agent; removed in Store::remove.
    pub fn set_agent(&self, id: &str, pid: u32) -> Result<()>;
    pub fn agent(&self, id: &str) -> Option<u32>;
}
```

### src/server/host.rs, panes.rs (modified)

`Host::open(&mut self, spec: PaneSpec) -> Result<u32>` returns the child's pid;
`Panes::open_pane` calls `store.set_agent(id, pid)` for `PaneKey::Flow(id)`, an error logged.

### src/event.rs, view.rs, fold.rs (modified)

```rust
EventKind::TurnWaiting { command: String, holder: String }   // "turn-waiting"
EventKind::TurnTaken                                          // "turn-taken"

pub struct FlowView { ..., #[serde(default)] pub turn: Option<TurnWait> }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnWait { pub command: String, pub holder: String, pub since: u64 }
```

Fold: TurnWaiting sets `turn` (since = event's at); TurnTaken, Idle, Waiting, Blocked and Ended
clear it. `fold::changes`: TurnWaiting always; TurnTaken only while `turn` is Some.

### UI and output (modified)

- `ui/sidebar/labels.rs`: `state_label` says `waits turn` (theme.ui.muted) while `flow.turn` is
  Some; glyph unchanged; `note` (third row) says `after <holder>: <command>`.
- `ui/board/cards.rs`: `working · waits turn`, like `· paused`.
- `ui/board/detail.rs`: `waits its turn for <command> since <time>; <holder> runs`.
- `cli/output/status.rs`: a `turn` column, empty or `waiting`.

## Call chains

### A listed command, the turn free

1. hook::run -> config.exclusive.enabled: true (false: nothing more)
2. hook::run -> Adapter::command(&payload): Some(line)
3. hook::run -> wait_for_turn(store, id, line, log, clock)
4. wait_for_turn -> fold::fold(id, events): FlowView -> repo_dir(&flow): PathBuf
5. wait_for_turn -> RepoConfig::load(&repo): RepoConfig -> Exclusive::runs(line): Some(command)
6. wait_for_turn -> Store::agent(id): Some(pid) -> processes::list() -> processes::descends(all, own pid, pid): true; harness = the hook's ancestors below pid
7. wait_for_turn -> Turns::of_project(store).try_take(&holder): Take::Taken(turn)
8. wait_for_turn -> keep_turn(turn, id, pid, &harness): Ok -> the hook exits 0, the tool runs
9. turn::run -> TurnWatch::new(now); every 500 ms: processes::list -> under(all, agent) -> Exclusive::runs -> fold for permission -> TurnWatch::step(now, look): Hold ... Release(RunEnded)
10. turn::run returns Document::Silent; stdin closes; the flock drops

### Another flow holds the turn

1. wait_for_turn -> Turns::try_take: Held(Some(holder))
2. wait_for_turn -> Store::append(id, TurnWaiting { command, holder: holder.name }) once
3. server StoreWatch -> fold: FlowView.turn -> Board -> sidebar `waits turn`, `after shop-42: pint`
4. every TURN_STEP: Turns::try_take ... Taken(turn)
5. wait_for_turn -> Store::append(id, TurnTaken) -> keep_turn(turn, id, pid, &harness)
6. Error: TURN_WAIT ran out -> append TurnTaken -> log "waited 30m for the turn (<holder> runs <command>); the tool runs" -> return
7. Error: the harness kills the hook -> no TurnTaken; the next Idle, Waiting, Blocked or Ended clears `turn`

### The same flow holds the turn

1. wait_for_turn -> Turns::try_take: Held(Some(holder)) with holder.flow == id -> waits like any
   other flow: a second heavy run of one worker (Codex re-sending a held call) never runs beside
   its first.

### Watcher releases

1. look.agent_alive false -> Release(AgentGone)
2. two quiet looks after a run was seen -> Release(RunEnded) (finished, killed, background process exited)
3. GRACE without a run, not counting time asking -> Release(NeverRan), logged
4. Error: processes::list fails -> logged, counted as running; 3 failures in a row -> release
5. The watcher is killed -> the kernel drops the flock

### Errors before a turn (all: logged, the call runs unheld)

1. RepoConfig::load: Err
2. Store::agent: None, or the pid is not the hook's ancestor
3. Turns::try_take: Err
4. keep_turn: Err (the turn drops when the hook exits)

## Test seams

- exclusive.rs: table of lines - Sail, `docker compose exec`, `docker exec`, `./vendor/bin/pint`,
  `pint.json`, `test:parallel`, quotes, `&&`, multi-word commands, no match.
- repo_config.rs: temp dirs; branch and exclusive together, missing file, invalid entries.
- config.rs: `[exclusive] enabled` default and set; the example test covers the commented line.
- turns.rs: take, held by another handle (the same flow too), released on drop, holder read back.
- processes.rs: parse, under, descends on fixed text; list finds the test's own pid.
- turn_watch.rs: every Release path, grace paused while asking, two quiet looks.
- fold.rs: TurnWaiting/TurnTaken, cleared by Idle and Ended, `changes`.
- labels, cards, detail, status: existing render tests and goldens.
- tests/cli/turn.rs: real binary, scratch store, two flows; A's hook takes the turn while a
  process named `pint` runs under a fake agent; B's hook waits and logs TurnWaiting, then goes on
  after the process is killed; `enabled = false` lets B through at once.

## Assumptions

- Word boundary: not a letter, digit, `_`, `.` or `-`. A line that only mentions a command waits too.
- `sail test` is caught only when listed; the docs example lists it.
- Waiters are not served strictly first come.
- Every flow session waits, plain ones included, when its folder's repository lists commands; the
  master never waits.
- Sidebar: spinner kept, label `waits turn` muted, third row `after shop-42: pint`.
- A turn whose run never shows is let go after 10 s; a permission prompt pauses that clock.
- Only the Bash tool (Claude Code's Bash, Codex's shell) is matched.
- Taking turns needs no repository trust.
- A run is a process below the agent. One the agent detaches itself (`cmd &` inside a call,
  `nohup`) is re-parented away and no longer counts; Claude Code's background shells and Codex's
  yielded commands stay below the agent and do. The docs say so.

## Tasks

- [x] Verify in both harnesses: a PreToolUse hook with a 1980 s timeout is accepted and may run that long (Codex's cap); a hook's process ancestors include the pane's agent pid. Record findings in docs/HANDOVER.md; stop and ask if either fails.
- [x] Add `repo_config.rs` (`RepoConfig::load`, `CONFIG_FILE`), move `Branching` callers onto it, with tests
- [x] Add `exclusive.rs` (`Exclusive`, `runs`, `commands` validation) and `RepoConfig.exclusive`, with the matching table tests
- [x] Add `ExclusiveConfig` to config.rs and the commented `[exclusive]` block to examples/config.toml, with tests
- [x] Add `turns.rs` (`Turns`, `Turn`, `Holder`, `Take`, `try_take`, `into_file`) with lock tests
- [x] Add `processes.rs` (`list`, `parse`, `under`, `descends`) with tests
- [x] Add `turn_watch.rs` (`TurnWatch`, `Look`, `Keep`, `Release`) with tests for every release
- [x] Add `TurnWaiting`/`TurnTaken` events, `FlowView.turn`/`TurnWait`, and their fold and `changes`, with fold tests
- [x] Add `Store::set_agent`/`agent` (removed with the flow); `Host::open` returns the pid; `Panes::open_pane` records it; tests
- [x] Add `Adapter::command` and raise both adapters' PreToolUse timeout to 1980 s, with adapter tests
- [x] Add the hidden `saaio turn` command (`cli/commands/turn.rs`)
- [x] Add `wait_for_turn` and `keep_turn` to hook.rs behind `config.exclusive.enabled`; tests/cli/turn.rs end to end
- [x] Show the wait: sidebar label and note, board card, detail, status `turn` column; update goldens
- [x] Docs: docs/repository.md (heavy commands section with the Sail example), docs/configuration.md (`[exclusive] enabled`), docs/cli.md and docs/json.md (`turn`), docs/ui.md (sidebar), the Codex caveat in docs/repository.md, CHANGELOG Unreleased Added, ROADMAP Planned future (machine-wide limit), CLAUDE.md architecture lines for the new modules
- [x] End to end: scratch store, two Claude workers in a repo with `[exclusive]`; one runs a slow listed command in the background, the other's call waits with `waits turn` in the sidebar and goes on when the first process ends; kill a watcher and see the turn free; `enabled = false` lets both run

### After review (decided 2026-11-04)

- [x] Check by hand in a scratch E2E: Esc in a Claude Code worker while its heavy call waits for its turn. If `waits turn` stays through the next prompt, clear `turn` when a new prompt arrives (a prompt event of its own, since a repeated `Working` is folded away), with fold and adapter tests
- [x] docs/repository.md: note that two heavy calls one Claude Code turn runs in parallel may stall about 10 s and then run at once

## Review

Isolated review of `flow/exclusive-runs` against `main` and this plan (2026-11-04).

### Fixed

- docs/json.md: the new Turn section replaced the `### Workspace` heading, leaving the Workspace
  table under Turn and the `#workspace` link dead (6c17a9b).
- docs/cli.md: the `saaio status --all` example the branch touched lacked the `kind` column the
  code prints (`project,name,kind,...`) (6c17a9b).

### Open

- A Claude Code Esc while a call waits for its turn: Claude Code runs no hook on Esc
  (HANDOVER), so if it kills the waiting hook no `TurnTaken` follows and `waits turn` stays on
  the flow through its whole next turn, until a Stop (Idle) clears it; if it does not kill the
  hook, the hook keeps waiting and takes the turn later, released after GRACE. Not proven either
  way. Recommendation: check it once by hand; if it sticks, clear `turn` on UserPromptSubmit
  (a prompt event of its own, since `Working` is folded away by `changes`).
  Decided: check it by hand, fix if it sticks (task under After review).
- Parallel heavy calls of one Claude Code turn: if Claude Code runs every PreToolUse hook of a
  batch before any tool, the second hook waits for a run that cannot start until it returns;
  NeverRan frees it after 10 s and both then run at once. No deadlock, but a 10 s stall and no
  exclusivity. Recommendation: accept; note it if seen in practice.
  Decided: accept, documented in docs/repository.md (task under After review).
- `TurnWaiting` names the holder at the first try only; a third flow taking the turn later is
  not shown, and a holder read mid-write shows as "another flow". Cosmetic; accept. Decided: accepted.
- `saaio turn` folds the flow's whole log every 500 ms for `asks` while a run lasts. Fine for
  today's log sizes; revisit if logs grow large. Decided: accepted.

### Where to look

1. `src/cli/commands/hook.rs` `wait_for_turn` -> `agent_of` -> `keep_turn`: the lock handed as
   stdin to a detached `saaio turn`, the harness pids, every fail-open path.
2. `src/cli/commands/turn.rs` `run` / `look` with `src/turn_watch.rs` `step`: when a turn is let
   go (agent gone, two quiet looks, 10 s grace paused while asking, 3 failed `ps`).
3. `src/turns.rs` `try_take`: the holder written in place into the locked file.
4. `src/fold.rs`: what clears `turn` (TurnTaken, Idle, Waiting, Blocked, Ended).

Routine, skim: repo_config.rs (moved from branching.rs), exclusive.rs matching table, config
`[exclusive] enabled` and examples/earlier/config-v4.toml (byte-identical to main's example),
UI labels, status column, docs.

### Checks

`cargo test`, `cargo clippy --all-targets -- -D warnings` and `cargo fmt --check` passed at
2318ba4; later commits change only the plan and docs. The fixes are docs only, so no production
code needed a second review.

### Second review

Isolated review of the commits after 4c3fe47 (387730b, e9e0d06, 0ebd781): the `prompted` event and
the docs under After review (2026-11-04).

- Fixed: `a_turn_stops_waiting_when_the_flow_stops` now also covers `Prompted`, which does not stop
  the flow; renamed to `..._or_gets_a_prompt` (test only).
- Checked, no finding: `prompted` is appended only while `turn` is set (`fold::changes`), so logs do
  not grow by a line a prompt; an older binary skips the unknown kind (`Event::from_line`);
  `master_state` ignores it; an ended flow's late `prompted` can only clear `turn`, which `Ended`
  already cleared; the hook re-reads the log before each kind, so `Working` then `Prompted` from one
  payload fold in order.
- Open: whether a harness can fire `UserPromptSubmit` while a hook still waits for its turn (a
  prompt queued or steered mid-turn, notably Codex). Then `waits turn` disappears early while the
  call still waits; cosmetic, the call itself is unaffected. Recommendation: accept; revisit only if
  seen.
- The hand check (Esc kills the waiting hook, recorded in HANDOVER) was not repeated here.

Checks: `cargo test`, clippy and `cargo fmt --check` passed at e9e0d06; the second review changed
only a test name (that test and fmt re-run).
