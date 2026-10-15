# flow

Tracks parallel feature flows: what each is doing, where it is blocked, how far it got.

`flow` is an [AXI](https://axi.md), a command line tool whose main user is an agent. A worker
reports what it does with `flow report`; anyone - an agent, a person, `flow ui` -
asks `flow` where every flow stands. Each flow is an append-only event log; `flow` alone
folds the logs into state, phases with durations, task counts, blockers and a timeline.

## Install

```sh
cargo install --path .
```

## Commands

```
flow                                    home view: every flow and what needs you
flow ui [--stop]                        the terminal UI: the master, every worker, the board
flow start <request> [--name <name>] [--mode guided|auto] [--agent <name>] [--type <type>]
                                        a worker in its own worktree, opened in flow ui
flow new --name <name> --request <text> [--mode guided|auto]
flow report [--flow <name|id>] <event>  the flow comes from --flow or $FLOW_ID
flow status [<name|id>] [--json] [--full]
flow status --all [--json] [--full]     every project's flows, with their project
flow remove <name|id> [--force] [--delete-unmerged]
                                        end a flow's worker, tear down its worktree and branch, forget it
flow send <name|id> <text>              tell a flow's worker something (the master's command)
flow clear                              forget finished flows, tear down their worktrees
```

Events for `flow report`:

| Event | Meaning |
| --- | --- |
| `phase <design\|implement\|review\|merge-request>` | the flow entered a phase |
| `working` | work goes on, after a stop or a subagent |
| `delegated <description>` | a subagent works on the flow |
| `waiting <question>` | the flow waits for the person's answer |
| `idle <note>` | the turn ended without a question; nothing for the person to do |
| `blocked <reason>` | the flow cannot go on |
| `decision <text>` | a decision made on the person's behalf |
| `tasks <done> <total>` | plan tasks done |
| `review --fixed <n> --open <n>` | review findings fixed and still open |
| `merge-request <url>` | the merge request is open; the flow is done |
| `ended` | the flow is over; flow ui does not open it again |
| `heartbeat` | the worker is alive; no event is added |

`flow report` without a flow id answers `skipped: no flow id` and exits 0, so a skill can
report unconditionally and still work in a session that is not a flow. Repeated reports fold
to the same state, so retries are harmless.

## flow ui

`flow ui` is home: one Claude Code session, the master, almost full screen, and every worker
beside it. It starts a session server (`flow server`) when none runs; the server owns every
session, so closing the terminal, a crash of the UI or `C-space d` leaves them running, and the
next `flow ui` attaches again. The master runs in the folder of the first `flow ui` with the
orchestrator instructions in `master.md` beside `config.toml` (flow writes the default and
updates it with flow while you have not edited it; your edits are never overwritten).

Every unfinished flow with a worktree gets its own session: `flow start` records the flow and
the server opens it within half a second. The sidebar on the right groups the workers under
NEEDS YOU, READY (idle: a turn ended, nothing asked), RUNNING and DONE with their steps, tasks and
what they ask or do.

Keys, after the prefix `C-space` (`C-space C-space` sends one through):

| Key | Does |
| --- | --- |
| `0`-`9` | jump to a session; 0 is the master, 1 the top of the sidebar |
| `j` `k`, `enter` | move the peek over the workers (their latest tool calls and text, read from Claude's transcript), jump in |
| `n` | start a flow: request, mode, what it creates |
| `o` | the board: a card per session and the selected flow's timeline, decisions and steps; `a` there shows every project's flows (read-only for other projects) |
| `b` | fold the sidebar to a rail and back |
| `r` | resume a session that exited, or an ended flow |

When a worker's session ends, its pane offers `x` to clean the flow up (worktree, branch, flow;
it asks first before discarding uncommitted changes or commits on no other branch), `r` to resume
it and `esc` to go back to the master.
| `[` | scroll back through the session's history |
| `d` | detach |
| `Q` | stop: lists unfinished workers; `k` keeps them for the next `flow ui`, `e` ends them |
| `?` | every key |

`flow ui --stop` stops the server and every session, keeping unfinished flows for the next
start. After a crash or a reboot the next `flow ui` resumes the master and every worker
(`claude --resume`); after a stop the master starts fresh.

### Themes

flow ui reads `theme.toml` beside `config.toml` (`~/.config/flow/theme.toml`) and repaints within a
second when it, or the theme it extends, changes. Without it flow uses the terminal's own colors.

```toml
# Colors: "#rrggbb", an ANSI index, "default" (the terminal's), or another key.
extends = "flow"        # terminal | flow | flow-light | a file in themes/, without .toml
nerd_font = true        # the font has Nerd Font icons: a kiwi bird as the mark
[ui]
accent = "#d9825b"
[state]
ended = "faint"
```

Keys: `[ui]` background, panel, selection, rule, text, strong, muted, faint, accent; `[state]`
working, waiting, blocked, done, ended. A key left out keeps the base's value. `nerd_font` (default
false) shows flow's mark as a kiwi bird, which needs a Nerd Font; without it the mark is a goose
in flight, `▚▄▞`, in block characters every font has. The mark takes the theme's accent either way. `flow` and `flow-light` are the design's dark
and light colors. A tool can keep a theme in `~/.config/flow/themes/<name>.toml` for `extends` to
name; a Noctalia template that writes `themes/noctalia.toml` makes flow follow Noctalia live. A
broken file keeps the last theme and says why in the title row.

## Starting a worker

Run `flow start` inside the repository, usually from the master:

```
$ flow start "SHOP-217 add CSV export"
flow: shop-217
worktree: ~/projects/shop/.flow/worktrees/shop-217
branch: flow/shop-217
slot: 1
setup: ran
agent: claude
pane: opening
```

It creates the flow, a git worktree in `.flow/worktrees/<name>` on branch `flow/<name>` (kept out
of `git status` through `.git/info/exclude`), runs the repo's setup hook there, and records the
flow's workspace; flow ui then opens the agent in it with `FLOW_ID` set, so it reports into the
flow (`pane: when flow ui runs` when no flow ui runs yet). The name comes from the ticket key
(`SHOP-217` -> `shop-217`) or the first four words.

### Agents

Claude Code is built in: `claude -n <name> --session-id <id> "/ship-feature <request>"`, resumed
with `claude -n <name> --resume <id>`. The master is the built-in `master` agent: Claude with
`--append-system-prompt-file <master.md>`. Add or change agents in `~/.config/flow/config.toml`
(or `$XDG_CONFIG_HOME/flow/config.toml`, or `$FLOW_CONFIG`):

```toml
agent = "claude"              # the default for flow start

[agents.claude]
command = ["claude", "-n", "{name}", "--session-id", "{id}", "{prompt}"]
prompt = "/ship-feature {request}"
resume = ["claude", "-n", "{name}", "--resume", "{id}"]

[agents.codex]
command = ["codex", "{prompt}"]
prompt = "{request}"
```

Placeholders: `{id}`, `{name}`, `{request}`, `{prompt}`, and `{instructions}` (the master's
`master.md`). An agent without `resume` starts fresh once and is not resumed after a restart.

### How a worker reports

A worker reports through its harness's own hooks; neither the model nor the skills take part.
When flow ui starts a worker whose agent names an `adapter`, it adds the adapter's wiring to the
command line. For Claude Code (`adapter = "claude-code"`, the built-in agents' default) that is
`--settings` with hooks that run `flow hook claude-code` on each event; they run beside your own
hooks and install nothing. `flow hook` reads the event, turns it into flow events and appends
only what changes the flow:

| Claude Code event | Reported |
| --- | --- |
| a prompt | `working` (and the phase of a typed `/skill`) |
| the Skill tool | `phase`, from the `[phases]` map |
| the Agent tool | `delegated <description>` |
| AskUserQuestion | `waiting <the question>` |
| a permission prompt | `waiting permission: Bash(rm -rf build)` |
| a tool finishing | `working`; plan-progress task counts and blocks; a merge request `gh` / `glab` created |
| the end of a turn | `waiting <the question>` when Claude's last line asks one, else `idle <its last sentence>`; the whole closing message becomes the flow's `answer` |
| `/exit` | `ended` (a hangup is not an end; flow ui resumes the worker) |

A subagent's own events are ignored. A custom agent replaces the built-in one, so give it
`adapter = "claude-code"` when it runs Claude Code. Which skill starts which phase:

```toml
[phases]                      # the built-in map, extend or override it
design-code = "design"
implement-plan = "implement"
review-changes = "review"
write-merge-request = "merge-request"
```

`flow report` stays for scripts and other tools.

### The master and its workers

Every Claude Code session flow starts, the master included, also runs `flow wait` as an
`asyncRewake` hook at its start and at each turn end. `flow wait` sleeps until there is news for
its session, prints it and exits 2, and Claude Code wakes the session with it:

- A worker is woken by the master's `flow send <name> "<text>"`, which adds a `master: <text>`
  line to the flow's timeline. A worker whose session is not running gets it when it runs again.
  Its system prompt says who the flow master is, so it acts on the message instead of taking hook
  feedback for an untrusted voice.
- The master is woken when flows of its project become `waiting`, `blocked`, `idle`, `done` or
  `not-responding`; flows that stop within two seconds come as one list.

The flow's mode decides who answers a waiting worker. In an `auto` flow the master answers what
the code, the plan or your earlier decisions settle, and tells you what it answered. A `guided`
flow's questions go to you, and the master sends your answer on. `flow status <name> --full`
shows a worker's whole last answer. To stop the master being woken:

```toml
[master]
wake = false
```

### Branch names

A flow's branch is `flow/<name>`, unless the repo commits a convention in `.flow/config.toml`:

```toml
[branch]
name  = "{type}/{ticket}-{slug}"                  # {type} {ticket} {slug} {name}
types = ["feature", "bugfix", "hotfix", "chore"]  # the first is the default
```

`SHOP-230 fix totals on refunds` with `--type bugfix` (or `bugfix` picked in the new-flow dialog)
becomes `bugfix/SHOP-230-fix-totals-on-refunds`. `{ticket}` is the request's ticket key as
written, `{slug}` up to five words after it; an empty placeholder takes the separator next to it
along. A branch that already exists gets `-2`. The worktree folder keeps the flow's name.

### Hooks, slots and Docker

A repo can commit two executable hooks; `flow` runs them in the worktree with `FLOW_ID`,
`FLOW_NAME`, `FLOW_SLOT`, `FLOW_WORKTREE` and `FLOW_REPO` set:

- `.flow/setup` after the worktree is created, before the worker starts. If it fails, the flow is
  marked blocked with its last error line and the worker still opens so you can look.
- `.flow/teardown` when `flow clear` removes a finished flow's worktree.

The worker's session gets the same variables, so it can reach its own stack, for example on port
`$((8000 + FLOW_SLOT))`.

`FLOW_SLOT` is 1 to 20, unique among unfinished flows in every project, for anything that must not collide, such as
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

`flow remove <name>` does the same for one flow at any state: it ends the worker's session in
flow ui, runs the teardown hook, removes the worktree, deletes the branch and forgets the flow.
It refuses and touches nothing when the worktree has uncommitted work (`--force` discards it) or
the branch has commits on no other branch, local or pushed (`--delete-unmerged` deletes them).

`flow clear` keeps a worktree with uncommitted work (listed under `kept`), and deletes a
finished flow's branch only when its commits are on another branch; a pushed branch with its
merge request stays on the remote. `flow status <name>` shows a flow's `branch`, `worktree`
and whether that worktree is still there.

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

States: `working`, `delegated`, `waiting` (asks you something), `idle` (its turn ended without
a question; not under NEEDS YOU), `blocked`, `not-responding` (a flow that sent heartbeats and
stopped for 90 seconds), `done`, `ended`.

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
      "changedAt": 1791953413760,
      "workspace": null,
      "answer": null
    }
  ]
}
```

Timeline tones: `neutral`, `progress`, `attention`, `problem`, `success`. `answer` is the last
turn's whole closing message, as the worker's harness reported it.

## Where the logs live

`$FLOW_HOME`, else `$XDG_STATE_HOME/flow`, else `~/.local/state/flow`: one
`flows/<id>.jsonl` per flow, and `flows/<id>.alive` whose modification time is the last
heartbeat. Beside them flow ui keeps `server.sock` (readable only by you), `server.log` and
`master.json` (the master's session id and folder).

### Several projects

Give each project its own `FLOW_HOME` (for example per tmux session,
`~/.local/state/flow/projects/<name>`) and each gets its own server, master and flows. They share
the default folder (`$XDG_STATE_HOME/flow`, else `~/.local/state/flow`, whatever `FLOW_HOME` says):

- `stores/<hash>`: every flow command registers its folder there, so `flow status --all` and the
  board's `a` find every project; a folder that is gone is dropped.
- `slots/<n>`: `flow start` claims its slot there before setup runs, so no two unfinished flows in
  any project share a slot. A claim only counts while its flow is unfinished.

`flow status --all --json` prints `{"version", "now", "projects": [{"name", "root", "board"}]}`,
each board as `flow status --json` prints it. A project is named after its folder.
