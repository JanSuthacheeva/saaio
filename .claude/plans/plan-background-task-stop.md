# A Stop always ends the turn

Implementation page: https://claude.ai/artifact/4DWtYgP4aikSJ3qX9viSkb
(approaches: https://claude.ai/artifact/DHNguH3E9rPXF6YqiHbxty)

## Decision

A Claude Code worker that ended its turn while a background task ran was recorded as
`delegated <task>`: no answer, no waiting or idle, the master never woken, the session left busy.
A Claude Code master in the same spot stayed `delegated` in `master.turn`. Approach A: a
main-agent Stop records the answer and `turn_end(answer)` whatever runs in the background, and
`turn` is `Ended` for it, as the Codex adapter already does. Background tasks are read nowhere;
no note carries them, and a background subagent is not delegated. Codex and `fold::master_state`
need no change.

## Units

`src/adapter/claude_code.rs` (modified)

```rust
/// The flow events one Claude Code hook payload means. ...
pub fn translate(payload: &Value, workflow: &Workflow) -> Vec<EventKind>;
//   "Stop": [Answer { text }] when the message has text, then turn_end(answer).
//   The background_tasks check at the top of the arm goes.

/// A prompt or a compaction begins a turn, a Stop ends it, the compaction's end leaves it at
/// its prompt with no Stop; a subagent's (agent_id set) are not the session's. ...
pub fn turn(payload: &Value) -> Option<Turn>;
//   The `"Stop" if running_background_task(..) => None` arm goes.

// removed:
// const ARTIFACT_WATCH_TYPE: &str;  const ARTIFACT_WATCH_PREFIX: &str;
// fn running_background_task(tasks: &Value) -> Option<&str>;
// fn artifact_watch(task: &Value) -> bool;
```

Fixtures `stop_background.json` and `stop_artifact_watch.json` stay as recorded Stops with
background work running.

Docs: `docs/flows.md` "the end of a turn" row drops "`delegated <the task>` while a background task
runs"; `docs/HANDOVER.md` `background_tasks` fact says saaio ignores it and why (a long-lived task
never finishes; a question waits for the person).

## Data

None. `EventKind::Delegated` / `State::Delegated` stay for the foreground Agent tool and Codex
`spawn_agent`. No log format change.

## Call chains

Worker ends a turn with a background task running:

1. Claude Code -> `saaio hook claude-code` (Stop, `background_tasks` running)
2. `hook::hook -> Adapter::translate(payload, workflow): [Answer { text }, Waiting { question } | Idle { note }]`
3. `hook::hook -> report(store, flow, kinds, log, now)`: appends both
4. `hook::hook -> Adapter::turn(payload): Some(Turn::Ended)` -> `mark_busy(Busy::Idle)`
5. `saaio wait --master` sees the stopped flow and wakes the master; the sidebar shows waiting/idle
6. When the task finishes and the agent resumes, its next PostToolUse records `working`.

Master: steps 1, 2, 4 with `Hooked::Master`; `record_master_turn -> fold::master_state(kinds): Some(Waiting | Idle)`.

Errors unchanged: an empty message gives `Idle { "turn ended" }` and no Answer; hook errors go to
server.log.

## Test seams

- `claude_code` unit `a_turn_ends_whatever_runs_in_the_background` replaces the delegated and
  artifact-watch tests: `stop_background` -> Answer + Idle; with a question -> Answer + Waiting;
  `stop_artifact_watch` -> Answer + Idle.
- `claude_code` turn test: `stop_background` -> `Some(Turn::Ended)`.
- `tests/cli.rs`: a worker Stop with a running background task and a question leaves the flow
  waiting with its answer and not busy; a master Stop with one leaves `master.turn` idle.

## Assumptions

1. The pending-watch and subagent-beside-watch tests are deleted: their case no longer exists.
2. No CHANGELOG line: only the 0.1.0 entry until it is tagged.
3. `master.md` stays: "delegated (a subagent ... runs for it)" still describes foreground subagents.
4. ROADMAP "Subagents in the sidebar" stays as written.

## Tasks

- [x] `translate` "Stop" and `turn` end every main-agent Stop; remove `running_background_task`, `artifact_watch` and the `ARTIFACT_WATCH_*` constants; unit test `a_turn_ends_whatever_runs_in_the_background` and the turn test expecting `Ended` for `stop_background`
- [x] `tests/cli.rs`: a worker that asks beside a background task is waiting with its answer and not busy; a master Stop beside one leaves `master.turn` idle
- [x] Docs: `docs/flows.md` end-of-turn row, `docs/HANDOVER.md` `background_tasks` fact
- [x] End-to-end check: the scratch-store reproduction (UI in a PTY, a real Claude Code worker starting `sleep 900` in the background and asking a question) shows the flow waiting with its answer, and the master woken
