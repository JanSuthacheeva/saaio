# The command line

- Output: [TOON](https://toonformat.dev), ending in `help[]` lines with suggested next commands.
- Programs read `flow status --json` ([json.md](json.md)).
- Output below is from real runs, paths shortened.

- [Commands at a glance](#commands-at-a-glance)
- [flow](#flow)
- [flow ui](#flow-ui)
- [flow start](#flow-start)
- [flow status](#flow-status)
- [flow send](#flow-send)
- [flow report](#flow-report)
- [flow new](#flow-new)
- [flow remove](#flow-remove)
- [flow clear](#flow-clear)
- [Commands flow runs itself](#commands-flow-runs-itself)
- [Output, errors and exit codes](#output-errors-and-exit-codes)
- [Environment variables](#environment-variables)
- [Several projects](#several-projects)

## Commands at a glance

| Command | Does |
| --- | --- |
| `flow` | every flow and what needs you |
| `flow ui [--stop]` | open the terminal UI, or stop it and every session |
| `flow start <request> [--name] [--mode] [--agent] [--type]` | start a worker in its own worktree |
| `flow start --plain <folder> [--name] [--agent]` | start an agent in a folder, without a flow |
| `flow status [<flow>] [--json] [--full] [--all]` | where flows stand |
| `flow send <flow> <text>` | tell a worker something |
| `flow report [--flow <flow>] <event>` | add an event to a flow by hand |
| `flow new --name <name> --request <text> [--mode]` | track a flow without starting a worker |
| `flow remove <flow> [--force] [--delete-unmerged]` | end a flow and tear down its worktree |
| `flow clear` | forget finished flows and tear down their worktrees |

- `<flow>`: a flow's name or id. A name means the unfinished flow of that name, else the most recent
  finished one.
- `flow --version` prints the version. Every command takes `--help`.

## flow

Flow counts, one line per flow, and the question or reason of each waiting or blocked flow.

```
$ flow
bin: ~/.cargo/bin/flow
description: "Tracks parallel feature flows: what each is doing, where it is blocked, how far it got"
count: 3
needs_you: 2
flows[3]{name,state,phase,tasks,age}:
  shop-221,waiting,plan,"",2s
  shop-217,blocked,implement,4/9,2s
  scratch,idle,"","",11s
blockers[2]{name,blocker}:
  shop-221,Should refunds count against the original order's totals or the refund date's?
  shop-217,"Task 5: the migration needs a default value"
help[2]: "Run `flow status <name>` for one flow's phases, timeline and decisions",Run `flow new --name <name> --request <text>` to track a new flow
```

- Order: needs you, ready (turn ended, nothing asked), running, finished. Most recent change first
  within each group.
- `age`: time since the flow's state, phase or blocker last changed.

## flow ui

```
flow ui
flow ui --stop
```

- Opens the terminal UI. Starts the session server if none runs. Screen: [ui.md](ui.md).
- On leaving, prints what happened, such as
  ``detached - the agents keep running; run `flow ui` to attach again``.
- `--stop`: stops the server and every session. Unfinished flows are kept and resume at the next
  `flow ui`. Prints `stopped the flow server and every session in it` or `no flow server runs`.
- Inside a worker, refused on the worker's own store (it would take over your flow ui). Set
  `FLOW_HOME` to another folder to run one there.

## flow start

```
flow start <request> [--name <name>] [--mode guided|auto] [--agent <agent>] [--type <type>]
flow start --plain <folder> [--name <name>] [--agent <agent>]
```

Run in the repository the flow works on. Steps:

1. names the flow
2. claims a slot (1 to 20, [repository.md](repository.md#slots-and-ports))
3. creates a git worktree in `.flow/worktrees/<name>` on a new branch
4. runs the repository's `.flow/setup` there, if present
5. records the flow, so flow ui opens the agent in the worktree

```
$ flow start "SHOP-217 add CSV export"
flow: shop-217
id: e514f1de-604d-4dbc-9b88-2cd76333d0a4
worktree: ~/projects/shop/.flow/worktrees/shop-217
branch: flow/shop-217
slot: 1
setup: none
agent: claude
pane: when flow ui runs
help[2]: Run `flow ui` to open the worker,Run `flow` for all flows
```

| Field | Values |
| --- | --- |
| `setup` | `ran`, `none` (no setup hook), `failed: <its last error line>`. A failed setup marks the flow blocked. The worker still opens. |
| `pane` | `opening` when flow ui runs, else `when flow ui runs` |

| Option | Default | Effect |
| --- | --- | --- |
| `<request>` | | what to build, such as `"SHOP-217 add CSV export"`. Up to 20000 characters. Part of the agent's first prompt. |
| `--name <name>` | from the request | the flow's name. Default: the ticket key at the start of the request, lowercased (`SHOP-217` becomes `shop-217`), else the first four words (`add-a-dark-mode`). A name held by an unfinished flow gets `-2`, `-3` and so on. |
| `--mode <mode>` | `guided` | `guided` or `auto`: who answers the worker's questions ([flows.md](flows.md#guided-and-auto)) |
| `--agent <agent>` | `[worker] agent`, else `claude` | an agent from the config ([configuration.md](configuration.md#agents)) |
| `--type <type>` | the repository's first type | a branch type from the repository's `.flow/config.toml` ([repository.md](repository.md#branch-names)) |
| `--plain <folder>` | | open the agent in this folder as a plain session |

The branch is `flow/<name>` unless the repository sets a convention. An existing branch gets `-2`,
`-3` and so on.

### Plain sessions

`--plain <folder>`: an agent in any folder, repository or not. No worktree, branch, slot or first
prompt. Ignored by the master ([flows.md](flows.md#plain-sessions)).

```
$ flow start --plain ~/projects/scratch
flow: scratch
id: 5418e55e-581e-4117-a46c-0824b8e475d9
folder: ~/projects/scratch
agent: claude
pane: when flow ui runs
help[2]: Run `flow ui` to open the session,Run `flow` for all flows
```

- Name: the folder's name, unless `--name`.
- A leading `~` is the home folder.
- Not combinable with a request, `--mode` or `--type`.

## flow status

```
flow status [<flow>] [--json] [--full] [--all]
```

Without a flow: every flow, as `flow`. With a flow: its request, mode, state, phases with time
spent, tasks, decisions and timeline.

```
$ flow status shop-217
name: shop-217
id: e514f1de-604d-4dbc-9b88-2cd76333d0a4
request: SHOP-217 add CSV export
mode: guided
state: blocked
phase: implement
blocker: "Task 5: the migration needs a default value"
tasks: 4/9
branch: flow/shop-217
worktree: ~/projects/shop/.flow/worktrees/shop-217
worktree_exists: true
steps[4]{phase,status,duration}:
  plan,done,5s
  implement,current,6s
  review,open,""
  merge-request,open,""
decisions[1]: "approach B: one exporter per format"
timeline[5]{ago,text}:
  11s,started
  11s,"phase: plan"
  8s,"decided: approach B: one exporter per format"
  6s,"phase: implement"
  2s,"blocked: Task 5: the migration needs a default value"
help[2]: Run `flow report --flow shop-217 working` once the block is resolved,Run `flow` for all flows
```

| Field | Shown |
| --- | --- |
| `blocker` | while there is one |
| `delegate` | while a subagent works (what the worker handed to it) |
| `tasks` | with task counts |
| `review`, `merge_request` | once reported |
| `branch`, `worktree`, `worktree_exists` | for a flow with a worktree. `worktree_exists` is false once the folder is gone. |
| `folder` | for a plain session |
| `answer` | the closing message of the worker's last turn |

| Option | Effect |
| --- | --- |
| `--json` | the JSON for programs instead of TOON ([json.md](json.md)) |
| `--full` | long texts in full. Without it, a text over 80 characters is cut and marked `(truncated, 87 chars - use --full)`. |
| `--all` | every project's flows, each line with its project ([Several projects](#several-projects)) |

## flow send

```
flow send <flow> <text>
```

The master's message to a worker: an answer, instruction or correction. Adds `master: <text>` to
the timeline and wakes the worker, or delivers it when the worker's session runs again.

```
$ flow send shop-221 "Count them against the refund date."
sent: shop-221
state: waiting
help[2]: "flow wakes its worker with it now, or once its session runs again",flow wakes you when shop-221 changes again
```

| Refused | Reason |
| --- | --- |
| inside a worker | `only the master sends`. Workers end their turn with the question. |
| a finished flow, a flow without a worker, a plain session | no worker to receive it |
| an empty text, a text over 4000 characters | |

## flow report

```
flow report [--flow <flow>] <event>
```

- Adds one event to a flow. For scripts, other tools and manual fixes. Workers report through their
  hooks ([flows.md](flows.md#how-a-worker-reports)).
- The flow: `--flow`, else `$FLOW_ID` (set in every worker). With neither, it does nothing and
  succeeds, so a skill can report outside a flow:

```
$ flow report working
skipped: no flow id
help[1]: Set FLOW_ID or pass --flow <name>
```

| Event | Means |
| --- | --- |
| `phase <name>` | the flow entered a phase of its workflow (`plan`, `implement`, `review`, `merge-request` by default). A phase the flow does not have is refused. |
| `working` | work goes on, after a question, a block or a subagent |
| `delegated <description>` | a subagent works on the flow |
| `waiting <question>` | the flow waits for your answer |
| `idle <note>` | the turn ended without a question |
| `blocked <reason>` | the flow cannot go on |
| `decision <text>` | a decision made on your behalf |
| `tasks <done> <total>` | how many of the plan's tasks are done |
| `review --fixed <n> --open <n>` | review findings fixed and still open |
| `merge-request <url>` | the merge request is open. The flow is done. |
| `ended` | the worker's session ended. flow ui does not reopen it. |
| `heartbeat` | the worker is alive. Adds no event (see `not-responding` in [flows.md](flows.md#states)). |

Each answers with the flow's new state:

```
$ flow report --flow shop-217 blocked "Task 5: the migration needs a default value"
flow: shop-217
state: blocked
phase: implement
help[1]: Run `flow report working` once the block is resolved
```

- The same state, phase, tasks or decision reported twice changes nothing. Retries are safe.
- `review`, `merge-request` and `ended` add a timeline entry every time.
- Texts are kept up to 500 characters.

## flow new

```
flow new --name <name> --request <text> [--mode guided|auto]
```

Tracks a flow without a worktree or worker, for a worker run by hand. An existing unfinished flow
of that name is returned (`existing: true`).

```
$ flow new --name shop-230 --request "SHOP-230 tracked by hand"
flow: shop-230
id: a825cdf3-a951-4333-bae2-e0c872be66bf
existing: false
help[2]: Run `flow report --flow shop-230 phase plan` when plan starts,Or set FLOW_ID=a825cdf3-a951-4333-bae2-e0c872be66bf in the worker's environment and leave out --flow
```

## flow remove

```
flow remove <flow> [--force] [--delete-unmerged]
```

Ends one flow in any state: closes the worker's session in flow ui, runs `.flow/teardown`, removes
the worktree, deletes the branch, forgets the flow.

```
$ flow remove shop-217
removed: shop-217
pane: none
worktree: removed
branch: deleted
help[1]: Run `flow` to see what is left
```

Refuses and changes nothing when the worktree has uncommitted work or the branch has commits on no
other branch, local or pushed:

```
$ flow remove shop-221
error: flow/shop-221 has 1 commit on no other branch
help[2]: "Merge or push flow/shop-221, then run `flow remove shop-221` again",Run `flow remove shop-221 --delete-unmerged` to delete it with its commits
```

| Option | Effect |
| --- | --- |
| `--force` | also remove a worktree with uncommitted work, discarding it |
| `--delete-unmerged` | also delete a branch whose commits are on no other branch |

A plain session: only the session is forgotten. The folder is never touched.

## flow clear

```
flow clear
```

Forgets every finished flow (`done` or `ended`) and tears down its worktree like `flow remove`,
except:

- a worktree with uncommitted work is kept and listed under `kept`
- a branch is deleted only when all its commits are on another branch
- a pushed branch with its merge request stays on the remote

```
$ flow clear
cleared: 2
names[2]: add-a-dark-mode,shop-230
kept[1]{name,path}:
  shop-230,~/projects/shop/.flow/worktrees/shop-230
help[2]: Run `flow` to see what is left,"Commit or discard the work in a kept worktree, then remove it with `git worktree remove <path>`"
```

## Commands flow runs itself

Not listed in `flow --help`. Not for manual use.

| Command | Run by |
| --- | --- |
| `flow server` | flow ui, when no server runs. Owns every session. Log: `server.log` ([files.md](files.md)). |
| `flow hook claude-code`, `flow hook codex` | the hooks flow adds to each worker. Reads the hook's event on stdin and reports what changes the flow. Never prints to stdout, never fails a worker's turn. For Codex, exits 2 at Stop to hand over news. |
| `flow wait [--master]` | Claude Code sessions, as a background hook. Sleeps until there is news for the session, prints it to stderr and exits 2, which wakes the session. `--master` waits for the master's news. |

## Output, errors and exit codes

Errors are TOON on stdout, with next steps:

```
$ flow report --flow shop-999 working
error: no flow named shop-999
help[2]: Run `flow` to list the flows,Run `flow new --name shop-999 --request <text>` to start it
```

| Exit code | Means |
| --- | --- |
| 0 | success |
| 1 | an error, such as a flow that does not exist |
| 2 | a usage error, such as an unknown option. `flow wait` also exits 2 when it has news. |

## Environment variables

Read by flow:

| Variable | Effect |
| --- | --- |
| `FLOW_HOME` | the folder flow keeps its flows in. Default: `$XDG_STATE_HOME/flow`, else `~/.local/state/flow`. |
| `FLOW_CONFIG` | the config file. Default: `$XDG_CONFIG_HOME/flow/config.toml`, else `~/.config/flow/config.toml`. |
| `XDG_STATE_HOME`, `XDG_CONFIG_HOME`, `HOME` | base of the defaults above |
| `FLOW_ID` | the flow `flow report` and `flow hook` report into. Set in every worker. |
| `FLOW_MASTER` | `1` in the master's session, whose hooks report no flow |
| `CLAUDE_CONFIG_DIR` | Claude Code's folder, where flow ui reads a worker's transcript for the peek. Default: `~/.claude`. |

Set by flow in every worker's session and in the setup and teardown hooks:

| Variable | Value |
| --- | --- |
| `FLOW_ID` | the flow's id |
| `FLOW_NAME` | the flow's name |
| `FLOW_SLOT` | the flow's slot, 1 to 20 |
| `FLOW_WORKTREE` | the worktree's path |
| `FLOW_REPO` | the repository's path |

- A plain session gets only `FLOW_ID`.
- Every session also gets `FLOW_HOME` when set, `TERM=xterm-256color` and `COLORTERM=truecolor`.

## Several projects

Default: one store, one server and one master for all projects. Separate projects with one
`FLOW_HOME` each, for example per tmux session:

```sh
export FLOW_HOME=~/.local/state/flow/projects/shop
```

Each store has its own flows, server and master. Shared in the default folder
(`$XDG_STATE_HOME/flow`, else `~/.local/state/flow`) regardless of `FLOW_HOME`:

| Shared | Purpose |
| --- | --- |
| store registry | `flow status --all` and the board's `a` key find every store |
| slots | no two unfinished flows in any project share a slot |

```
$ flow status --all
count: 3
needs_you: 2
flows[3]{project,name,state,phase,tasks,age}:
  shop,shop-221,waiting,plan,"",2s
  shop,shop-217,blocked,implement,4/9,2s
  shop,scratch,idle,"","",11s
blockers[2]{project,name,blocker}:
  shop,shop-221,Should refunds count against the original order's totals or the refund date's?
  shop,shop-217,"Task 5: the migration needs a default value"
help[2]: "Run `flow status <name>` in a flow's project for its phases, timeline and decisions",Run `flow status --all --json` for every project's board
```

- A project is named after its store's folder (`shop` above). The default store is `default`.
- A store whose folder is gone is dropped.
