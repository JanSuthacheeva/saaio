# Pause idle services

Design page: https://claude.ai/artifact/MpEQzi8U8puwdDsTtEXFpC

## Decision

`saaio server` owns pausing and resuming a flow's services. A `PauseWatch` looks at the board
every tick, runs the repo's `.saaio/pause` or `.saaio/resume` on a background thread and records
each step as a `services` event in the flow's log. The worker's `PreToolUse` hook waits while the
services are pausing, paused or resuming, so a command never meets a half-stopped stack. saaio
stays harness- and Docker-agnostic: the repo's scripts say how.

Decided in review:

- The worker is not told about a failed resume. The tool runs; the board, status and the master
  see `resume-failed`.
- The sidebar stays as it is (paused services are about the project's stack, not where the flow
  stands). The board card and the board detail show them.
- The setup-saaio skill (agent-skills repo) is updated in this flow, as a separate PR there.
- The setting is `[services] pause_after_minutes = 5`, an integer; 0 never pauses.

## Units

### `src/pause.rs` (new): which job a flow needs, whether its tool calls wait (pure)

```rust
/// What PauseWatch runs for a flow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Job {
    Pause,
    Resume,
}

/// The job a flow needs at `now`; None for a plain session, a flow without a worktree, or with
/// `after` None ([services] off).
pub fn next(flow: &FlowView, now: u64, after: Option<Duration>) -> Option<Job>;

/// Idle, waiting, blocked, done and ended: a worker that runs nothing.
fn rests(state: State) -> bool;

/// Pausing, paused or resuming: a tool call waits.
pub fn holds_tools(flow: &FlowView) -> bool;
```

`next`:

| services | flow | next |
| --- | --- | --- |
| none, running | rests for `after` since `changed_at` | Pause |
| pause-failed | rests, `changed_at` after the failure's `since`, for `after` | Pause |
| pausing (no job runs: server restarted) | any | Pause |
| paused | working or delegated | Resume |
| resuming (no job runs) | any | Resume |
| resume-failed | working or delegated, `changed_at` after the failure | Resume |
| resume-failed | rests for `after`, `changed_at` after the failure | Pause |

Anything else: None. Not-responding never pauses or resumes. Plain sessions (`flow.is_plain()`)
and flows without a workspace: None.

### `src/server/pause_watch.rs` (new): runs the jobs, one per flow at a time, off the server thread

Mirrors `server/forge_watch.rs`.

```rust
/// The longest a pause or resume script may run before it is killed.
pub const HOOK_LIMIT: Duration = Duration::from_mins(5);

/// What a job's thread sends back.
#[derive(Debug)]
pub struct Finished {
    pub id: String,
    pub job: Job,
    /// Ok(false): the script was gone by the time it ran.
    pub result: Result<bool, String>,
}

/// The flows a job runs for.
#[derive(Debug, Default)]
pub struct PauseWatch {
    running: HashSet<String>,
}

impl PauseWatch {
    /// The flows and jobs to start now, with none running for them; marks them running.
    pub fn due(&mut self, flows: &[FlowView], now: u64, after: Option<Duration>) -> Vec<(FlowView, Job)>;
    /// The job for `id` ended.
    pub fn done(&mut self, id: &str);
}

impl Server {
    /// Each tick: starts the jobs due, for flows whose repo has both scripts; appends pausing or
    /// resuming before the thread starts.
    pub(super) fn pause_idle(&mut self);
    /// Appends paused, running, pause-failed or resume-failed; logs a failure.
    pub(super) fn pause_finished(&mut self, finished: Finished);
}

/// The script on a thread of its own; Event::Paused(Finished) when it ends.
fn run_in_background(flow: FlowView, repo: PathBuf, job: Job, events: Sender<Event>);
```

### `src/server.rs` (modified)

```rust
struct Server { ..., pauses: PauseWatch }
enum Event { ..., Paused(pause_watch::Finished) }
// serve(): self.pause_idle() after self.ask_forges()
// handle(): Event::Paused(finished) => self.pause_finished(finished)
```

### `src/workspace.rs` (modified)

```rust
pub enum Hook { Setup, Teardown, Pause, Resume }

/// Whether <repo>/.saaio/<hook> exists and is executable.
pub fn has_hook(repo: &Path, hook: Hook) -> bool;

/// run_hook, killed and failed past `limit` ("no answer within 5m").
pub fn run_hook_within(
    repo: &Path,
    hook: Hook,
    workspace: &Workspace,
    env: &[(String, String)],
    limit: Duration,
) -> Result<bool>;
```

`run_hook` keeps its signature. Both share a private `hook_file(repo, hook) -> PathBuf`;
`run_hook_within` shares the deadline loop with `output_in`.

### `src/cli/commands/hook.rs` (modified)

```rust
/// How long a tool call waits for services to come back.
const RESUME_WAIT: Duration = Duration::from_mins(2);
/// How long a tool call waits for the server to start a resume of paused services.
const PICKUP_WAIT: Duration = Duration::from_secs(2);
/// How often the wait reads the flow's log.
const WAIT_STEP: Duration = Duration::from_millis(200);

/// Before a tool call of a flow whose services pause::holds_tools: waits until they run again,
/// fail, or the wait is over; logs a wait that ran out.
fn wait_for_services(store: &Store, id: &str, log: &Log, clock: fn() -> u64);
```

`run` calls it for `Hooked::Flow` when `adapter.before_tool(&payload)`, after `hook(...)`. Paused
but not yet resuming: waits at most `PICKUP_WAIT`, so a store without a server never stalls tools.

### `src/adapter.rs`, `adapter/claude_code.rs`, `adapter/codex.rs` (modified)

```rust
impl Adapter {
    /// The payload is the hook before a tool call (PreToolUse in both harnesses).
    pub fn before_tool(self, payload: &Value) -> bool;
}
```

- `claude_code.rs`: PreToolUse's handler gets `TOOL_HOOK_TIMEOUT_SECS = 150`.
- `codex.rs`: the PreToolUse entry of `HOOKS` gets `TOOL_HOOK_TIMEOUT_SECS = 150`.

### `src/config.rs`, `examples/config.toml` (modified)

```rust
pub struct Config { ..., pub services: ServicesConfig }

/// [services]: when saaio pauses a resting flow's services.
#[serde(default, deny_unknown_fields)]
pub struct ServicesConfig {
    /// Minutes a flow rests before its services pause; 0 never pauses. 5 unless set.
    pub pause_after_minutes: u32,
}

impl ServicesConfig {
    /// The delay; None at 0.
    pub fn pause_after(&self) -> Option<Duration>;
}
```

```toml
#[services]
# Minutes a flow rests (idle, waiting, blocked, done or ended) before saaio runs the repo's
# .saaio/pause. Its .saaio/resume runs when the worker works again. 0 never pauses.
#pause_after_minutes = 5
```

### `src/fold.rs` (modified)

`Folding::apply` for `EventKind::Services`: sets `view.services`; a failure adds a timeline entry
(`pause failed: <reason>`, `resume failed: <reason>`, `Tone::Problem`). Never touches state,
blocker or `changed_at`. Applied after Ended too.

### `src/cli/output/status.rs` (modified)

- List: `FlowRow` and the `--all` row gain `services`: empty while running or never paused, else
  `paused`, `pausing`, `resuming`, `pause-failed`, `resume-failed`.
- Detail: `services: paused` and `services_error: <reason>` when failed.

```
flows[2]{name,state,phase,tasks,age,services}:
  shop-217,idle,review,9/9,12m,paused
  shop-230,working,implement,3/8,1m,
```

### UI (modified)

- `ui/board/cards.rs` `state_label`: `ready · paused`, `working · resuming`, `working · services
  down` (resume-failed), the way delegated appends `· agent`. Pausing reads as paused.
- `ui/board/detail.rs` header: `services paused 14:02`, first among the facts (a long request
  cut it off at the end, found in the E2E check); resume-failed with its reason.
- Sidebar: unchanged.

### Docs

- `docs/repository.md`: a "Pause and resume" section beside setup and teardown (when each runs,
  environment, the cases table), the Compose example gains both scripts; fix "A done, ended or
  removed flow releases its slot": a done flow keeps its slot until it ends (`slots.rs`).
- `docs/configuration.md`: `[services]`.
- `docs/flows.md`: what pausing does to a flow and when.
- `docs/json.md`: the `services` key. `docs/ui.md`, `docs/cli.md`: the card label and the status
  column.
- No CHANGELOG line (0.1.0 not released yet).

### setup-saaio skill (`~/projects/agent-skills/skills/setup-saaio/SKILL.md`, separate PR)

- Writes `.saaio/pause` and `.saaio/resume` whenever it writes a setup that starts something
  long-running.
- Compose: pause `docker compose stop`; resume `docker compose up -d --no-recreate --wait` (keeps
  the containers and ports setup made, returns when healthy).
- Dev servers: no pause or resume. The skill's setup never starts one (the worker runs it
  itself), so there is nothing of saaio's to stop. (Changed in the build: the background dev
  server case does not exist in the skill.)
- Both idempotent, quiet, one-line error on stderr when they fail.
- The scratch-worktree proof runs pause, resume, then teardown.
- Built on branch `setup-saaio/pause-resume` in `~/projects/agent-skills-pause-services`
  (commit 69bcfa3), with grader checks; pushed and opened as a PR on the user's OK.

## Data

### `event.rs`: the log line

```rust
EventKind::Services {
    status: ServiceStatus,
    /// The script's last error line, for pause-failed and resume-failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ServiceStatus { Pausing, Paused, Resuming, Running, PauseFailed, ResumeFailed }

impl ServiceStatus {
    /// paused, resume-failed, ...
    pub fn name(self) -> &'static str;
}
```

```
{"v":1,"at":1792489968000,"kind":"services","status":"paused"}
{"v":1,"at":1792490101000,"kind":"services","status":"resume-failed","reason":"port 8003 is taken"}
```

### `view.rs`: the `--json` contract

```rust
pub struct FlowView { ..., pub services: Option<Services> }

/// Where a flow's services stand since saaio first paused them.
#[serde(rename_all = "camelCase")]
pub struct Services {
    pub status: ServiceStatus,
    pub since: u64,
    pub reason: Option<String>,
}
```

A new key: no `VIEW_VERSION` bump. None until the first pause. The Board crosses the UI socket as
postcard, so `PROTOCOL_VERSION` goes up by one.

## Call chains

### a. A flow rests past the delay

1. `Server::serve -> Server::pause_idle()` every tick, after `ask_forges`.
2. `Server::pause_idle -> PauseWatch::due(&board.flows, now, config.services.pause_after()):
   Vec<(FlowView, Job)>`, each via `pause::next(flow, now, after): Option<Job>`, skipping flows
   with a job running.
3. `Server::pause_idle -> scripted_repo(&flow): Option<PathBuf>`: `worktree::repo_of(space)`, the
   worktree folder exists, and `workspace::has_hook(repo, Pause) && workspace::has_hook(repo,
   Resume)`. Either missing:
   `PauseWatch::done(id)`, nothing appended, nothing shown.
4. `Server::pause_idle -> Store::append(id, Services { status: Pausing })`, then
   `run_in_background(flow, repo, Job::Pause, events)`.
5. thread `-> workspace::run_hook_within(repo, Hook::Pause, space, &worktree::hook_env(..),
   HOOK_LIMIT): Result<bool>` `-> Event::Paused(Finished)`.
6. `Server::handle -> Server::pause_finished(finished)`: Ok(true) appends `Paused`; Ok(false)
   appends `Running`; then `PauseWatch::done(id)`.

Errors: the script exits non-zero or runs past `HOOK_LIMIT` -> `Err(last line)` ->
`pause_finished` appends `PauseFailed { reason }` and writes server.log. Fold adds a Problem
timeline entry; no retry until the flow's state changes again. An append that fails (flow removed
meanwhile) goes to server.log only.

### b. The worker works again

1. harness `-> saaio hook -> hook::hook -> Store::append(id, Working)`, as today.
2. Within a tick: `Server::pause_idle -> pause::next: Some(Resume)` `-> Store::append(Services {
   Resuming })` `-> run_in_background(.., Job::Resume, ..)`.
3. The first tool call: `saaio hook (PreToolUse) -> hook::run -> Adapter::before_tool(&payload):
   true -> wait_for_services(store, id, log, clock)`.
4. `wait_for_services -> Store::events(id) -> fold::fold(..) -> pause::holds_tools(&flow): bool`,
   every `WAIT_STEP`, until false or the wait is over.
5. thread `-> Finished { Ok(true) } -> pause_finished -> Store::append(Services { Running })`. The
   waiting hook sees running and returns; the tool runs.

Errors: resume fails -> `ResumeFailed { reason }`; the waiting hook returns, the tool runs against
stopped services. The card says `working · services down`, the detail the reason, status
`resume-failed`. Retried at the next turn. Resume slower than `RESUME_WAIT`: the hook gives up,
server.log says so, the tool runs; the script goes on until `HOOK_LIMIT`. Paused and no server to
resume: the hook waits `PICKUP_WAIT` and lets the tool run.

### c. A pause still runs when the worker starts again

1. Status is `pausing` and a job runs: `PauseWatch::due` skips the flow; tool calls already wait.
2. `Finished { Pause, Ok }` -> `Paused` -> next tick: `pause::next` sees paused and working ->
   `Resume`, as in b.

### d. Remove, clear and a server restart

1. `saaio remove / clear -> retire::retire_worktree -> run_hook(Teardown)`, unchanged: teardown
   runs on stopped services. The flow leaves the board, so `pause::next` is never asked again; a
   job still running for it appends to a gone log, which goes to server.log.
2. The server stops mid-job: the log still says `pausing` or `resuming`. The next server's first
   tick: `pause::next` returns the same job again (scripts are idempotent).

## Test seams

- `pause::next`, `holds_tools`: table tests over every status and state, inline.
- `fold`: services events set `view.services`, failures add Problem entries, nothing moves
  `changed_at` (`fold/tests.rs`).
- `workspace::has_hook`, `run_hook_within`: real scripts in a temp repo; env reaches pause; a
  hanging script is killed at the limit.
- `PauseWatch::due`: one job per flow, `done` frees it, unit.
- Server, over a real socket in `server/tests.rs`: a flow with events dated 10 minutes back and
  both scripts (they touch files) is paused, then a Working event gets it resumed; a repo without
  scripts gets no services event.
- `wait_for_services`: `tests/cli.rs`, a PreToolUse payload into `saaio hook claude-code` while
  the log says resuming; a thread appends running after 300 ms; the hook returns after it. Paused
  with no server: returns within `PICKUP_WAIT`.
- Config: the example test covers `[services]`; an unknown key fails.
- Status TOON and UI: the column, the detail lines and the card label in their existing tests and
  goldens.

## Assumptions

1. Rest states: idle, waiting, blocked, done, ended. Never working, delegated, not-responding, or a
   plain session.
2. The delay counts from the flow's last state change. Something the worker left running in the
   background gets only the delay as protection.
3. A repo needs both `.saaio/pause` and `.saaio/resume`; one alone counts as none.
4. The scripts get the setup's environment (`hook_env`), run in the worktree, may run 5 minutes,
   and must be idempotent.
5. A tool call waits up to 2 minutes for a resume; the PreToolUse hook timeout rises to 150 s in
   both adapters.
6. A failed pause or resume retries only after the flow's state changes again.
7. Timeline entries only for failures.
8. `pause_after_minutes` is read live like the rest of config.toml.

## Tasks

- [x] Add `ServiceStatus` and `EventKind::Services` (event.rs) and `Services` / `FlowView::services` (view.rs), fold them in `Folding::apply` with Problem entries for failures; bump `PROTOCOL_VERSION`; fold and log-line tests
- [x] Add `ServicesConfig` with `pause_after()` to config.rs and the `[services]` lines to examples/config.toml; config and example tests
- [x] Add `pause::next`, `rests` and `holds_tools` in src/pause.rs with table tests over every status and state
- [x] Add `Hook::Pause`/`Hook::Resume`, `workspace::has_hook` and `workspace::run_hook_within` (shared `hook_file`, deadline loop shared with `output_in`); tests for env, missing script and the limit
- [x] Add `server/pause_watch.rs` (`PauseWatch::due`/`done`, `Server::pause_idle`, `Server::pause_finished`, `run_in_background`, `Event::Paused`) and wire it into `Server::serve` and `handle`; unit tests and a server test that pauses and resumes over a real socket
- [x] Add `Adapter::before_tool`, the 150 s PreToolUse timeouts in both adapters, and `wait_for_services` in hook.rs; adapter tests and tests/cli.rs for waiting on resuming and not stalling without a server
- [x] Show services in `saaio status`: the list's `services` column (and `--all`), the detail's `services` and `services_error`; TOON tests
- [x] Show services on the board card label and in the board detail header; UI tests and goldens
- [x] Update docs/repository.md (pause and resume section, Compose example, the slot line fix), docs/configuration.md, docs/flows.md, docs/json.md, docs/ui.md, docs/cli.md
- [x] Update the setup-saaio skill in ~/projects/agent-skills to write and prove `.saaio/pause` and `.saaio/resume`, as a separate PR there
- [x] End-to-end check: scratch `SAAIO_HOME`, a repo whose pause and resume write timestamps and sleep, `pause_after_minutes = 1`, a Claude worker in the UI: it pauses after a minute idle, the card says `ready · paused`, a new prompt resumes it and the first tool call waits for it; status and `--json` agree
- [x] Count a tool call of a resting flow whose services are held as work: `pause::wakes_at_tool_call(flow)` (held, resting, not ended; a call that asks the user, `Adapter::asks`, does not wake) and `wait_for_services` appending `Working`, so the server resumes at once; pause table test and a tests/cli.rs test that the flow is working after the call
- [x] Add "a hook that runs too long is killed with everything it started (process group), for setup, teardown, pause and resume" to ROADMAP.md under Planned soon

## Review

Isolated review of `flow/pause-idle-services` against `main` and this plan. The data format, fold,
`pause::next`, the server wiring, the status columns, the UI labels and the docs match the plan.

### Fixed

- A pause or resume script that leaves a process holding its stdout or stderr (`sleep 30 &`, a
  dev server started without redirects) never finished: `finish_within` joined its pipe readers,
  so the flow stayed `resuming` and every tool call waited the full 2 minutes. A test took 61 s
  where it should take under a second. The output is now read for at most 500 ms after the script
  ends (15e82a2), and what a left-behind process writes afterwards is drained and dropped
  (734fc08). A fresh agent reviewed 15e82a2 and confirmed the test fails without it; 734fc08 came from its note and was reviewed only by me.
- docs/repository.md said setup-saaio writes pause and resume for dev servers too; the plan and the
  skill say services only (c7c1c27).
- `tests/cli.rs` `a_store_is_for_its_user_alone` was flaky: the server listens before it writes
  master.json, so the mode check could find no file (e8281a6).

### Open

- A Claude worker woken by the master (asyncRewake `saaio wait`) records no `working` until its
  first PostToolUse. PreToolUse of an ordinary tool appends nothing, so the flow still rests,
  the server does not resume, and that first tool call waits `PICKUP_WAIT` (2 s), then runs
  against stopped services. From then on the flow is working and later calls wait for the resume.
  Recommendation: treat a tool call of a resting flow with held services as work, e.g. append
  `Working` in `wait_for_services` when the flow rests. Codex is not affected: its Stop hand-over
  appends `Working`.
- At `HOOK_LIMIT` only the script's own process is killed. A `docker compose up` it started goes
  on. Recommendation: run the hook in its own process group and kill the group, also for setup and
  teardown, as a follow-up.

### Where to look

1. `src/pause.rs` `next`: the table of when a job runs, especially the failed cases
   (`changed_since`) and `Pausing`/`Resuming` rerunning after a server restart.
2. `src/server/pause_watch.rs` `pause_idle` to `pause_finished`: the `running` set and the
   append before the thread starts. The board is refolded before `pause_idle` in the same tick,
   because an append changes the log's size.
3. `src/cli/commands/hook.rs` `wait_for_services`: the 2 minute and 2 second limits, and that
   it runs only for a flow's PreToolUse.
4. `src/workspace.rs` `finish_within` and `Pipe`: the fix above.

Routine and skimmable: the event and view types, the config section, the status columns, the card
and detail labels, the golden, the adapter timeouts and the docs.

### Checks

`cargo test`, `cargo clippy --all-targets -- -D warnings` and `cargo fmt --check` pass at e8281a6.

### Decided

- Jan, 2026-10-20: fix the first tool call after a master wake in this branch (task above); the
  process-group kill goes to ROADMAP.md as a follow-up.
- Built: `wait_for_services` appends `Working` while `pause::wakes_at_tool_call` (55982f6), docs
  (ec7a06a), the roadmap row (9d5c01b). Also f2a5b20: both pipes of an ended hook share one
  500 ms grace, and the left-behind test kills its `sleep 30`s, which failed other tests under
  load.
