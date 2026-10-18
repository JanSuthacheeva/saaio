# The JSON contract

```sh
saaio status --json          # this project's flows
saaio status --all --json    # every project's flows
saaio status shop-217 --json # a board with just that flow
```

- The whole board: every flow, nothing truncated.
- camelCase keys, `version` first.
- The command line and this JSON are saaio's only public interface. The log files are private and
  may change.

## The board

```json
{
  "version": 3,
  "now": 1792214602016,
  "count": 2,
  "needsYou": 1,
  "flows": [
    {
      "id": "e514f1de-604d-4dbc-9b88-2cd76333d0a4",
      "name": "shop-217",
      "request": "SHOP-217 add CSV export",
      "mode": "guided",
      "phase": "implement",
      "state": "blocked",
      "blocker": "Task 5: the migration needs a default value",
      "delegate": null,
      "tasks": { "done": 4, "total": 9 },
      "review": null,
      "mergeRequest": null,
      "merged": false,
      "decisions": ["approach B: one exporter per format"],
      "steps": [
        { "phase": "plan", "status": "done", "durationMs": 5052 },
        { "phase": "implement", "status": "current", "durationMs": 22132 },
        { "phase": "review", "status": "open", "durationMs": null },
        { "phase": "merge-request", "status": "open", "durationMs": null }
      ],
      "timeline": [
        { "at": 1792214574703, "text": "started", "tone": "neutral" },
        { "at": 1792214574832, "text": "phase: plan", "tone": "progress" },
        { "at": 1792214577856, "text": "decided: approach B: one exporter per format", "tone": "neutral" },
        { "at": 1792214579884, "text": "phase: implement", "tone": "progress" },
        { "at": 1792214583915, "text": "blocked: Task 5: the migration needs a default value", "tone": "problem" }
      ],
      "startedAt": 1792214574703,
      "changedAt": 1792214583915,
      "workspace": {
        "path": "/Users/jan/projects/shop/.saaio/worktrees/shop-217",
        "branch": "flow/shop-217",
        "slot": 1,
        "window": null,
        "agent": "claude"
      },
      "answer": null,
      "session": null
    },
    {
      "id": "5418e55e-581e-4117-a46c-0824b8e475d9",
      "name": "scratch",
      "request": "",
      "mode": "plain",
      "phase": null,
      "state": "idle",
      "blocker": null,
      "delegate": null,
      "tasks": null,
      "review": null,
      "mergeRequest": null,
      "merged": false,
      "decisions": [],
      "steps": [],
      "timeline": [{ "at": 1792214574828, "text": "started", "tone": "neutral" }],
      "startedAt": 1792214574828,
      "changedAt": 1792214574828,
      "workspace": {
        "path": "/Users/jan/projects/scratch",
        "branch": null,
        "slot": null,
        "window": null,
        "agent": "claude"
      },
      "answer": null,
      "session": null
    }
  ]
}
```

| Key | Type | Holds |
| --- | --- | --- |
| `version` | number | the contract's version, 3 today |
| `now` | number | when the board was made, in Unix milliseconds |
| `count` | number | how many flows there are |
| `needsYou` | number | how many are `waiting`, `blocked` or `not-responding` |
| `flows` | array | the flows, sorted: needs you, ready (`idle`), running, finished. The most recent change first within each group |

All times are Unix milliseconds.

## A flow

| Key | Type | Holds |
| --- | --- | --- |
| `id` | string | the flow's id, a UUID |
| `name` | string | the flow's name |
| `request` | string | what was asked for. Empty for a plain session |
| `mode` | string | `guided`, `auto` or `plain` |
| `phase` | string or null | the phase it is in. Null before its first phase and for a plain session |
| `state` | string | see [States](#states) |
| `blocker` | string or null | the question of a `waiting` flow, or the reason of a `blocked` one |
| `delegate` | string or null | what a subagent works on while the flow is `delegated` |
| `tasks` | object or null | `{ "done", "total" }`, the plan's tasks |
| `review` | object or null | `{ "fixed", "open" }`, the review's findings |
| `mergeRequest` | string or null | the merge request's URL |
| `merged` | boolean | the merge request was merged |
| `decisions` | array of strings | decisions made on your behalf, oldest first |
| `steps` | array | one per phase of the flow's workflow. Empty for a plain session. See [Steps](#steps). |
| `timeline` | array | what happened, oldest first. See [Timeline](#timeline). |
| `startedAt` | number | when the flow started |
| `changedAt` | number | when its state, phase or blocker last changed |
| `workspace` | object or null | where its worker runs. Null for a flow made with `saaio new`. See [Workspace](#workspace). |
| `answer` | string or null | the worker's last turn's whole closing message, as its harness reported it |
| `session` | object or null | `{ "id", "transcript" }`: the session id the harness picked and the path of its transcript (`transcript` may be null). Set for Codex, which picks its own id. Null for Claude Code, which uses the flow's. |

### States

| State | Means |
| --- | --- |
| `working` | the worker is at work |
| `delegated` | a subagent works on the flow |
| `waiting` | the worker asks you something (`blocker` holds the question) |
| `idle` | the worker's turn ended without a question |
| `blocked` | the flow cannot go on (`blocker` holds the reason) |
| `not-responding` | the worker sent heartbeats and stopped for 90 seconds |
| `done` | the merge request is open or merged |
| `ended` | the worker's session ended for good |

`waiting`, `blocked` and `not-responding` count as needing you.

### Steps

| Key | Type | Holds |
| --- | --- | --- |
| `phase` | string | the phase's name, as the flow's workflow names it |
| `status` | string | `done`, `current` or `open` |
| `durationMs` | number or null | the time spent in the phase. The current phase counts up to now. Null for a phase not reached |

A flow keeps the phases it started with. Two flows can have different steps.

### Timeline

| Key | Type | Holds |
| --- | --- | --- |
| `at` | number | when it happened |
| `text` | string | what happened: `started`, `phase: plan`, `waiting: ...`, `answered`, `idle: ...`, `blocked: ...`, `decided: ...`, `review: 3 fixed, 1 open`, `MR opened`, `PR #42 merged`, `PR #42 closed`, `master: ...` (a message from the master), `ended` |
| `tone` | string | `neutral`, `progress`, `attention`, `problem` or `success` |

### Workspace

| Key | Type | Holds |
| --- | --- | --- |
| `path` | string | the worktree, or a plain session's folder |
| `branch` | string or null | the flow's branch. Null for a plain session |
| `slot` | number or null | the flow's slot, 1 to 20. Null for a plain session |
| `window` | string or null | `saaio-ui` once the UI opened the worker. Null before |
| `agent` | string | the agent it runs, a name from the config |

## Every project

`saaio status --all --json`: one board per registered store with flows.

```json
{
  "version": 3,
  "now": 1792214602016,
  "projects": [
    { "name": "shop", "root": "/Users/jan/.local/state/saaio/projects/shop", "board": { "version": 3, "...": "..." } }
  ]
}
```

| Key | Type | Holds |
| --- | --- | --- |
| `name` | string | the project's name: its store's folder name, or `default` for the default store. ` (2)`, ` (3)` and so on after a name already taken |
| `root` | string | the store's folder |
| `board` | object | the board, as `saaio status --json` prints it |

## Versions

- `version` increases when a change could break a reader.
- New keys can appear without a new version. Ignore unknown keys.

| Version | Changed |
| --- | --- |
| 1 | the first contract |
| 2 | plain sessions: `workspace.branch` and `workspace.slot` can be null, and `mode` can be `plain` |
| 3 | `phase` and `steps[].phase` are the names of the flow's own workflow, not always the built-in four |
