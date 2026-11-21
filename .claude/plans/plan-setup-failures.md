# Plan: setup failures - keep the cause, re-run setup

Implementation page: https://claude.ai/artifact/74CfyV3njqXHRDV4ZukJtF
(approaches: https://claude.ai/artifact/PLSg8EDYjBFhdrWdUHUDFt)

## Decision

- A failed `.saaio/setup` kept only its last stderr line, and its `Blocked` state was cleared by
  the worker's first prompt (`Working`) seconds later, so status lost it and the worker was never told.
- A new `Setup` event records every setup run's outcome and log path. `fold` keeps it as a sticky
  `FlowView.setup` that only the next setup run replaces. Status, `--json`, the UI, the master's
  news and a worker note carry the failure and its log until a run passes.
- Every hook's stdout and stderr go straight into one file, in order:
  `<store>/hooks/<flow-id>/<hook>.log` (setup, teardown, pause, resume), each run replacing the
  last, removed with the flow. `saaio remove` writes `teardown.log` too (one code path).
- `saaio setup [name]`: teardown, then setup, with the flow's slot and env. Without a name it takes
  the flow of the worker that runs it (`$SAAIO_FLOW_ID`); a worker may run it on its own flow.
- UI: `C-space s` on the worker in view, `s` on the board's selected card, both confirmed with `y`.
- No automatic retries anywhere.

## Units

### `src/private.rs` (modified)

```rust
/// An empty 0600 file at `path` in place of anything there, a link too; its folder created 0700.
pub fn create_fresh(path: &Path) -> io::Result<File>;
```

### `src/workspace/process.rs` (modified)

```rust
/// Runs `command` with no stdin in a process group of its own, stdout and stderr both into `log`;
/// Err with the last non-empty line `path` holds on a non-zero exit, or "no answer within ..." past
/// `limit`, the whole group killed (what it printed until then stays in the file).
pub(super) fn finish_into(command: Command, log: File, path: &Path, program: &str,
    limit: Duration) -> Result<()>;

/// The last non-empty line of the file's last 64 KiB, or "failed without a message".
fn last_line_of(path: &Path) -> String;
```

### `src/workspace/hooks.rs` (modified)

The log file is created only when the hook exists and trust covers the repo.

```rust
pub fn run(trust: &Trust, repo: &Path, hook: Hook, workspace: &Workspace,
    env: &[(String, String)], log: &Path) -> Result<bool>;
pub fn run_within(trust: &Trust, repo: &Path, hook: Hook, workspace: &Workspace,
    env: &[(String, String)], log: &Path, limit: Duration) -> Result<bool>;
```

### `src/store.rs` (modified)

```rust
/// Where the flow's last run of the hook named `hook` wrote: hooks/<id>/<hook>.log.
pub fn hook_log(&self, id: FlowId, hook: &str) -> PathBuf;

/// Deletes the log, the heartbeat file, what its waiter kept and hooks/<id>/.   (changed)
pub fn remove(&self, id: FlowId) -> Result<()>;
```

### `src/event.rs` (modified)

```rust
/// The repo's .saaio/setup ran for this flow: why it failed (its log's last line), None when it
/// passed; where its whole output is.
Setup { failed: Option<String>, log: String },
```

JSON line `{"v":1,"at":..,"kind":"setup","failed":"...","log":"..."}`, `failed` left out on a
pass; `clean` caps both texts.

### `src/view.rs` (modified)

```rust
pub struct FlowView {
    // ...
    /// The last setup run: None when the repo has no .saaio/setup or the flow predates this.
    #[serde(default)]
    pub setup: Option<SetupRun>,
}

pub struct SetupRun {
    pub failed: Option<String>,
    pub log: String,
    pub at: u64,
}

impl FlowView {
    /// "setup failed: <why> (full output: <log>)" while the last setup run failed.
    pub fn setup_problem(&self) -> Option<String>;
    /// The same for a list: the reason cut to LIST_TEXT_LIMIT unless `full`, the path whole.
    pub fn setup_line(&self, full: bool) -> Option<String>;
}
```

New `--json` key `setup`; no `VIEW_VERSION` bump (new keys are allowed, docs/json.md).

### `src/fold.rs` (modified)

- `Setup` failed: `view.setup` set; `stop_for(Blocked, setup_problem, ..)`.
- `Setup` passed: `view.setup` replaced. After a failure: timeline note `setup passed`, and a flow
  still blocked on that failure (blocker equals the old `setup_problem`) turns `Idle`. A pass with
  no failure before adds no note.
- Ignored once ended, like `Blocked`.

### `src/wake.rs` (modified)

```rust
pub enum Note {
    Master(String),
    BaseMerged { base: String, onto: String },
    /// The flow's setup failed for `reason`; its whole output is in `log`.
    SetupFailed { reason: String, log: String },
}

pub struct Mark {
    pub state: State,
    pub changed_at: u64,
    /// When the failed setup the master was told about failed.
    #[serde(default)]
    pub setup_failed: Option<u64>,
}
```

- `undelivered`: a failed `Setup` is a note, in log order.
- `for_worker` for `SetupFailed`: "news from saaio: .saaio/setup failed for this worktree:
  <reason>. Its whole output is in <log>. Read it. When the cause is in this repository, fix it and
  run `saaio setup`; when it needs something on this machine, such as a missing PHP extension, end
  your turn saying what is missing."
- `changed_flows`: a flow is news when its state needs the master and its mark changed (as today),
  or when its setup failed at a time the master has not seen, whatever its state.
- `for_master` `says`: a blocker other than the setup failure, else `setup_line(false)`, else the
  blocker or the last note.

### `src/cli/commands/setup.rs` (new)

```rust
#[derive(Debug, clap::Args)]
pub struct Args {
    /// Flow name or id [default: the flow whose worker runs it]
    name: Option<String>,
}

/// Runs the flow's teardown, then its setup, with its slot and environment.
pub fn run(args: &Args, ctx: &Context) -> Result<Document, Failure>;

/// Runs .saaio/setup for the flow into its setup log and appends a Setup event;
/// Setup::None and no event when the repo has none. Shared with start.
pub(super) fn run_setup(store: &Store, trust: &Trust, repo: &Path, id: FlowId, name: &str,
    space: &Workspace, now: u64) -> Result<Setup, Failure>;
```

### `src/cli/commands/start.rs` (modified)

The inline setup block becomes `setup::run_setup(..)`; no `Blocked` line of its own.

### `src/cli/commands/retire.rs` (modified)

```rust
/// Runs the flow's .saaio/teardown into its teardown log when trust covers the repo.
pub fn teardown(trust: &Trust, store: &Store, flow: &FlowView) -> Teardown;

/// Now calls teardown; gains the store.   (changed; callers remove.rs, clear.rs)
pub fn retire_worktree(trust: &Trust, store: &Store, flow: &FlowView,
    uncommitted: Uncommitted) -> (Retired, Teardown);
```

### `src/cli/output/setup.rs` (new)

Owns `Setup`, moved from `output/start.rs`.

```rust
pub(crate) enum Setup {
    Ran { log: String },
    None,
    Failed { reason: String, log: String },
}
/// ran, none, failed: <reason> (full output: <log>)
impl fmt::Display for Setup { ... }

/// flow, teardown, setup, log, help.
pub(crate) fn reran(flow: &FlowView, teardown: &Teardown, setup: &Setup) -> Document;

/// error "setup failed: <reason> (full output: <log>)", help: read the log, a failed teardown's
/// reason, run it again once fixed. (A Failure carries no keys beyond error, refusal and help.)
pub(crate) fn failed(flow: &FlowView, teardown: &Teardown, reason: &str, log: &str) -> Failure;

/// "<name> has no worktree to set up", "<name> has no .saaio/setup in <repo>", ...
pub(crate) fn refused(name: &str, why: &str) -> Failure;

/// "<repo>, the repository of <name>, is not trusted", help: `saaio trust` in the person's terminal.
pub(crate) fn untrusted(name: &str, repo: &Path) -> Failure;
```

```
$ saaio setup shop-240
flow: shop-240
teardown: ran
setup: ran
log: /Users/jan/.local/state/saaio/projects/shop/hooks/0b6e.../setup.log
help[1]: Run `saaio status shop-240` to see the flow

$ saaio setup shop-240          # exit 1
error: "setup failed: Please run composer update (full output: /Users/jan/.local/state/saaio/projects/shop/hooks/0b6e.../setup.log)"
help[2]:
  Read its whole output in the log
  Run `saaio setup shop-240` again once the cause is fixed
```

### `src/cli/output/status.rs` (modified)

- List: a failed setup is a `blockers` row, its reason cut to the list limit but the log path kept
  whole; the plain blocker row is left out when it is the same failure.
- One flow: a `setup` key with `setup_problem` (reason cut like the list), and no `blocker` key when
  the blocker is that same failure.

### `src/cli.rs`, `src/cli/commands.rs` (modified)

```rust
/// Run a flow's .saaio/teardown, then its .saaio/setup again
Setup(setup::Args),
```

### `src/server/pause_watch.rs` (modified)

`run_in_background` gains a `Store` clone and passes `store.hook_log(id, hook.name())`.

### UI (modified)

```rust
// ui/input.rs
pub enum Command { ..., Setup }                 // b's' after the prefix
// ui/app.rs
pub enum Confirmed { Remove(Removal), Trust(Box<NewFlowForm>), Setup(String) }
// ui/app/effect.rs
pub enum Job { Start(..), Remove(Removal), Setup(String), Overview }   // args: setup -- <name>
// ui/app/keys.rs
/// Asks y before re-running setup for the flow in view or the board's card; a notice for
/// the master, a plain session or a finished flow.
fn ask_setup(&mut self, flow: Option<FlowView>) -> Vec<Effect>;
/// "setting up <name>…" and Effect::Run(Job::Setup(name)).
pub(super) fn rerun_setup(&mut self, name: String) -> Vec<Effect>;
```

- `board_keys.rs`: `s` on this project's selected worker card -> `ask_setup`.
- `messages.rs` `on_ran`: Ok -> notice `<name> is set up`; Err -> notice with the error message.
- Help view and board hints list the key.
- `sidebar/rows.rs`: line 3 shows `setup failed: <reason>` in the problem color while setup failed
  and nothing more pressing (a question, a blocker, a turn) is shown.
- `board/detail.rs`: `fn setup(&self, flow: &FlowView) -> Option<String>`, the reason and the log
  path, in the header beside `services` and `turn`.

### `src/server/master.md` (modified), `master-v14.md` (new)

The current master.md is kept byte for byte as `master-v14.md` in `master.rs`'s earlier list. The
setup line becomes: a failed setup shows in `saaio status` with the path of its whole output; read
it, fix the cause (the `setup-saaio` skill for the script, the user for something on the machine),
then `saaio setup <name>`.

## Call chains

### a. `saaio start` with a failing setup

1. `start::run -> setup::run_setup(&store, &trust, &repo, id, &name, &space, now): Result<Setup, Failure>`
2. `run_setup -> Store::hook_log(id, "setup"): PathBuf`
3. `run_setup -> hooks::run(trust, repo, Hook::Setup, space, &hook_env(..), &log): Result<bool>`
4. `hooks::run_within -> private::create_fresh(log): io::Result<File>`, then
   `process::finish_into(command, file, log, program, limit): Result<()>`
5. `finish_into` waits; non-zero exit -> `Err(last_line_of(log))`; past the limit the group is
   killed and `Err("no answer within 10m")`, the partial output stays in the log.
6. `run_setup`: `Err` -> `Store::append(id, Setup { failed: Some(reason), log })`, returns
   `Setup::Failed`; `Ok(true)` -> append `Setup { failed: None, log }`, returns `Setup::Ran`;
   `Ok(false)` -> nothing, `Setup::None`.
7. `start::run -> append_workspace`, fold, `output::start::started(..)` prints
   `setup: failed: <reason> (full output: <log>)`.

Errors: a log that cannot be created fails the hook (`could not write <path>`), which becomes a
failed setup. An append that fails is a `Failure` as today.

### b. `saaio setup [name]` and the UI's `s`

1. UI: `Command::Setup` or board `s` -> `App::ask_setup(flow)` -> `View::Confirm { question: "Run
   teardown, then setup, for <name>?", then: Confirmed::Setup(name) }`; `y` ->
   `App::rerun_setup(name) -> Effect::Run(Job::Setup(name))` -> `saaio setup -- <name>`.
2. `setup::run -> ctx.open()`, `views(&store, now)`, the flow by `args.name` or `ctx.env.flow_id()`.
3. Refusals before anything runs (`output::setup::refused`, exit 1): no name outside a worker; no
   such flow; finished; plain session or no worktree; worktree gone; no `.saaio/setup`; repo not
   trusted (help: `saaio trust`).
4. `setup::run -> retire::teardown(&trust, &store, &flow): Teardown` (output in `teardown.log`; a
   failure is reported and the setup still runs).
5. `setup::run -> run_setup(..): Setup`
6. `Setup::Ran -> output::setup::reran(..)`; `Setup::Failed -> Err(output::setup::failed(..))`.
7. UI `App::on_ran(Job::Setup(name), result)`: notice `<name> is set up` or the error.
8. The server folds the new `Setup`; a pass clears the failure from status, sidebar and board.

### c. Who hears about a failure

1. Worker: `deliver -> wake::undelivered(events, delivered)` includes `Note::SetupFailed` ->
   `wake::for_worker` text above.
2. Master: `wake::changed_flows(flows, seen)` includes the flow once per failure
   (`Mark.setup_failed`); `for_master` says `setup_problem`.
3. Status: blockers row `shop-240,"setup failed: <reason> (full output: <log>)"` until a pass.
4. Person: sidebar line 3, board detail header, timeline.

## Test seams

- `private::create_fresh`: replaces a file and a symlink without following it; 0600; folder 0700.
- `process::finish_into`: interleaved stdout/stderr order, last line on failure, partial output on
  timeout, a left-behind process does not block.
- `hooks::run`: existing tests get a log path; a failing hook's whole output is in the log; no log
  for a missing hook or an untrusted repo.
- `Store::remove`: removes `hooks/<id>/`.
- `event`: round trip of `Setup` with and without `failed`.
- `fold`: a failure survives `Working`; a pass after it notes and unblocks; a pass alone adds no note.
- `wake`: note order with messages; master news once per failure, not on every state change; old
  `Mark` JSON reads.
- `output::status`, `output::setup`, `output::start`: TOON; long reason cut, path whole.
- UI `app/tests`: `C-space s` and board `s` ask, `y` runs `Job::Setup`, notices for master, plain
  and finished; `on_ran` notices; golden for the sidebar line.
- `tests/cli/setup.rs` (new subject in `tests/cli/main.rs`): scratch repo and store; failing start,
  log content, status row, `saaio setup` fixing it, default to `SAAIO_FLOW_ID`, refusals, teardown
  before setup.

## Assumptions

1. A passing setup at start also records a `Setup` event (with its log path), without a note.
2. A teardown failure during `saaio setup` is reported and the setup still runs.
3. `saaio setup` exits 1 when the setup fails; the UI shows the error as a notice.
4. It refuses finished flows, plain sessions and flows without a worktree or a `.saaio/setup`.
5. The worker keeps running while setup re-runs; nothing is paused or closed.
6. A flow still blocked on its failed setup turns `Idle` once a run passes.
7. Services marked paused stay marked paused after a re-run.
8. No lock against two re-runs of the same flow at once.
9. The error line is the last line of stdout and stderr merged, no longer stderr first.

## Decided in review

- A worker may run `saaio setup` on its own flow; without a name it takes `$SAAIO_FLOW_ID`'s flow.
- `saaio remove` writes `teardown.log` too.

## Tasks

### Output to a log

- [x] Add `private::create_fresh(path)` with unit tests for a file, a symlink, modes
- [x] Add `process::finish_into` and `last_line_of` with unit tests for order, failure line, timeout output, a left-behind process
- [x] Give `hooks::run`/`run_within` a `log: &Path` and `Store::hook_log`; `Store::remove` removes `hooks/<id>/`; pass logs from `retire::teardown` (new, `retire_worktree` gains the store), `pause_watch` and `start`; tests for the log content and removal

### The setup outcome

- [x] Add `EventKind::Setup` with JSON round trip and `clean` tests
- [x] Add `view::SetupRun`, `FlowView.setup`, `setup_problem`, and fold `Setup` (failure survives Working, pass notes and unblocks) with fold tests
- [x] Move `Setup` to `output/setup.rs` as `Ran{log}`/`None`/`Failed{reason, log}`; add `setup::run_setup` and use it in `start::run` instead of the Blocked line; start TOON tests
- [x] Status: setup blocker row (reason cut, path whole, no duplicate blocker row) and the one-flow `setup` key, with TOON tests

### Who hears it

- [x] `wake::Note::SetupFailed` in `undelivered` and `for_worker`; `Mark.setup_failed` in `changed_flows`, `mark` and `for_master`; wake tests including old Mark JSON

### `saaio setup`

- [x] Add `cli/commands/setup.rs` (`Args`, `run`, refusals, default to `SAAIO_FLOW_ID`), `output::setup::{reran, failed, refused}`, `Command::Setup` in `cli.rs`; `tests/cli/setup.rs` end to end

### UI

- [x] `Command::Setup` (`C-space s`), board `s`, `Confirmed::Setup`, `Job::Setup`, `ask_setup`, `rerun_setup`, `on_ran` arms, help view and hints, with app tests
- [ ] Sidebar line 3 and board detail header show a failed setup, with golden and unit tests

### Instructions and docs

- [ ] master.md setup line and `saaio setup`, current text kept as `master-v14.md`; master tests
- [ ] Docs: cli.md (`saaio setup`, start's `setup` field), repository.md (When setup fails), troubleshooting.md, files.md (`hooks/`), json.md (`setup`), ui.md (keys, sidebar), README command list if it lists commands; CHANGELOG Unreleased; CLAUDE.md plan list and architecture lines

### Check

- [ ] End to end in a scratch store and repo: a start whose setup fails like composer's (several lines, the cause not last), status and the worker note name the log with the whole output, `saaio setup` fails again, fix the script, `saaio setup` passes and the blocker clears; the UI's `s` in a PTY; `saaio --stop` at the end
