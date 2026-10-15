# The master and its workers talk to each other

Implementation page (review surface): `docs/design/talk-implementation.html`; approaches: `docs/design/talk-approaches.html`.

## Decision

- `flow send <flow> "<text>"` appends a `Message` event to the flow's log. Every Claude Code session flow starts,
  the master included, runs `flow wait` as an `asyncRewake` hook on `SessionStart` and `Stop`: it sleeps until
  there is news for its session, prints it to stderr and exits 2, and Claude Code wakes the session with it
  (verified on 2.1.300 for both events, also on a fresh session with no prompt).
- Worker news: the master's messages not yet delivered. Master news: flows of its store that became waiting,
  blocked, idle, done or not responding since it last saw them.
- Who answers is decided by the flow's mode: `auto` -> the master answers what the code, plan or the user's
  earlier decisions settle and tells the user; `guided` -> the master tells the user and relays the answer.
- A Stop also records the turn's whole closing message (`Answer`), read with `flow status <name> --full`.
- Every idle wakes the master. `[master] wake = false` in config.toml switches the master's waiter off.

## Units

```rust
// src/event.rs
pub enum EventKind { ..,
    /// What the master told this flow's worker, through `flow send`.
    Message { text: String },
    /// A turn's whole closing message, for the master to read; Waiting/Idle keep its short form.
    Answer { text: String },
}

// event.rs: pub const LONG_TEXT_LIMIT: usize = 4000; Event::new cuts Message and Answer to it
// (in place of a separate ANSWER_LIMIT and MESSAGE_LIMIT).

// src/view.rs
pub struct FlowView { .., pub answer: Option<String> }   // additive, VIEW_VERSION stays 1
// src/fold.rs: Message -> timeline "master: {text}" (Tone::Neutral), state unchanged;
// Answer -> view.answer; changes(): true for Message, Answer when it differs from view.answer.
// An Idle with a new note sets changed_at (a woken turn may end idle again without working first).

// src/wake.rs (new, pure)
pub type Seen = BTreeMap<String, Mark>;  // flow id -> how the master last saw it
pub struct Mark { pub state: State, pub changed_at: u64 }  // a flow gone silent is news again
pub fn undelivered(events: &[Event], delivered: usize) -> Vec<&str>;
pub fn changed_flows<'a>(flows: &'a [FlowView], seen: &Seen) -> Vec<&'a FlowView>;  // oldest change first
pub fn mark_seen(seen: &mut Seen, flows: &[&FlowView]);
pub fn first_seen(flows: &[FlowView]) -> Seen;   // every finished flow

// src/store.rs, files under <store>/waits/
pub enum Waiter { Master, Flow(String) }
impl Store {
    pub fn claim_waiter(&self, waiter: &Waiter, pid: u32) -> Result<()>;   // waits/<key>.pid
    pub fn waiter(&self, waiter: &Waiter) -> Option<u32>;
    pub fn delivered(&self, id: &str) -> usize;                            // waits/<id>.delivered
    pub fn set_delivered(&self, id: &str, count: usize) -> Result<()>;
    pub fn seen(&self) -> Option<Seen>;                                    // waits/master.seen (JSON)
    pub fn set_seen(&self, seen: &Seen) -> Result<()>;
}
// remove(id) also deletes waits/<id>.pid and waits/<id>.delivered.

// src/adapter/mod.rs
impl Adapter { pub fn master_wiring(self, flow_exe: &Path) -> Vec<String>; }

// src/adapter/claude_code.rs
const WAIT_TIMEOUT_SECS: u64 = 604_800;
/// As today, plus on SessionStart and Stop:
/// {"type":"command","command":flow_exe,"args":["wait"],"asyncRewake":true,"timeout":WAIT_TIMEOUT_SECS}
pub fn settings(flow_exe: &Path) -> String;
/// Only the waiter, args ["wait","--master"], on SessionStart and Stop.
pub fn master_settings(flow_exe: &Path) -> String;
// translate, Stop: [Answer { text }, turn_end(..)] when the message is not empty; every Stop counts,
// also with stop_hook_active (a turn flow woke ends with it set, verified on 2.1.300).
pub const WORKER_PROMPT: &str = "...";   // who the flow master is; Adapter::wiring appends it with
                                        // --append-system-prompt (found in the E2E check: without it a
                                        // worker rightly would not act on hook feedback)

// src/config.rs
pub struct Config { .., #[serde(default)] pub master: MasterConfig }
pub struct MasterConfig { #[serde(default = "...true")] pub wake: bool }   // [master] wake = false

// src/server/master.rs
pub fn spec(&self, fresh: bool, agent: &Agent, env: &LaunchEnv, instructions: &Path, wake: bool)
    -> Option<PaneSpec>;   // inserts adapter.master_wiring(&env.flow_exe) after the program when wake
const PREVIOUS_DEFAULTS: [&str; 2] = [include_str!("master-v1.md"), include_str!("master-v2.md")];
// master.md: flow wakes you with the flows that changed; auto: answer what the code, plan or earlier
// decisions settle with flow send and tell the user; guided: tell the user, send their answer;
// flow status <name> --full for the whole answer, git -C <worktree> for the code; send nothing unasked.

// src/main.rs
enum Command { ..,
    /// Tell a flow's worker something: an answer, an instruction, a correction
    Send { flow: String, text: String },
    #[command(hide = true)]
    Wait { #[arg(long)] master: bool },
}
const WAIT_EVERY: Duration = Duration::from_millis(500);
const SETTLE: Duration = Duration::from_secs(2);
fn send(store: &Store, flow: &str, text: &str, now: u64) -> Result<Document, Failure>;
fn wait(store: &Store, waiter: &Waiter, clock: fn() -> u64) -> Option<String>;
fn worker_news(store: &Store, id: &str, now: u64) -> Option<Option<String>>;  // None: finished or gone
fn master_news(store: &Store, clock: fn() -> u64, is_mine: &dyn Fn() -> bool) -> Option<String>;
fn run_wait(master: bool) -> ExitCode;
// main(): Command::Wait handled before the stdout path, like Hook: Some(news) -> eprint, exit 2;
// None -> exit 0; never stdout.

// src/output.rs
pub fn sent(flow: &FlowView) -> Document;
pub fn news_for_worker(messages: &[&str]) -> String;   // "message from the flow master:" + each
pub fn news_for_master(flows: &[&FlowView]) -> String; // changed[n]{name,state,mode,says} + help per mode
// status_detail: adds answer (cut unless --full).
```

## Call chains

1. Send:
   1. `master -> main::send(store, flow, text, now)`.
   2. `views -> fold::resolve`.
   3. `Store::append(id, Message{text})`.
   4. `output::sent`.
   - Errors, all exit 1: no flow -> "no flow named X"; a finished flow; no workspace -> "has no worker"; `$FLOW_ID` set -> "only the master sends"; empty -> "nothing to send"; over LONG_TEXT_LIMIT.
2. Worker delivery:
   1. `wait loop -> Store::events(id) -> wake::undelivered(events, Store::delivered(id))`.
   2. Non-empty and `Store::waiter == own pid` -> `Store::set_delivered(id, n)`.
   3. `output::news_for_worker` -> eprint, exit 2 -> Claude Code wakes the worker.
   - A session that is not running gets its messages from its SessionStart waiter.
3. Master wake:
   1. `worker Stop -> main::hook -> translate: [Answer, Waiting|Idle] -> append`.
   2. `master's wait loop -> views -> wake::changed_flows(flows, seen)`. The seen marks come from `Store::seen()`, or `wake::first_seen` when there are none yet.
   3. On a non-empty result: sleep SETTLE, then views and changed_flows again.
   4. If `Store::waiter(Master) == own pid`: `mark_seen -> Store::set_seen`.
   5. `output::news_for_master` -> eprint, exit 2.
   6. The master acts per mode (master.md).
   - A failed write -> exit 0 silently.
4. Arming:
   1. SessionStart or Stop -> Claude Code spawns `flow wait [--master]` in the background.
   2. The waiter comes from `--master` or `$FLOW_ID`; with neither -> exit 0.
   3. `Store::claim_waiter(waiter, pid)`.
   4. The loop every WAIT_EVERY returns None when:
      - another waiter claimed the session;
      - the parent pid changed;
      - a worker's flow is removed or finished.
   - A waiter armed earlier fires mid-turn too, so a correction reaches a busy worker; verify this in the E2E check.

## Test seams

- wake.rs: unit tests (undelivered, which states count, settle-free changed_flows, first_seen).
- store.rs:
  - claim, replace and read a waiter;
  - delivered and seen round trips;
  - remove cleans `waits/`.
- fold.rs:
  - Message adds an entry and leaves the state;
  - Answer sets `answer`;
  - `changes` for both.
- claude_code.rs:
  - the waiter is on SessionStart and Stop;
  - master_settings has the waiter only;
  - Stop yields Answer, then Waiting or Idle.
- master.rs:
  - wires `wait --master` when wake is on, for fresh and resumed sessions, and leaves it out when wake is off;
  - the v2 default is replaced.
- config.rs: `[master] wake` parses, defaulting to true.
- tests/cli.rs:
  - send's refusals and its event;
  - `flow wait` as a process exits 2 with a message, or exits 0 when replaced;
  - the master waiter sees a waiting flow.

## Assumptions

1. Only the master sends; a worker asks by ending its turn with the question. There is no worker-to-master or worker-to-worker message.
2. The master is woken for waiting, blocked, idle, done and not responding (every idle, decided in review).
3. Changes within 2 s arrive as one wake.
4. A message does not change the flow's state.
5. The worker sees "message from the flow master:" as hook feedback, not as the user's words.
6. The diff stays git's job (`git -C <worktree>`).
7. The master's waiter watches its own store only.
8. Agents without a Claude Code adapter get no waiter; their messages wait in the log.

## Tasks

- [x] event.rs Message and Answer; fold.rs/view.rs: timeline entry, FlowView.answer, changes(); tests
- [x] wake.rs: Seen, undelivered, changed_flows, mark_seen, first_seen with unit tests
- [x] store.rs: Waiter, claim_waiter/waiter, delivered/set_delivered, seen/set_seen, remove cleans waits/; tests
- [x] output.rs: sent, news_for_worker, news_for_master, answer in status_detail; tests
- [x] main.rs: flow send with its refusals; tests/cli.rs cases
- [x] main.rs: flow wait (loop, replace, settle, exit 2 on stderr), drop the dead_code allowances; tests/cli.rs process tests
- [x] claude_code.rs: waiter hooks in settings, master_settings, Answer on Stop; Adapter::master_wiring; tests
- [x] config.rs MasterConfig; master.rs spec(.., wake) wiring; server passes config.master.wake; tests
- [x] master.md v3 (master-v2.md kept as a previous default); README, CLAUDE.md, HANDOVER
- [x] End-to-end check: flow ui with an auto and a guided worker; each wakes the master; the master answers the auto one with flow send and relays for the guided one; a mid-turn send reaches a busy worker
