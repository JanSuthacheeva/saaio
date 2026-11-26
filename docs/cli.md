# The command line

- [Introduction](#introduction)
- [Commands at a glance](#commands-at-a-glance)
- [saaio](#saaio)
    - [In a terminal](#in-a-terminal)
    - [Run by an agent or a script](#run-by-an-agent-or-a-script)
    - [--stop](#--stop)
- [saaio start](#saaio-start)
    - [Stacked flows](#stacked-flows)
    - [Plain sessions](#plain-sessions)
- [saaio status](#saaio-status)
- [saaio send](#saaio-send)
    - [--type](#--type)
- [saaio report](#saaio-report)
- [saaio new](#saaio-new)
- [saaio remove](#saaio-remove)
- [saaio setup](#saaio-setup)
- [saaio clear](#saaio-clear)
- [saaio trust](#saaio-trust)
- [saaio bug-report](#saaio-bug-report)
- [saaio completions](#saaio-completions)
- [saaio update](#saaio-update)
- [Commands saaio runs itself](#commands-saaio-runs-itself)
- [Output, errors and exit codes](#output-errors-and-exit-codes)
- [Environment variables](#environment-variables)
- [Several projects](#several-projects)

## Introduction

saaio's main user is an agent, so its commands are built to be read by one. Every command prints
[TOON](https://toonformat.dev), a compact format agents read with few tokens, and ends in `help[]`
lines that suggest the next commands to run. Programs read `saaio status --json` instead, whose
shape is a contract ([json.md](json.md)).

This page lists every command and option. The output shown is from real runs, with paths
shortened:

```
$ saaio send shop-221 "Count them against the refund date."
sent: shop-221
state: waiting
help[2]: "saaio wakes its worker with it now, or once its session runs again",saaio wakes you when shop-221 changes again
```

## Commands at a glance

Most of the time, you only type `saaio` and let the master run the rest. Here is everything there
is:

| Command | Does |
| --- | --- |
| `saaio` | the terminal UI in your terminal. Every flow and what needs you for an agent or a script. |
| `saaio --stop` | stop the server and every session |
| `saaio start <request> [--name] [--mode] [--kind] [--agent] [--type] [--base]` | start a worker in its own worktree |
| `saaio start --plain <folder> [--name] [--agent]` | start an agent in a folder, without a flow |
| `saaio status [<flow>] [--json] [--full] [--all]` | where flows stand |
| `saaio send <flow> <text>` | tell a worker something |
| `saaio send --type <flow> <line>` | type a line into a worker's pane, such as `/compact` |
| `saaio report [--flow <flow>] <event>` | add an event to a flow by hand |
| `saaio new --name <name> --request <text> [--mode] [--kind]` | track a flow without starting a worker |
| `saaio remove <flow> [--force] [--delete-unmerged]` | end a flow and tear down its worktree |
| `saaio setup [<flow>]` | run a flow's `.saaio/teardown`, then its `.saaio/setup` again |
| `saaio clear` | forget finished flows and answered research flows, and tear down their worktrees |
| `saaio trust [<folder>] [--revoke] [--list]` | let saaio run a repository's `.saaio` scripts |
| `saaio bug-report` | write a file to attach to a bug report |
| `saaio completions <shell>` | print the completion script for a shell |
| `saaio update [--version <tag>]` | install the latest release over this binary |

Wherever a command takes a `<flow>`, you can give a flow's name or its id. A name means the
unfinished flow of that name, else the most recent finished one.

`saaio --version` prints the version and commit: `saaio 0.3.1 (e035b96)`, a pre-release with its
suffix (`saaio 0.3.1-rc.1 (95aec22)`), or `unknown` for the commit of a build from source outside
a git checkout of saaio ([install.md](install.md#from-source)). Every command takes `--help`.

## saaio

Bare `saaio` does what fits whoever runs it: it opens the UI for you, and prints every flow for an
agent:

```
saaio
saaio --stop
```

### In a terminal

Typed in your terminal, `saaio` opens the terminal UI ([ui.md](ui.md)), and starts the session
server if none runs. When you leave, it prints what happened, such as
``detached - the agents keep running; run `saaio` to attach again``. A broken `config.toml` stops
it before the server starts.

### Run by an agent or a script

saaio counts as run by an agent or a script when stdin or stdout is not a terminal, or when
`SAAIO_FLOW_ID`, `SAAIO_MASTER=1`, `CLAUDECODE` or `CODEX_THREAD_ID` is set. The UI never opens
there, so a worker or the master can run `saaio` safely. Instead, it prints the flow counts, one
line per flow, and the question or reason of each waiting or blocked flow:

```
$ saaio
bin: ~/.local/bin/saaio
description: "Tracks parallel feature flows: what each is doing, where it is blocked, how far it got"
count: 3
needs_you: 2
flows[3]{name,kind,state,phase,tasks,age,services,turn}:
  shop-221,feature,waiting,plan,"",2s,"",""
  shop-217,fix,blocked,implement,4/9,2s,"",""
  scratch,"",idle,"","",11s,"",""
blockers[2]{name,blocker}:
  shop-221,Should refunds count against the original order's totals or the refund date's?
  shop-217,"Task 5: the migration needs a default value"
help[2]: "Run `saaio status <name>` for one flow's phases, timeline and decisions",Run `saaio new --name <name> --request <text>` to track a new flow
```

The flows are ordered as in the sidebar: needs you, ready (turn ended, nothing asked), running,
finished, with the most recent change first within each group.

- `kind`: `feature`, `fix`, `research` or one of the config's; empty for a plain session and a
  flow from before kinds.
- `age`: time since the flow's state, phase or blocker last changed.
- No flows: `flows: none tracked`. A log that cannot be read adds a `warnings[]` line.
- `services`: empty while the flow's services run or were never paused; else `pausing`, `paused`,
  `resuming`, `pause-failed` or `resume-failed` ([repository.md](repository.md#pause-and-resume)).
- `turn`: `waiting` while a heavy command of the flow waits for another worker's run, else empty
  ([repository.md](repository.md#heavy-commands)).

### --stop

`saaio --stop` stops the server and every session, typed in a terminal or run by an agent or a
script. Unfinished flows are kept, and resume the next time `saaio` opens the UI. It prints
`stopped the saaio server and every session in it`, or `no saaio server runs`.

> [!NOTE]
> Inside a worker, `--stop` is refused on the worker's own store, since it would end your
> sessions. Set `SAAIO_HOME` to another folder to stop a server there.

## saaio start

```
saaio start <request> [--name <name>] [--mode guided|auto] [--kind <kind>] [--agent <agent>] [--type <type>] [--base <branch>]
saaio start --plain <folder> [--name <name>] [--agent <agent>]
```

`saaio start` begins a flow. Run it in the repository the flow works on, and it takes these
steps:

1. refuses a repository not [trusted](repository.md#trust) yet when it has `.saaio` scripts or the
   agent is Codex
2. picks the kind and names the flow
3. claims a slot (1 to 20, [repository.md](repository.md#slots-and-ports))
4. creates a git worktree in `.saaio/worktrees/<name>` on a new branch, from what the repository
   has checked out or from `--base`; for a research flow detached there, without a branch
5. runs the repository's `.saaio/setup` there, if present, for at most 10 minutes
6. records the flow, so the UI opens the agent in the worktree

```
$ saaio start "SHOP-217 add CSV export"
flow: shop-217
id: e514f1de-604d-4dbc-9b88-2cd76333d0a4
kind: feature
worktree: ~/projects/shop/.saaio/worktrees/shop-217
branch: flow/shop-217
slot: 1
setup: none
agent: claude
pane: when saaio opens
help[2]: Run `saaio` in a terminal to open the worker,Run `saaio` for all flows
```

| Field | Values |
| --- | --- |
| `setup` | `ran`, `none` (no executable `.saaio/setup`), `failed: <its last line> (full output: <log>)`. A failed setup marks the flow blocked and stays in `saaio status` until a run passes. The worker still opens and hears of it ([repository.md](repository.md#when-setup-or-teardown-fails)). |
| `pane` | `opening` while the UI runs, else `when saaio opens` |
| `skill` | only when the agent is Claude Code and the kind's skill is not installed: `<skill> not found, the worker got saaio's built-in prompt`, with a `help` line on how to change that ([configuration.md](configuration.md#without-matching-skills)) |
| `branch` | left out for a research flow, which says it has none in `help` |

| Option | Default | Effect |
| --- | --- | --- |
| `<request>` | | what to build, such as `"SHOP-217 add CSV export"`. Up to 20000 characters. Part of the agent's first prompt. |
| `--name <name>` | from the request | the flow's name, lowercased, with anything but letters and digits turned into `-` (`Skills Reporting` becomes `skills-reporting`). Default: the ticket key at the start of the request, lowercased (`SHOP-217` becomes `shop-217`), else the first four words (`add-a-dark-mode`). A name held by an unfinished flow, given or not, gets `-2`, `-3` and so on. |
| `--mode <mode>` | `guided` | `guided` or `auto`: who answers the worker's questions ([flows.md](flows.md#guided-and-auto)) |
| `--kind <kind>` | `[worker] kind`, else `feature` | `feature`, `fix`, `research` or a kind from the config: the flow's phases, first prompt and branch ([flows.md](flows.md#kinds)). An unknown kind is refused with the list. |
| `--agent <agent>` | `[worker] agent`, else `claude` | an agent from the config ([configuration.md](configuration.md#agents)) |
| `--type <type>` | the first of the kind's types the repository lists, else its first | a branch type from the repository's `.saaio/config.toml` ([repository.md](repository.md#branch-names)). Refused for a research flow, which has no branch. |
| `--base <branch>` | what the repository has checked out | the branch the worktree starts from and the merge request targets ([Stacked flows](#stacked-flows)) |
| `--plain <folder>` | | open the agent in this folder as a plain session |

The branch is `flow/<name>` unless the repository sets a convention. An existing branch gets `-2`,
`-3` and so on.

### Stacked flows

With `--base <branch>`, the worktree starts from that branch instead of what the repository has
checked out, so each merge request of a stack runs as a flow of its own. The base is often another
flow's branch:

```
$ saaio start "SHOP-218 export button" --base flow/shop-217
flow: shop-218
id: 9c1d53a0-2f4e-4b8e-a1c3-5d2e7f9b6a10
worktree: ~/projects/shop/.saaio/worktrees/shop-218
branch: flow/shop-218
base: flow/shop-217
slot: 2
setup: none
agent: claude
pane: when saaio opens
help[2]: Run `saaio` in a terminal to open the worker,Run `saaio` for all flows
```

- A local branch is used as it is, without a fetch. A branch only a remote has is fetched, from
  `origin` first, then the other remotes.
- The flow's branch does not track its base, so `git pull` and `git push` never touch the base.
- The worker is told its merge request targets the base, and gets `SAAIO_BASE`.
- Refused before anything is created: a name git takes for no branch, and a branch neither the
  repository nor a remote has (`no branch <base> here or on a remote`).
- Not combinable with `--plain`.
- When the base's merge request merges, the flow moves onto the branch it merged into, and its
  worker is told to rebase and point its merge request there
  ([flows.md](flows.md#stacked-flows)).

### Plain sessions

With `--plain <folder>`, saaio opens an agent in any folder, repository or not, with no worktree,
branch, slot or first prompt. The master ignores it ([flows.md](flows.md#plain-sessions)):

```
$ saaio start --plain ~/projects/scratch
flow: scratch
id: 5418e55e-581e-4117-a46c-0824b8e475d9
folder: ~/projects/scratch
agent: claude
pane: when saaio opens
help[2]: Run `saaio` in a terminal to open the session,Run `saaio` for all flows
```

The session is named after the folder, unless you pass `--name`, and a leading `~` is the home
folder. `--plain` is not combinable with a request, `--mode`, `--kind`, `--type` or `--base`.

## saaio status

```
saaio status [<flow>] [--json] [--full] [--all]
```

`saaio status` tells you where flows stand. Without a flow, it prints every flow, as `saaio` does.
With a flow, it prints its request, mode, state, phases with time spent, tasks, decisions and
timeline:

```
$ saaio status shop-217
name: shop-217
id: e514f1de-604d-4dbc-9b88-2cd76333d0a4
request: SHOP-217 add CSV export
mode: guided
state: blocked
phase: implement
blocker: "Task 5: the migration needs a default value"
tasks: 4/9
branch: flow/shop-217
worktree: ~/projects/shop/.saaio/worktrees/shop-217
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
help[2]: Run `saaio report --flow shop-217 working` once the block is resolved,Run `saaio` for all flows
```

Some fields show only when they apply:

| Field | Shown |
| --- | --- |
| `blocker` | while there is one |
| `setup` | while the flow's last setup run failed: `failed: <its last line> (full output: <log>)`, in place of a `blocker` that says the same. The list shows it among the `blockers` until the flow ends, whatever its state. |
| `questions`, `options` | while the worker waits on a question dialog: each question with its header and whether it takes `one` option or `several`, then each option's label and description by question number |
| `delegate` | while a subagent works (what the worker handed to it) |
| `tasks` | with task counts |
| `review`, `merge_request` | once reported |
| `merged` | once the merge request is merged |
| `branch`, `worktree`, `worktree_exists` | for a flow with a worktree. `worktree_exists` is false once the folder is gone. |
| `base` | for a [stacked flow](#stacked-flows): the branch it stands on, after `branch` |
| `folder` | for a plain session |
| `answer` | the closing message of the worker's last turn |
| `services`, `services_error` | while the flow's services are not running: `paused`, `resume-failed`, ..., and the last line a failed pause or resume printed |
| `turn` | while a heavy command waits for another worker's run: `waits to run pint after shop-42` |

A Claude Code worker that asks with a question dialog (AskUserQuestion) shows its questions and
options:

```
state: waiting
blocker: Which color?
questions[2]{n,header,question,pick}:
  1,Color,Which color?,one
  2,Toppings,Which toppings?,several
options[6]{question,n,label,description}:
  1,1,Red,"A warm, bold color"
  1,2,Green,"A calm, natural color"
  1,3,Blue,"A cool, steady color"
  2,1,Cheese,Melted cheese
  2,2,Ham,Sliced ham
  2,3,Olives,Black olives
```

| Option | Effect |
| --- | --- |
| `--json` | the JSON for programs instead of TOON ([json.md](json.md)) |
| `--full` | long texts in full. Without it, a text over 80 characters is cut and marked `(truncated, 87 chars - use --full)`. |
| `--all` | every project's flows, each line with its project ([Several projects](#several-projects)) |

## saaio send

```
saaio send <flow> <text>
```

`saaio send` is how the master talks to a worker: an answer, an instruction or a correction. It
adds `master: <text>` to the timeline and wakes the worker, or delivers the message when the
worker's session runs again:

```
$ saaio send shop-221 "Count them against the refund date."
sent: shop-221
state: waiting
help[2]: "saaio wakes its worker with it now, or once its session runs again",saaio wakes you when shop-221 changes again
```

A worker waiting on a question dialog cannot read a message while the dialog is open. Answer
every question in one message, by option label or in words. saaio's server then presses Esc in the
worker's pane, after ten seconds without typing there, so the dialog closes and the worker reads
the message as its answer. saaio presses Esc once per dialog: one still open after it waits for an
answer in the worker's pane. Answered in the pane first, the dialog is left alone. The message
waits while the UI is closed.

```
$ saaio send shop-217 "The user says: Blue; Cheese and Olives"
sent: shop-217
state: waiting
help[2]: saaio closes shop-217's question dialog and its worker reads this as the answer to every question in it,saaio wakes you when shop-217 changes again
```

A worker waiting on a permission prompt reads the message only once the prompt is answered in its
pane. `saaio send` still takes the message and names the prompt in `prompt`. The prompt waits for
you in the worker's pane: saaio does not answer it.

```
$ saaio send shop-217 "Check the date header too."
sent: shop-217
state: waiting
prompt: "permission: Bash(curl -sI https://example.org)"
help[2]: shop-217's permission prompt is open in its pane. Its worker reads this once the user answers the prompt there,saaio wakes you when shop-217 changes again
```

| Refused | Reason |
| --- | --- |
| outside the master's session | `only the master sends`. Workers end their turn with the question. |
| an ended flow, a flow without a worker, a plain session | no worker to receive it. An open PR alone does not stop sends. |
| an empty text, a text over 4000 characters | |

### --type

```
saaio send --type <flow> <line>
```

A message reaches the model as words, so a slash command sent with `saaio send` does not run.
`--type` types the line and Enter into the worker's pane instead, as if the user typed it there,
so a slash command or a skill runs: `/compact`, `/review`. It adds `master typed: <line>` to the
timeline:

```
$ saaio send --type shop-221 /compact
queued: shop-221
line: /compact
ahead: 0
help[2]: saaio types it into shop-221's pane once its session is idle,"Queue the next line with another `saaio send --type shop-221 \"<line>\"`. It waits for this one's turn to end"
```

- The line waits in the flow's log until saaio's server types it. `ahead` counts the lines queued
  before it.
- One line per call. Lines are typed in the order they were queued, each once the turn the line
  before it began has ended.
- A line is typed only into an idle session, after the session's first turn, and after ten seconds
  without typing in that pane.
- A line that begins no turn (a command that only opens a dialog, such as `/cost`) holds the next
  line for 30 seconds.
- After an Esc in a Claude Code worker, its lines are typed once it is idle, as after a turn's
  end; with a Claude Code older than 2.1.295, which does not report an Esc, they wait until the
  next prompt typed in that pane ends its turn.
- Lines wait while the UI is closed and are typed once it opens again.

To compact a worker and then let it go on, queue two lines:

```sh
saaio send --type shop-221 /compact
saaio send --type shop-221 "continue shipping"
```

| Refused | Reason |
| --- | --- |
| a line with a newline | `a typed line is one line`. Queue one call per line. |
| an agent without an adapter | it reports no turns, so saaio cannot tell when it is idle |
| everything `saaio send` refuses | |

## saaio report

```
saaio report [--flow <flow>] <event>
```

Workers report through their hooks ([flows.md](flows.md#how-a-worker-reports)), so you rarely need
`saaio report`. It adds one event to a flow, for scripts, other tools and manual fixes.

The flow is `--flow`, else `$SAAIO_FLOW_ID`, which is set in every worker. With neither, it does
nothing and succeeds, so a skill can report outside a flow:

```
$ saaio report working
skipped: no flow id
help[1]: Set SAAIO_FLOW_ID or pass --flow <name>
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
| `merge-request <url>` | the merge request is open. The flow shows `done` until its worker reports more activity; the session remains reachable. saaio keeps it as `.../pull/<n>` or `.../-/merge_requests/<n>`, without a trailing `/`, `?query`, `#fragment` or a page such as `/files`. Any other URL, or a merge request of another repository than any of the flow's remotes, is refused. |
| `ended` | the worker's session ended. The UI does not reopen it. |
| `heartbeat` | the worker is alive. Adds no event (see `not-responding` in [flows.md](flows.md#states)). |

Each event answers with the flow's new state:

```
$ saaio report --flow shop-217 blocked "Task 5: the migration needs a default value"
flow: shop-217
state: blocked
phase: implement
help[1]: Run `saaio report working` once the block is resolved
```

The same state, phase, tasks or decision reported twice changes nothing, so retries are safe.
`review`, `merge-request` and `ended` add a timeline entry every time. Texts are kept up to 500
characters.

## saaio new

```
saaio new --name <name> --request <text> [--mode guided|auto] [--kind <kind>]
```

`saaio new` tracks a flow without a worktree or worker, for a worker you run by hand. An existing
unfinished flow of that name is returned (`existing: true`). The request: up to 20000 characters,
as for `saaio start`. `--kind` records the kind's phases, as `saaio start` does:

```
$ saaio new --name shop-230 --request "SHOP-230 tracked by hand"
flow: shop-230
id: a825cdf3-a951-4333-bae2-e0c872be66bf
kind: feature
existing: false
help[2]: Run `saaio report --flow shop-230 phase plan` when plan starts,Or set SAAIO_FLOW_ID=a825cdf3-a951-4333-bae2-e0c872be66bf in the worker's environment and leave out --flow
```

## saaio remove

```
saaio remove <flow> [--force] [--delete-unmerged]
```

`saaio remove` ends one flow in any state. It closes the worker's session in the UI, runs
`.saaio/teardown`, removes the worktree, deletes the branch and forgets the flow:

```
$ saaio remove shop-217
removed: shop-217
pane: none
teardown: none
worktree: removed
branch: deleted
help[1]: Run `saaio` to see what is left
```

Your work is safe with it, though. `saaio remove` refuses and changes nothing when the worktree
has uncommitted work, git cannot check it (git does not list it as a worktree of the repository),
or the branch has commits on no other branch, local or pushed:

```
$ saaio remove shop-221
error: flow/shop-221 has 1 commit on no other branch
refusal: unmerged
help[2]: "Merge or push flow/shop-221, then run `saaio remove shop-221` again",Run `saaio remove shop-221 --delete-unmerged` to delete it with its commits
```

Its output fields:

| Field | Values |
| --- | --- |
| `pane` | `closed`, `none` or `timed out` |
| `teardown` | `ran`, `none` (no worktree or no executable `.saaio/teardown`), `failed: <its last line>`, or `not run, the repository is not trusted` |
| `worktree` | `removed`, `none` or `kept: <path>`, with a help line to discard what is left |
| `branch` | `deleted`, `kept` or `none` |
| `stacked[n]` | the unfinished flows that stand on its branch ([Stacked flows](#stacked-flows)), with a help line. Left out without any. |

A flow's removal is never refused for the flows that stand on it. They keep their commits: a base
whose commits all live on in a stacked branch is not unmerged, so its branch is deleted. Its
refusal for unmerged commits names them too. When saaio saw the base merge, `saaio remove` and
`saaio clear` move the flows on it onto the branch it merged into first
([flows.md](flows.md#stacked-flows)).

| Option | Effect |
| --- | --- |
| `--force` | also remove a worktree with uncommitted work, discarding it, or one git cannot check |
| `--delete-unmerged` | also delete a branch whose commits are on no other branch |

For a plain session, only the session is forgotten. The folder is never touched.

## saaio setup

```
saaio setup [<flow>]
```

`saaio setup` runs a flow's `.saaio/teardown`, then its `.saaio/setup` again, in its worktree and
with its slot, as `saaio start` ran them. Run it once you fixed what made setup fail; the flow
keeps its worker, worktree and branch. Without a `<flow>` it takes the flow of the worker that runs
it, so a worker can set up its own worktree again:

```
$ saaio setup shop-240
flow: shop-240
teardown: ran
setup: ran
log: ~/.local/state/saaio/projects/shop/hooks/0b6e5c4a-.../setup.log
help[1]: Run `saaio status shop-240` to see the flow
```

A setup that passes clears the failure from `saaio status`, and a flow still blocked on it is
`idle` again. One that fails again exits 1, and its error names the file with the whole output.
It blocks the flow unless its worker is in the middle of a turn, as when the worker ran it: then
the flow stays `working` and the failure shows in `saaio status` and the timeline:

```
$ saaio setup shop-240
error: "setup failed: Please run composer update (full output: ~/.local/state/saaio/projects/shop/hooks/0b6e5c4a-.../setup.log)"
help[2]: Read its whole output in the log,Run `saaio setup shop-240` again once the cause is fixed
```

A teardown that fails is named in the help and the setup runs anyway. `saaio setup` never retries
on its own, and it refuses a flow that has ended, a plain session, a flow whose worktree is gone,
a repository with no `.saaio/setup`, and one you have not [trusted](repository.md#trust).

| Field | Values |
| --- | --- |
| `teardown` | as in [saaio remove](#saaio-remove) |
| `setup` | `ran` |
| `log` | the file that holds the setup's whole output ([files.md](files.md)) |

## saaio clear

```
saaio clear
```

`saaio clear` cleans up after finished work. It forgets every ended flow, and every research flow
that answered (`done`), and tears down its worktree like `saaio remove`, except:

- a worktree with uncommitted work is kept and listed under `kept`
- a flow whose teardown did not run, since the repository is no longer trusted, is listed under
  `untrusted`
- a branch is deleted only when all its commits are on another branch
- a pushed branch with its merge request stays on the remote

```
$ saaio clear
cleared: 2
names[2]: add-a-dark-mode,shop-230
kept[1]{name,path}:
  shop-230,~/projects/shop/.saaio/worktrees/shop-230
help[2]: Run `saaio` to see what is left,"Commit or discard the work in a kept worktree, then remove it with `git worktree remove <path>`"
```

## saaio trust

```
saaio trust [<folder>] [--revoke]
saaio trust --list
```

`saaio trust` trusts the repository `<folder>` is in (default: the current folder), so saaio runs
its `.saaio` scripts and lets Codex workers trust its worktrees ([repository.md](repository.md#trust)).
Only a person trusts: run without a terminal, or in an agent's or a worker's shell, it refuses.

```
$ saaio trust
trusted: ~/projects/shop
scripts[2]: .saaio/setup,.saaio/teardown
help[1]: Run `saaio trust --revoke ~/projects/shop` to take it back
```

| Option | Effect |
| --- | --- |
| `--revoke` | takes the trust back: `revoked: <repository>`, or `revoked: none` when it was not trusted. Works anywhere, also on a folder deleted since (pass the path `--list` shows). |
| `--list` | every trusted repository |

## saaio bug-report

```
saaio bug-report
```

`saaio bug-report` writes `saaio-report-<date>.txt` in `reports/` of the shared folder
([files.md](files.md#the-shared-folder)), or `saaio-report-<date>-2.txt` and on when that name is
taken, readable only by you, never into the current folder or a repository: the
versions of saaio, the OS, `claude`, `codex`, `gh` and `glab`, the store's path, whether
`config.toml` and `theme.toml` load, and the last 300 lines of `server.log` and `server.log.1`.
The home folder is written `~`, also as the real path a linked home leads to and in Claude Code's
project names (`-Users-you-...`). Works without a store, and then says why there is none. Without
`HOME` and `XDG_STATE_HOME` it fails: `cannot find a state directory`.

```
$ saaio bug-report
report: /Users/jan/.local/state/saaio/reports/saaio-report-2026-10-18.txt
help[2]: "Check the file before uploading it: it can hold flow names, requests and paths",Attach it from there; it is never written into a repository
```

How to report a bug: [troubleshooting.md](troubleshooting.md#reporting-a-bug).

## saaio completions

```
saaio completions <shell>
```

`saaio completions` prints the completion script for `bash`, `zsh`, `fish`, `elvish` or
`powershell` on stdout, as the shell reads it, not as TOON. It covers every command `saaio --help`
lists, with their options and values. Works without a store. Where each shell keeps it:
[install.md](install.md#shell-completions).

```
$ saaio completions tcsh
error: invalid value 'tcsh' for '<SHELL>'; one of bash | elvish | fish | powershell | zsh
help[1]: Run `saaio completions --help` for the usage
```

## saaio update

```
saaio update [--version <tag>]
```

`saaio update` installs the latest release, or the tag `--version` names (a pre-release too), over
the `saaio` being run, in the folder it is in. It runs that release's `install.sh`, which checks the
download against `SHA256SUMS` and renames the new binary over the old one. Works without a store.

```
$ saaio update
updated: saaio 0.3.0 -> 0.3.1
binary: ~/.local/bin/saaio
shadowed: ~/.cargo/bin/saaio (saaio 0.2.1 (9b77d8c))
servers[1]{store,pid,build,binary}:
  ~/.local/state/saaio/projects/shop,4321,0.3.0 (45630ee),~/.local/bin/saaio
help[3]: "`saaio` runs ~/.cargo/bin/saaio, not ~/.local/bin/saaio: remove it (`cargo uninstall saaio`) or put ~/.local/bin first on PATH","Run `SAAIO_HOME=~/.local/state/saaio/projects/shop saaio --stop`, then start saaio again; unfinished flows resume","Changes: https://github.com/JanSuthacheeva/saaio/releases/tag/v0.3.1"
```

- Already on the latest release (or a newer pre-release), it changes nothing and says so.
  `--version` installs the tag it names even when it is older.
- The latest release never is a pre-release. Install one with `--version v0.3.1-rc.1`.
- A build from source (`cargo install`) is not replaced. It names the `cargo install` line for the
  new tag instead.
- A folder you cannot write, such as `/usr/local/bin`, is not touched. It names the `sudo` line
  that runs `install.sh` there.
- The running server and the UI attached to it keep the old version until `saaio --stop`.
  `servers[]` lists every running server, of any registered store, that is not on the version
  just installed (or, when nothing was newer, the one running), with its store, pid, build and
  the binary it started from. A server of an earlier protocol shows `an earlier protocol`.
- `shadowed` names another `saaio` that comes first on PATH and is not that version: typing
  `saaio` runs it, not the one just updated. Remove it or reorder PATH.

```
$ saaio update
error: "this saaio was built from source, so saaio update does not replace it"
help[2]: "Run `cargo install --locked --git https://github.com/JanSuthacheeva/saaio --tag v0.1.1 --force`","Or install the release binary instead: docs/install.md"
```

## Commands saaio runs itself

saaio runs a few commands of its own, behind the scenes. They are not listed in `saaio --help`,
and are not for manual use:

| Command | Run by |
| --- | --- |
| `saaio server` | `saaio`, when it opens the UI and no server runs. Owns every session. Log: `server.log` ([files.md](files.md)). |
| `saaio hook claude-code`, `saaio hook codex` | the hooks saaio adds to each worker. Reads the hook's event on stdin (an event over 16 MiB is skipped) and reports what changes the flow. Never prints to stdout, never fails a worker's turn. Its errors go to `server.log`. For Codex, exits 2 at Stop to hand over news. |
| `saaio turn` | `saaio hook`, once a worker's heavy command took the project's turn. Keeps it while the run lasts, then exits, which frees it. Says why in `server.log`. |
| `saaio wait [--master]` | Claude Code sessions, as a background hook. Sleeps until there is news for the session, prints it to stderr and exits 2, which wakes the session. `--master` waits for the master's news. |

## Output, errors and exit codes

Errors are TOON on stdout too, with next steps, so an agent knows what to do about them:

```
$ saaio report --flow shop-999 working
error: no flow named shop-999
help[2]: Run `saaio` to list the flows,Run `saaio new --name shop-999 --request <text>` to start it
```

The exit code tells a script how it went:

| Exit code | Means |
| --- | --- |
| 0 | success |
| 1 | an error, such as a flow that does not exist |
| 2 | a usage error, such as an unknown option. `saaio wait` also exits 2 when it has news. |

When saaio refuses to do something that only a person may allow, the error says which refusal it
is in a `refusal` line between `error` and `help`. A program reads that line, not the wording of
the message, which may change. The values are stable:

| `refusal` | Means | Allowed by |
| --- | --- | --- |
| `uncommitted` | `saaio remove`: the worktree has uncommitted work | `--force` |
| `unmerged` | `saaio remove`: the branch has commits on no other branch | `--delete-unmerged` |
| `untrusted` | `saaio start`: the repository is not trusted to run its `.saaio` scripts | `saaio trust`, in the person's terminal |

Other errors have no `refusal` line.

## Environment variables

saaio reads these variables:

| Variable | Effect |
| --- | --- |
| `SAAIO_HOME` | the folder saaio keeps its flows in. Default: the store of the git repository saaio runs in, else the shared folder (`$XDG_STATE_HOME/saaio`, else `~/.local/state/saaio`). [files.md](files.md#the-store) |
| `SAAIO_CONFIG` | the config file. Default: `$XDG_CONFIG_HOME/saaio/config.toml`, else `~/.config/saaio/config.toml`. |
| `XDG_STATE_HOME`, `XDG_CONFIG_HOME`, `HOME` | base of the defaults above |
| `SAAIO_FLOW_ID` | the flow `saaio report`, `saaio hook` and `saaio wait` act for. Set in every worker. While it is set, `saaio` prints the flows instead of opening the UI. |
| `SAAIO_MASTER` | `1` in the master's session, whose hooks report no flow |
| `SAAIO_PROOF` | the key the server hands each session it opens. `saaio hook` and `saaio wait` act for `SAAIO_FLOW_ID` or `SAAIO_MASTER` only with that session's key, and `saaio send` only with the master's. |
| `XDG_RUNTIME_DIR`, `TMPDIR` | where the socket goes when the store's path is too long for one ([files.md](files.md#the-store)) |
| `CLAUDECODE`, `CODEX_THREAD_ID` | set by Claude Code or Codex. `saaio` prints the flows instead of opening the UI. |
| `SSH_CONNECTION`, `SSH_TTY`, `WAYLAND_DISPLAY`, `DISPLAY` | how the UI copies: OSC 52 only over SSH, else also `pbcopy`, `wl-copy` or `xclip` ([ui.md](ui.md#copying)) |
| `TERM`, `TERM_PROGRAM`, `VTE_VERSION`, `ALACRITTY_WINDOW_ID` | which sequence the UI's desktop notifications use: OSC 9, OSC 777 or the bell ([ui.md](ui.md#notifications)) |
| `TMUX` | the UI runs inside tmux: it asks tmux which terminal is attached and sends its notifications through tmux's passthrough |
| `CLAUDE_CONFIG_DIR` | Claude Code's folder, where the UI reads a worker's transcript for the peek. Default: `~/.claude`. |
| `CODEX_HOME` | Codex's folder. The UI reads a Codex worker's transcript for the peek only from its `sessions` folder. Default: `~/.codex`. |
| `SAAIO_RELEASES_URL` | a mirror of the [releases page](https://github.com/JanSuthacheeva/saaio/releases), laid out like it (`latest/download/VERSION`, `download/<tag>/...`; `file://` works). Read by the UI's update check and `saaio update`. |

saaio sets these in every worker's session, and in the setup, teardown, pause and resume hooks:

| Variable | Value |
| --- | --- |
| `SAAIO_FLOW_ID` | the flow's id |
| `SAAIO_FLOW_NAME` | the flow's name |
| `SAAIO_SLOT` | the flow's slot, 1 to 20 |
| `SAAIO_WORKTREE` | the worktree's path |
| `SAAIO_REPO` | the repository's path |
| `SAAIO_BASE` | the branch a stacked flow stands on ([Stacked flows](#stacked-flows)). Unset without `--base`. |

- A plain session gets only `SAAIO_FLOW_ID`.
- Every session, the master's too, gets its own `SAAIO_PROOF`. Hooks get none.
- Every session also gets `SAAIO_HOME` (the store of the UI that opened it), `TERM=xterm-256color`
  and `COLORTERM=truecolor`.

## Several projects

saaio keeps one store per git repository, picked from the folder it runs in
([files.md](files.md#the-store)). Each store has its own flows, server and master, so you run
`saaio` in a repository for its sidebar and master. Outside a repository, saaio uses the shared
folder.

`SAAIO_HOME` overrides the choice, for example to keep several repositories in one store:

```sh
export SAAIO_HOME=~/.local/state/saaio-work
```

What ties the stores together lives in the shared folder (`$XDG_STATE_HOME/saaio`, else
`~/.local/state/saaio`), regardless of `SAAIO_HOME`
([files.md](files.md#the-shared-folder)):

| Shared | Purpose |
| --- | --- |
| store registry | `saaio status --all` and the board's `a` key find every store |
| slots | no two unfinished flows in any project share a slot |

So `saaio status --all` shows every project's flows at once:

```
$ saaio status --all
count: 3
needs_you: 2
flows[3]{project,name,kind,state,phase,tasks,age,services,turn}:
  shop,shop-221,feature,waiting,plan,"",2s,"",""
  shop,shop-217,fix,blocked,implement,4/9,2s,"",""
  shop,scratch,"",idle,"","",11s,"",""
blockers[2]{project,name,blocker}:
  shop,shop-221,Should refunds count against the original order's totals or the refund date's?
  shop,shop-217,"Task 5: the migration needs a default value"
help[2]: "Run `saaio status <name>` in a flow's project for its phases, timeline and decisions",Run `saaio status --all --json` for every project's board
```

- A project is named after its store's folder (`shop` above). The shared folder's store is `default`.
  A second repository of the same name has the store `<name>-2` and shows so. Two stores in folders
  of the same name, such as two `SAAIO_HOME`s, show as `<name>` and `<name> (2)`.
- Projects without flows are left out.
- A store whose folder is gone is dropped.
