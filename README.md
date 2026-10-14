# flow

Tracks parallel feature flows: what each is doing, where it is blocked, how far it got.

`flow` is an [AXI](https://axi.md), a command line tool whose main user is an agent. A worker
reports what it does with `flow report`; anyone - an agent, a person, the Claude Code board -
asks `flow` where every flow stands. Each flow is an append-only event log; `flow` alone
folds the logs into state, phases with durations, task counts, blockers and a timeline.

## Install

```sh
cargo install --path .
```

## Commands

```
flow                                    home view: every flow and what needs you
flow start <request> [--name <name>] [--mode guided|auto] [--agent <name>]
                                        a worker in its own worktree and tmux window
flow new --name <name> --request <text> [--mode guided|auto]
flow report [--flow <name|id>] <event>  the flow comes from --flow or $FLOW_ID
flow status [<name|id>] [--json] [--full]
flow remove <name|id> [--force]         stop a flow's worker, tear down its worktree, forget it
flow clear                              forget finished flows, tear down their worktrees
```

Events for `flow report`:

| Event | Meaning |
| --- | --- |
| `phase <design\|implement\|review\|merge-request>` | the flow entered a phase |
| `working` | work goes on, after a stop or a subagent |
| `delegated <description>` | a subagent works on the flow |
| `waiting <question>` | the flow waits for the person's answer |
| `blocked <reason>` | the flow cannot go on |
| `decision <text>` | a decision made on the person's behalf |
| `tasks <done> <total>` | plan tasks done |
| `review --fixed <n> --open <n>` | review findings fixed and still open |
| `merge-request <url>` | the merge request is open; the flow is done |
| `ended` | the worker session ended |
| `heartbeat` | the worker is alive; no event is added |

`flow report` without a flow id answers `skipped: no flow id` and exits 0, so a skill can
report unconditionally and still work in a session that is not a flow. Repeated reports fold
to the same state, so retries are harmless.

## Starting a worker

Run `flow start` inside the repository and inside tmux:

```
$ flow start "SHOP-217 add CSV export"
flow: shop-217
worktree: ~/projects/shop/.flow/worktrees/shop-217
branch: flow/shop-217
slot: 1
setup: ran
agent: claude
window: @14
```

It creates the flow, a git worktree in `.flow/worktrees/<name>` on branch `flow/<name>` (kept out
of `git status` through `.git/info/exclude`), runs the repo's setup hook there, and opens a
background tmux window running the agent with `FLOW_ID` set, so it reports into the flow. The
name comes from the ticket key (`SHOP-217` -> `shop-217`) or the first four words.

### Agents

Claude Code is built in: `claude -n <name> --session-id <id> "/ship-feature <request>"`. Add or
change agents in `~/.config/flow/config.toml` (or `$XDG_CONFIG_HOME/flow/config.toml`, or
`$FLOW_CONFIG`):

```toml
agent = "claude"              # the default for flow start

[agents.claude]
command = ["claude", "-n", "{name}", "--session-id", "{id}", "{prompt}"]
prompt = "/ship-feature {request}"

[agents.codex]
command = ["codex", "{prompt}"]
prompt = "{request}"
```

Placeholders: `{id}`, `{name}`, `{request}`, `{prompt}`.

### Hooks, slots and Docker

A repo can commit two executable hooks; `flow` runs them in the worktree with `FLOW_ID`,
`FLOW_NAME`, `FLOW_SLOT`, `FLOW_WORKTREE` and `FLOW_REPO` set:

- `.flow/setup` after the worktree is created. If it fails, the flow is marked blocked with its
  last error line and the window still opens so you can look.
- `.flow/teardown` when `flow clear` removes a finished flow's worktree.

`FLOW_SLOT` is 1 to 9, unique among running flows, for anything that must not collide, such as
ports. For a Docker Compose project whose tests run in the containers, read the ports from
variables in `compose.yaml` (`"${HTTP_PORT:-80}:80"`, `"${DB_PORT:-3306}:3306"`), then:

```sh
#!/bin/sh
# .flow/setup
set -e
export HTTP_PORT=$((8000 + FLOW_SLOT)) DB_PORT=$((33060 + FLOW_SLOT))
docker compose up -d
docker compose exec -T php php artisan migrate --seed
```

```sh
#!/bin/sh
# .flow/teardown
docker compose down -v
```

Compose names a project after its folder, so each worktree runs its own stack and
`docker compose exec ...` inside a worktree reaches that stack.

`flow remove <name>` does the same for one flow at any state: it closes the worker's tmux
window, runs the teardown hook, removes the worktree and forgets the flow. With uncommitted
work in the worktree it refuses and touches nothing; `--force` discards that work.

`flow clear` keeps a worktree with uncommitted work (listed under `kept`) and always keeps the
branch, which carries the merge request.

## Output

Agents get [TOON](https://toonformat.dev): short default fields, counts, explicit empty states
and `help[]` lines with the next commands. Long texts are cut at 80 characters with a hint;
`--full` shows them whole.

```
$ flow
bin: ~/.cargo/bin/flow
description: "Tracks parallel feature flows: what each is doing, where it is blocked, how far it got"
count: 1
needs_you: 1
flows[1]{name,state,phase,tasks,age}:
  shop-221,blocked,implement,4/9,1m
blockers[1]{name,blocker}:
  shop-221,"Task 5: migration needs a default value"
help[2]: "Run `flow status <name>` for one flow's phases, timeline and decisions",Run `flow new --name <name> --request <text>` to track a new flow
```

```
$ flow status shop-221
name: shop-221
id: 265ecccc-ce1c-4d32-a02a-7795fb47c01e
request: SHOP-221 add CSV export
mode: auto
state: blocked
phase: implement
blocker: "Task 5: migration needs a default value"
tasks: 4/9
steps[4]{phase,status,duration}:
  design,done,9m
  implement,current,31m
  review,open,""
  merge-request,open,""
decisions[1]: "approach B: forge adapter"
timeline[5]{ago,text}:
  41m,started
  41m,"phase: design"
  35m,"decided: approach B: forge adapter"
  32m,"phase: implement"
  1m,"blocked: Task 5: migration needs a default value"
help[2]: Run `flow report --flow shop-221 working` once the block is resolved,Run `flow` for all flows
```

Errors are TOON on stdout too, with next steps. Exit codes: 0 ok, 1 error, 2 usage.

```
$ flow report --flow shop-999 working
error: no flow named shop-999
help[2]: Run `flow` to list the flows,Run `flow new --name shop-999 --request <text>` to start it
```

States: `working`, `delegated`, `waiting`, `blocked`, `not-responding` (a flow that sent
heartbeats and stopped for 90 seconds), `done`, `ended`.

## The `--json` contract

Programs read `flow status --json`: the whole board, nothing truncated, camelCase keys,
`version` first. This and the command line are the only contract; the log format is private.

```json
{
  "version": 1,
  "now": 1791953473760,
  "count": 1,
  "needsYou": 1,
  "flows": [
    {
      "id": "265ecccc-ce1c-4d32-a02a-7795fb47c01e",
      "name": "shop-221",
      "request": "SHOP-221 add CSV export",
      "mode": "auto",
      "phase": "implement",
      "state": "blocked",
      "blocker": "Task 5: migration needs a default value",
      "delegate": null,
      "tasks": { "done": 4, "total": 9 },
      "review": null,
      "mergeRequest": null,
      "decisions": ["approach B: forge adapter"],
      "steps": [
        { "phase": "design", "status": "done", "durationMs": 540000 },
        { "phase": "implement", "status": "current", "durationMs": 1860000 },
        { "phase": "review", "status": "open", "durationMs": null },
        { "phase": "merge-request", "status": "open", "durationMs": null }
      ],
      "timeline": [{ "at": 1791951013760, "text": "started", "tone": "neutral" }],
      "startedAt": 1791951013760,
      "changedAt": 1791953413760
    }
  ]
}
```

Timeline tones: `neutral`, `progress`, `attention`, `problem`, `success`.

## Where the logs live

`$FLOW_HOME`, else `$XDG_STATE_HOME/flow`, else `~/.local/state/flow`: one
`flows/<id>.jsonl` per flow, and `flows/<id>.alive` whose modification time is the last
heartbeat.
