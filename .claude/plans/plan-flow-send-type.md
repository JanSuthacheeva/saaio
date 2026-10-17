# Plan: flow send --type types a line into a worker's pane

Implementation page: https://claude.ai/artifact/5Ey1jNmQopSk24esVvaMkk (approaches: https://claude.ai/artifact/Mi8Q7tnfyukhTgW9JXmzqM, approach A). Copies in `docs/design/send-type-*.html`.

## Decision

`flow send --type <flow> "<line>"` appends a `Typed { text }` event to the flow's log: the log is
the queue. `waits/<id>.typed` counts the lines the server typed. A new `Server::type_lines`
(`server/typing.rs`) types the next line as one bracketed paste and `\r` into the worker's pane
once the session is idle, the pane is quiet and the line before it ended its turn. Both
harnesses mark turns in `waits/<key>.busy`, so idle means the same for Claude Code and Codex.

Decided with the user: approach A (log holds the queue, survives a server restart, no protocol
change); one line per call, calls queue in order; event name `typed`, timeline
`master typed: <text>`; the answer `queued`, `line`, `ahead`.

Verified 2026-10-17 (Claude Code 2.1.302, Codex 0.163.0), hooks logged while lines were typed:

| Typed | Claude Code | Codex |
| --- | --- | --- |
| a prompt | UserPromptSubmit, Stop | UserPromptSubmit, Stop |
| `/compact` | PreCompact, SessionStart compact, PostCompact, no Stop | PreCompact, PostCompact, no Stop (SessionStart compact at the next prompt) |
| a skill `/commit` | UserPromptSubmit, UserPromptExpansion, ..., Stop | - |
| `/clear` | SessionEnd (reason clear), SessionStart clear | - |
| `/cost` | no hook, opens a dialog that takes the next keys | - |

Both run `/compact` from a bracketed paste and `\r`.

## Units

`src/cli/commands/send.rs` (modified):

```rust
pub struct Args {
    /// Flow name or id
    flow: String,
    /// What to tell it, or with --type the line to type
    text: String,
    /// Type the text and Enter into the worker's pane once it is idle, as if the user typed it
    #[arg(long = "type")]
    typed: bool,
}

pub fn run(args: &Args, store: &Store, config: &Config, sender: Option<&str>, now: u64)
    -> Result<Document, Failure>;
```

`cli.rs` passes the config. Existing refusals apply to both. With `--type` also: a newline in the
line, and a flow whose agent has no adapter.

`src/cli/output/flow.rs` (modified):

```rust
/// queued, the line, how many lines wait before it, and when flow types it.
pub(crate) fn queued(flow: &FlowView, line: &str, ahead: usize) -> Document;
```

```
queued: shop-217
line: /compact
ahead: 0
help[2]: flow types it into shop-217's pane once its session is idle,Queue the next line with another `flow send --type shop-217 "<line>"`; it waits for this one's turn to end
```

`src/event.rs` (modified):

```rust
/// A line the master queued for the server to type into this flow's pane, through `flow send --type`.
Typed { text: String },
```

Cut to `LONG_TEXT_LIMIT` like `Message`. On disk `{"kind":"typed","text":"/compact"}`.

`src/fold.rs` (modified): `changes` is true for every `Typed`; the timeline notes
`master typed: <text>`, neutral. No `FlowView` field, `VIEW_VERSION` stays.

`src/wake.rs` (modified):

```rust
/// The lines queued in a flow's log after the first `typed` of them.
pub fn untyped(events: &[Event], typed: usize) -> Vec<&str>;
```

`src/store.rs` (modified):

```rust
/// Whether a session runs a turn, as `waits/<key>.busy` says.
pub enum Busy {
    /// At its prompt: no busy file.
    Idle,
    /// A hook or `flow wait` began a turn ("turn", or an empty file an earlier flow wrote).
    Turn,
    /// The server typed into the pane and no turn began yet ("typed").
    Typed,
}

pub fn set_busy(&self, waiter: &Waiter, busy: Busy) -> Result<()>; // was busy: bool
pub fn busy(&self, waiter: &Waiter) -> Busy;                         // was bool

/// How many of the flow's queued lines the server typed.
pub fn typed(&self, id: &str) -> usize;
pub fn set_typed(&self, id: &str, count: usize) -> Result<()>;
```

`remove` also removes `waits/<id>.typed`.

`src/adapter.rs`, `src/adapter/claude_code.rs`, `src/adapter/codex.rs` (modified):

```rust
// adapter.rs
pub fn turn(self, payload: &Value) -> Option<Turn>; // ClaudeCode => claude_code::turn

// Turn::Interrupted is renamed Turn::Idle: at its prompt with no Stop (an Esc, the end of a
// compaction), its hook hands nothing over
// claude_code.rs: EVENTS gains PreCompact, PostCompact
/// A prompt or a compaction begins a turn (Began), a Stop ends it (Ended), the compaction's end
/// leaves it at its prompt (Idle); a Stop with a background task running is no end (the agent
/// resumes on its own); a subagent's are not the session's.
pub fn turn(payload: &Value) -> Option<Turn>;

// codex.rs: EVENTS gains PreCompact, PostCompact; turn maps them to Began, Idle
```

Changed in the build: PostCompact is `Turn::Idle`, not `Ended`. Codex runs no Stop after a
compaction, and news handed over (exit 2) at PostCompact would be marked delivered with nothing
to wake the session with it.

`translate` gives the new events no kinds.

`src/cli/commands/hook.rs` (modified): `Turn::Began` marks `Busy::Turn`. At `Turn::Ended` a
session that wakes itself (Claude Code) is only marked `Busy::Idle`; Codex keeps `hand_over`.

`src/cli/commands/wait.rs` (modified): before it returns news, marks the session `Busy::Turn`.

`src/server/typing.rs` (new):

```rust
/// How long a typed line may go without a turn beginning before the next one may follow:
/// a command such as /cost runs no hook.
#[cfg(not(test))]
const UNSTARTED: Duration = Duration::from_secs(30);
#[cfg(test)]
const UNSTARTED: Duration = Duration::from_millis(300);

impl Server {
    /// Types each flow's next queued line into its pane when the pane is ready, and lets a line
    /// that began no turn within UNSTARTED go.
    pub(super) fn type_lines(&mut self);

    /// Pastes `text` and Enter into the pane, marks the session Busy::Typed and when; news and
    /// queued lines both type through it.
    pub(super) fn type_into(&mut self, key: &PaneKey, waiter: &Waiter, text: &str) -> Result<()>;

    /// The flow whose queued lines this pane takes: a running worker of a flow that is not plain
    /// or finished, whose agent has an adapter and whose session ended a turn (answer set).
    fn takes_lines(&self, key: &PaneKey) -> Option<String>;
}

/// True when the pane takes a line now: idle, open and untouched by the person for QUIET.
fn ready(busy: Busy, opened_at: Option<Instant>, typed_at: Option<Instant>, now: Instant) -> bool;

/// True when the server's own typed line began no turn within UNSTARTED.
fn unstarted(busy: Busy, line_at: Option<Instant>, now: Instant) -> bool;
```

`src/server/news.rs` (modified): `deliver_news` checks `busy == Busy::Idle` and types through
`type_into`. `QUIET` and `paste` become `pub(super)`.

`src/server.rs`, `src/server/panes.rs` (modified):

```rust
// Server, new fields
/// When each pane opened, so nothing is typed into a harness still starting up.
opened_at: HashMap<PaneKey, Instant>,
/// When the server last typed into each pane, for UNSTARTED.
line_at: HashMap<PaneKey, Instant>,
```

`serve` calls `type_lines` after `deliver_news`. `open_pane` marks `Busy::Idle` and sets `opened_at`.

`src/server/master.md`, `src/server/master.rs` (modified): today's master.md becomes
`master-v8.md`, listed in `master.rs`. New bullet:

```
- `flow send --type <name> "<line>"` types the line and Enter into the worker's pane once it is
  idle, as if the user typed it there, so a slash command runs: `/compact`, or a skill. One line
  per call, typed in order, each after the turn before it ended. When the user asks to compact a
  worker and go on, queue `/compact`, then the line they name, such as "continue shipping". Type
  only what the user asks for.
```

Docs (plain reference style, no semicolons, no first person): `docs/cli.md` (flow send,
commands at a glance), `docs/files.md` (`waits/`: `.typed`, `.busy` for both harnesses),
`docs/flows.md` where it describes `flow send`, `README.md` if it lists send, `docs/HANDOVER.md`
(verified facts above, the decision).

## Data

| What | Shape | Crosses |
| --- | --- | --- |
| `EventKind::Typed` | `{"at":..,"kind":"typed","text":"/compact"}` | send -> log -> server, fold |
| `waits/<id>.typed` | a count, like `.delivered` | server only |
| `waits/<key>.busy` | absent (Idle), `turn` or empty (Turn), `typed` (Typed) | hook, wait -> server |

## Call chains

The master queues a line:

1. `cli::run -> send::run(args, store, config, env.flow_id(), now): Result<Document, Failure>`
2. `send::run -> views(store, now)`, `fold::resolve(&flows, &args.flow): Option<&FlowView>`
3. Refusals in order: sender set, empty, too long, a newline (with `--type`), no flow, finished,
   no worker, plain, agent without adapter (`config.agent(Some(&workspace.agent))`, with `--type`).
   Newline: "a typed line is one line", help "Queue one `flow send --type` per line". No adapter:
   "<name> runs <agent>, which reports no turns to flow", help "Use `flow send` for a message".
4. `send::run -> store.events(id)`, `wake::untyped(&events, store.typed(id)).len()` (ahead)
5. `send::run -> store.append(id, Event::new(now, Typed { text }))`
6. `send::run -> output::flow::queued(flow, text, ahead): Document`

The server types the next line:

1. `Server::serve -> Server::type_lines()` every `WATCH_EVERY`, after `deliver_news`
2. For each running pane: `takes_lines(&key): Option<String>`
3. `store.busy(&waiter)`. If `unstarted(busy, line_at, now)`: `store.set_busy(&waiter, Busy::Idle)`,
   log "no turn began after typing into <name>", go on as idle.
4. `ready(busy, opened_at, typed_at, now)` false: skip.
5. `store.events(id)`, `store.typed(id)`, `wake::untyped(&events, typed).first()`: none, skip.
6. `type_into(&key, &waiter, line)`: `host.write(&key, &paste(line))`,
   `store.set_busy(&waiter, Busy::Typed)`, `line_at.insert(key, now)`
7. `store.set_typed(id, typed + 1)`

Errors: a failed write is logged, the count not raised, the line tried again next tick. A failed
`set_typed` is logged; the line could be typed twice, like a failed `set_delivered`.

The worker runs it and comes back idle:

1. `hook -> adapter.turn(payload): Some(Turn::Began) -> mark_busy(store, &waiter, Busy::Turn)`
2. `hook -> adapter.turn(payload): Some(Turn::Ended)`: `adapter.wakes_itself()` ->
   `mark_busy(.., Busy::Idle)`; else `hand_over` as today, Idle when there is no news.
3. The next `type_lines` tick sees Idle and types the next line.

Esc on Claude Code runs no hook: Turn stays until the next prompt's turn ends. A line that starts
nothing: Typed for UNSTARTED, then Idle.

flow wait wakes a Claude Code worker:

1. `wait::wait -> deliver::worker_news(store, id, now): WorkerNews::News`
2. `wait::wait -> store.set_busy(&waiter, Busy::Turn)`, then returns the news (exit 2)
3. That turn's Stop marks Idle.

## Test seams

- `wake::untyped`, `Store` Busy and typed counts, `fold` note and `changes`: unit tests.
- `claude_code::turn`, `codex::turn`: unit tests on recorded PreCompact, PostCompact and
  Stop-with-background fixtures.
- `hook`: a Claude Code Stop marks Idle and hands nothing over. `wait`: marks Turn with news.
- `typing::ready`, `typing::unstarted`: pure unit tests like `news::quiet`.
- `send --type`: `tests/cli.rs` end to end, the refusals and `ahead`.
- Server: `server/tests.rs` over a real socket with a fake agent that runs `flow hook`: two
  queued lines, the second typed only after the first's turn ends.
- E2E with a scratch `FLOW_HOME`: a Claude Code and a Codex worker, `/compact` then a line.

## Assumptions

- The worker sees a typed line as the user's prompt, unmarked.
- A line is typed only after the session ended a turn once (its flow has an answer) and its pane
  has been open for QUIET.
- A line that begins no turn holds the queue UNSTARTED (30 s).
- Queued lines wait while no server runs.
- No way to drop a queued line; removing the flow drops them.
- No typing into the master's pane.
- CHANGELOG untouched (0.1.0 not released); ROADMAP has no item for it.
- Compaction changes no flow state.
- Codex asks once to review flow's hooks again (two new hooks).

## Tasks

- [x] Add `EventKind::Typed`, its cut, the fold note `master typed: <text>` and `fold::changes` for it, with unit tests
- [x] Add `wake::untyped` and `Store::typed` (removed with the flow), with unit tests; built with `flow send --type`, its first caller, and `set_typed` with the server, its only caller (dead code fails clippy)
- [x] Replace `Store::set_busy`/`busy` bool with `Busy { Idle, Turn, Typed }` and update hook.rs, news.rs, panes.rs, with store unit tests including an empty file reading Turn
- [x] Add `claude_code::turn` and PreCompact/PostCompact to both adapters' EVENTS and `codex::turn`, with recorded fixtures and unit tests
- [x] `flow hook`: Claude Code turns mark Busy only, never hand over; `flow wait` marks Busy::Turn with news; tests
- [x] `flow send --type` with its refusals and `output::flow::queued`, with output unit tests and `tests/cli.rs` cases
- [ ] `server/typing.rs`: `Store::set_typed`, `type_lines`, `type_into`, `takes_lines`, `ready`, `unstarted`, the `opened_at`/`line_at` fields, `deliver_news` through `type_into`; unit tests and a `server/tests.rs` case with two queued lines
- [ ] master.md bullet with `master-v8.md` kept and listed in master.rs; test that the old default upgrades
- [ ] Docs: `docs/cli.md`, `docs/files.md`, `docs/flows.md`, README if needed, HANDOVER verified facts and decision
- [ ] E2E with a scratch FLOW_HOME: a Claude Code worker and a Codex worker get `/compact` then a line, each typed when idle, in order
