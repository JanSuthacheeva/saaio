# The command line

flow's command line is written for agents first. Every command answers in
[TOON](https://toonformat.dev), a compact text format, and ends with `help[]` lines suggesting what
to run next. Programs should read `flow status --json` instead (see [json.md](json.md)).

All the output on this page comes from real runs, with only the paths shortened.

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

Wherever a command takes `<flow>`, you can give a flow's name or its id. A name means the
unfinished flow with that name, or if there's none, the most recent finished one.
`flow --version` prints the version, and every command takes `--help`.

## flow

Run on its own, `flow` gives you the overview. It shows how many flows there are and how many need
you, a line per flow, and what each waiting or blocked flow is asking.

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

The flows that need you come first, then the ready ones (their turn ended and nothing was asked),
then the running ones, then the finished ones. Within each group, the most recent change comes
first. `age` is how long ago the flow's state, phase or blocker last changed.

## flow ui

```
flow ui
flow ui --stop
```

Opens the terminal UI, and starts the session server behind it if it isn't running yet.
[ui.md](ui.md) explains everything on the screen.

When you leave, flow ui prints what happened, for example
``detached - the agents keep running; run `flow ui` to attach again``.

`--stop` stops the server and every session in it. Unfinished flows are kept, and their workers
resume the next time you run `flow ui`. It prints `stopped the flow server and every session in
it`, or `no flow server runs`.

Inside a worker, `flow ui` refuses to run on the worker's own store, because it would take over
your flow ui. Set `FLOW_HOME` to another folder to run one there.

## flow start

```
flow start <request> [--name <name>] [--mode guided|auto] [--agent <agent>] [--type <type>]
flow start --plain <folder> [--name <name>] [--agent <agent>]
```

Run it inside the repository the flow should work on. In order, it:

1. names the flow
2. claims a slot (1 to 20, see [repository.md](repository.md#slots-and-ports))
3. creates a git worktree in `.flow/worktrees/<name>` on a new branch
4. runs the repository's `.flow/setup` there, if it has one
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

`setup` says `ran`, `none` (the repository has no setup hook) or `failed: <its last error line>`.
A failed setup marks the flow blocked, but the worker still opens so you can look into it. `pane`
says `opening` if flow ui is running, and `when flow ui runs` if it isn't.

| Option | Default | Does |
| --- | --- | --- |
| `<request>` | | what to build, for example `"SHOP-217 add CSV export"`; up to 20000 characters. It becomes part of the agent's first prompt. |
| `--name <name>` | from the request | the flow's name. Without it: the ticket key at the start of the request, lowercased (`SHOP-217` becomes `shop-217`), else its first four words (`add-a-dark-mode`). A name an unfinished flow holds gets `-2`, `-3` and so on. |
| `--mode <mode>` | `guided` | `guided` or `auto`: who answers the worker's questions. See [flows.md](flows.md#guided-and-auto). |
| `--agent <agent>` | `[worker] agent`, else `claude` | an agent from the config. See [configuration.md](configuration.md#agents). |
| `--type <type>` | the repository's first type | a branch type from the repository's `.flow/config.toml`. See [repository.md](repository.md#branch-names). |
| `--plain <folder>` | | open the agent in this folder instead, as a plain session |

The branch is `flow/<name>` unless the repository has its own convention. If the branch already
exists, flow adds `-2`, `-3` and so on.

### Plain sessions

`--plain <folder>` opens an agent in any folder, repository or not. You get no worktree, branch,
slot or first prompt, and the master leaves it alone (see [flows.md](flows.md#plain-sessions)).

```
$ flow start --plain ~/projects/scratch
flow: scratch
id: 5418e55e-581e-4117-a46c-0824b8e475d9
folder: ~/projects/scratch
agent: claude
pane: when flow ui runs
help[2]: Run `flow ui` to open the session,Run `flow` for all flows
```

The session is named after the folder unless you pass `--name`. A leading `~` means your home
folder. `--plain` can't be combined with a request, `--mode` or `--type`.

## flow status

```
flow status [<flow>] [--json] [--full] [--all]
```

Without a flow, it lists every flow, just like `flow`. Give it a flow and you get everything about
that one.

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

Fields with nothing to show are left out. `blocker` and `delegate` (what the worker handed to a
subagent) only appear while there is one, `tasks` only with task counts, and `review` and
`merge_request` once one was reported. A flow without a worktree has no `branch`, `worktree` or
`worktree_exists` (which turns false once the folder is gone); a plain session shows `folder`
instead. `answer` is the closing message of the worker's last turn.

| Option | Does |
| --- | --- |
| `--json` | print the JSON programs read instead of TOON (see [json.md](json.md)) |
| `--full` | show long texts whole; without it a text over 80 characters is cut and says so: `(truncated, 87 chars - use --full)` |
| `--all` | every project's flows, each line with its project (see [Several projects](#several-projects)) |

## flow send

```
flow send <flow> <text>
```

This is how the master tells a worker something, whether that's an answer, an instruction or a
correction. flow adds `master: <text>` to the flow's timeline and wakes the worker with it, or
hands it over once the worker's session is running again.

```
$ flow send shop-221 "Count them against the refund date."
sent: shop-221
state: waiting
help[2]: "flow wakes its worker with it now, or once its session runs again",flow wakes you when shop-221 changes again
```

It refuses to run inside a worker (`only the master sends`), since a worker asks by ending its
turn with the question. It also refuses a finished flow, a flow without a worker, a plain session,
and a text that's empty or over 4000 characters.

## flow report

```
flow report [--flow <flow>] <event>
```

Adds one event to a flow. Workers don't need it, because their harness's hooks report for them
(see [flows.md](flows.md#how-a-worker-reports)). It's for scripts, other tools and fixing a flow by
hand.

The flow comes from `--flow`, or else from `$FLOW_ID`, which flow sets in every worker. With
neither, `flow report` does nothing and still succeeds, so a skill can report and still work
outside a flow:

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
| `idle <note>` | the turn ended without a question; nothing for you to do |
| `blocked <reason>` | the flow cannot go on |
| `decision <text>` | a decision made on your behalf |
| `tasks <done> <total>` | how many of the plan's tasks are done |
| `review --fixed <n> --open <n>` | review findings fixed and still open |
| `merge-request <url>` | the merge request is open; the flow is done |
| `ended` | the worker's session ended; flow ui does not open it again |
| `heartbeat` | the worker is alive; adds no event (see `not-responding` in [flows.md](flows.md#states)) |

Each one answers with the flow's new state:

```
$ flow report --flow shop-217 blocked "Task 5: the migration needs a default value"
flow: shop-217
state: blocked
phase: implement
help[1]: Run `flow report working` once the block is resolved
```

Reporting the same state, phase, tasks or decision twice changes nothing, so retrying those is
harmless. `review`, `merge-request` and `ended` add a timeline entry every time. Texts are kept up
to 500 characters.

## flow new

```
flow new --name <name> --request <text> [--mode guided|auto]
```

Tracks a flow without a worktree or a worker, for when you run the worker yourself. If an
unfinished flow with that name already exists, you get that one back (`existing: true`).

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

Ends one flow, whatever state it's in. It closes the worker's session in flow ui, runs the
repository's `.flow/teardown`, removes the worktree, deletes the branch and forgets the flow.

```
$ flow remove shop-217
removed: shop-217
pane: none
worktree: removed
branch: deleted
help[1]: Run `flow` to see what is left
```

If the worktree has uncommitted work, or the branch has commits that aren't on any other branch,
local or pushed, it refuses and changes nothing:

```
$ flow remove shop-221
error: flow/shop-221 has 1 commit on no other branch
help[2]: "Merge or push flow/shop-221, then run `flow remove shop-221` again",Run `flow remove shop-221 --delete-unmerged` to delete it with its commits
```

| Option | Does |
| --- | --- |
| `--force` | also remove a worktree with uncommitted work, discarding it |
| `--delete-unmerged` | also delete a branch whose commits are on no other branch |

For a plain session, `flow remove` only forgets the session. The folder is never touched.

## flow clear

```
flow clear
```

Forgets every finished flow (`done` or `ended`) and tears down its worktree the way `flow remove`
does. Two things are different. A worktree with uncommitted work is kept and listed under `kept`,
and a branch is only deleted when all its commits are on another branch. A pushed branch with its
merge request stays on the remote.

```
$ flow clear
cleared: 2
names[2]: add-a-dark-mode,shop-230
kept[1]{name,path}:
  shop-230,~/projects/shop/.flow/worktrees/shop-230
help[2]: Run `flow` to see what is left,"Commit or discard the work in a kept worktree, then remove it with `git worktree remove <path>`"
```

## Commands flow runs itself

These don't show up in `flow --help`, and you shouldn't need to run them yourself.

| Command | What runs it |
| --- | --- |
| `flow server` | flow ui, when no server runs. It owns every session; its log is `server.log` (see [files.md](files.md)). |
| `flow hook claude-code`, `flow hook codex` | the hooks flow adds to each worker. It reads the hook's event on stdin and reports what changes the flow. It never prints to stdout and never fails a worker's turn; for Codex it exits 2 at Stop to hand over news. |
| `flow wait [--master]` | Claude Code sessions, as a hook that runs in the background. It sleeps until there is news for the session, prints it to stderr and exits 2, which wakes the session. `--master` waits for the master's news. |

## Output, errors and exit codes

Errors are TOON on stdout too, with next steps like everything else:

```
$ flow report --flow shop-999 working
error: no flow named shop-999
help[2]: Run `flow` to list the flows,Run `flow new --name shop-999 --request <text>` to start it
```

| Exit code | Means |
| --- | --- |
| 0 | it worked |
| 1 | an error, such as a flow that does not exist |
| 2 | a usage error, such as an unknown option; `flow wait` also exits 2 when it has news |

## Environment variables

flow reads these:

| Variable | Does |
| --- | --- |
| `FLOW_HOME` | the folder flow keeps its flows in. Default: `$XDG_STATE_HOME/flow`, else `~/.local/state/flow`. |
| `FLOW_CONFIG` | the config file. Default: `$XDG_CONFIG_HOME/flow/config.toml`, else `~/.config/flow/config.toml`. |
| `XDG_STATE_HOME`, `XDG_CONFIG_HOME`, `HOME` | where the defaults above live |
| `FLOW_ID` | the flow `flow report` and `flow hook` report into; flow sets it in every worker |
| `FLOW_MASTER` | `1` in the master's session, whose hooks report no flow |
| `CLAUDE_CONFIG_DIR` | Claude Code's folder, where flow ui reads a worker's transcript for the peek. Default: `~/.claude`. |

And it sets these in every worker's session and in the repository's setup and teardown hooks:

| Variable | Holds |
| --- | --- |
| `FLOW_ID` | the flow's id |
| `FLOW_NAME` | the flow's name |
| `FLOW_SLOT` | the flow's slot, 1 to 20 |
| `FLOW_WORKTREE` | the worktree's path |
| `FLOW_REPO` | the repository's path |

A plain session gets only `FLOW_ID`. Every session also gets `FLOW_HOME` when you set it, and
`TERM=xterm-256color` and `COLORTERM=truecolor`.

## Several projects

By default, every project shares one store, one server and one master. To keep projects apart,
give each one its own `FLOW_HOME`, for example one per tmux session:

```sh
export FLOW_HOME=~/.local/state/flow/projects/shop
```

Each store then has its own flows, server and master. Two things stay shared, in the default
folder (`$XDG_STATE_HOME/flow`, else `~/.local/state/flow`) whatever `FLOW_HOME` says. Every store
registers itself there, which is how `flow status --all` and the board's `a` key find them all.
And slots are claimed there, so no two unfinished flows in any project share a slot.

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

A project is named after its store's folder (`shop` above), and the default store is called
`default`. If a store's folder is gone, it's dropped from the list.
