# Plan: keep the whole flow start request

Review page: https://claude.ai/artifact/8rVLJsCBNeB3KTpuyAhrJ8

## Decision

`Event::new` cut a Started event's request to `TEXT_LIMIT` (500 chars), and the worker prompt is built
from the folded (cut) request, so long requests lost their tail. The log and the prompt now keep the whole
request; only displays cut it (they already do). `flow start` and `flow new` refuse a request over
`REQUEST_LIMIT` = 20,000 chars before anything is created, the way `flow send` refuses a long message.
20,000 chars is at most 80 KB of UTF-8, under Linux's 128 KiB limit on one argv string (the prompt
reaches the agent as one). `--name` is out of scope: still cut to `TEXT_LIMIT`.

## Units

`src/event.rs` (modified)

```rust
/// The longest request flow start and flow new take; past it they refuse. Kept whole, since it
/// becomes the worker's prompt, one argv string (Linux refuses one over 128 KiB).
pub const REQUEST_LIMIT: usize = 20_000;

impl EventKind {
    // Started: cuts name to TEXT_LIMIT only; request is left whole.
    fn cut_texts(&mut self);
}

impl Event {
    // Doc: each text cut to its limit, except a Started request, which its command checks.
    pub fn new(at: u64, kind: EventKind) -> Event;
}
```

`src/cli/commands.rs` (modified), beside `views` and `no_flow`

```rust
/// Refuses a request over REQUEST_LIMIT chars, so it is never cut on the way to the worker.
fn check_request(request: &str) -> Result<(), Failure>;
```

Failure (exit 1): message `the request is {length} chars, over {REQUEST_LIMIT}`, help
`Put the details in a file in the repo and name it in the request`.

`src/cli/commands/start.rs` (modified): `run` calls `check_request(&args.request)?` first, before the
agent lookup. Signature unchanged.

`src/cli/commands/new.rs` (modified): `run` calls `check_request(&args.request)?` first, before looking
for an existing flow. Signature unchanged.

## Data

No new types. Log format unchanged (`EVENT_VERSION` 1); `VIEW_VERSION` unchanged (`request` is the same
field, now whole). Old logs stay cut; nothing to migrate.

## Call chains

flow start "<request>":

1. `main -> cli::run -> commands::start::run(args, ...)`
2. `start::run -> commands::check_request(&args.request): Result<(), Failure>`
3. Over the limit: `Err(Failure)` returned, printed as TOON, exit 1; no flow, slot, worktree or branch.
   In flow ui the start job fails and the app shows the error line.
4. `start::run -> Event::new(now, Started { name, request, mode }): Event` (request whole)
5. `start::run -> Store::create(&first): id`, then slot, worktree, setup, Workspace event, as today
6. `flow server -> fold -> FlowView.request -> pane::worker_spec -> agent_argv(agent, id, name, &flow.request)`:
   the prompt carries the whole request

flow new --name x --request "<request>":

1. `commands::new::run -> check_request(&args.request)`; over the limit: same error, exit 1, no log
2. `new::run -> Event::new(..Started..) -> Store::create`, as today

## Test seams

- `tests/cli.rs` repro (first, fails before the fix): agent command
  `sh -c 'printf %s "$1" > <file>; exec cat' sh {prompt}`, prompt `{request}`; under a running
  `flow server`, start a 600-char request ending in a marker; the file holds the whole request and
  `flow status --json` shows it whole.
- `tests/cli.rs`: 20,001 chars refused by `flow start` (exit 1, "over 20000", no flow, no `flow/*`
  branch) and by `flow new`; exactly 20,000 is taken.
- `event.rs` unit test: name cut to `TEXT_LIMIT`, request kept whole.
- `cli/output/status.rs`: a long request is cut with the `--full` hint.

## Assumptions

- Chars are Unicode scalar values (`chars().count()`), as `flow send` counts.
- The check runs on the request as given, not trimmed.
- The UI's new-flow form gets no limit of its own; a too-long request shows start's error line.
- Old logs keep their cut requests.
- `Event::new` stays infallible; the check lives in the commands that take input.

## Tasks

Build note: tasks 1 and 2 are one commit, so no commit holds a failing test; `REQUEST_LIMIT` came
with task 3, its first user (clippy refuses dead code).


- [x] Add the CLI repro test in `tests/cli.rs`: a 600-char request reaches the worker's prompt and `flow status --json` whole (fails today)
- [x] Stop cutting the Started request in `EventKind::cut_texts`, update the `event.rs` unit test; the repro passes
- [x] Add `REQUEST_LIMIT` and `commands::check_request` and call it first in `start::run` and `new::run`, with CLI tests: 20,001 chars refused by both (no flow, no branch), 20,000 taken
- [ ] Add a `status.rs` case: a long request is cut with the `--full` hint
- [ ] End-to-end: `cargo install --path .`, start a long request in a scratch repo, check the worker's prompt and `flow status` output; check a 20,001-char start is refused
