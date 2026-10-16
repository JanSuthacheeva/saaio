# An artifact watch is not delegated work

No review page: a one-function fix, decided with the master in the design conversation.

## Decision

- A worker that publishes a claude.ai Artifact stays `delegated` forever: Claude Code arms a live
  subscription for the artifact, a `monitor_ws` task the Stop hook reports in `background_tasks` as
  `{"type": "monitor", "status": "running", "description": "live updates for artifact <url> (<how it was armed>)"}`
  (Claude Code 2.1.301, its `KEn` serializer and `Yt` description), and `running_background_task`
  reads it as work in flight.
- A watch never finishes, so the flow is not waiting on it. `running_background_task` skips tasks with
  `type == "monitor"` whose description starts with `live updates for artifact `; the turn then folds
  to the answer and idle or waiting as usual.
- Only artifact watches are skipped: background subagents, shells (shell Monitors included) and
  deliberate WebSocket or MCP monitors still count as delegated.
- The fixture is a recorded Stop payload, so a rewording of the description in Claude Code fails the
  test instead of silently bringing the bug back.

## Units

```rust
// src/adapter/claude_code.rs (modified)
/// What a turn ending with background work in flight waits for: the first running or pending
/// task's description, artifact watches aside.
fn running_background_task(tasks: &Value) -> Option<&str>;
/// A live subscription Claude Code keeps for a published artifact: it never finishes.
fn artifact_watch(task: &Value) -> bool;
```

## Call chain

1. Claude Code -> `flow hook claude-code` (Stop payload on stdin)
2. `translate` "Stop" -> `running_background_task(&payload["background_tasks"])`
3. `running_background_task` -> `artifact_watch(task)` per task: a watch is skipped
4. Only watches running -> None -> `Answer` + `turn_end(answer)` (Idle or Waiting)
5. A subagent or shell running beside a watch -> `Delegated { description }` of that task

## Test seams

- `src/adapter/fixtures/claude-code/stop_artifact_watch.json`: a recorded Stop with only the watch
  running -> `[Answer, Idle]` (or Waiting, by the answer).
- Same payload with a running subagent added (built in the test with `json!`) -> `Delegated` of the
  subagent.
- `stop_background.json` keeps proving a background subagent is delegated.

## Assumptions

- The prefix `live updates for artifact ` together with `type == "monitor"` identifies a watch for
  every way it is armed (publish, /artifacts attach, watch, resume, MCP edit).
- A watch whose status is `pending` is skipped as well.

## Tasks

- [x] Reproduce end to end: a real Claude worker publishes an artifact and its flow stays delegated; record its Stop payload as `stop_artifact_watch.json`
- [x] `artifact_watch` and the skip in `running_background_task`, with tests for a watch alone (idle) and a watch beside a running subagent (delegated)
- [x] Docs: the `background_tasks` fact in `docs/HANDOVER.md` names the artifact watch and that it is skipped
- [x] End-to-end check: the same real worker publishing an artifact ends its turn idle (or waiting) and wakes the master
