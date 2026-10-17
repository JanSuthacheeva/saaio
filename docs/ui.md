# flow ui

`flow ui` is where you spend your time with flow: one terminal with the master, every worker and
every plain session. Run it in a repository:

```sh
flow ui
```

- [The screen](#the-screen)
- [Keys](#keys)
- [The peek](#the-peek)
- [The board](#the-board)
- [Starting a flow](#starting-a-flow)
- [A session that ended](#a-session-that-ended)
- [History](#history)
- [Mouse](#mouse)
- [Leaving and stopping](#leaving-and-stopping)
- [After a crash or a reboot](#after-a-crash-or-a-reboot)

## The screen

![flow ui](media/flow-ui.png)

**The pane** on the left is the session in view: the master when flow ui opens, or a worker you
jumped to. Everything you type goes to it, except the [prefix key](#keys).

**The title row** above the pane names the session in view. For a worker: its state, name, steps,
phase, branch and slot. For a plain session: its state and folder. For the master: its branch. On the
right it shows notices, such as a theme error or "starting the flow…".

**The sidebar** on the right lists every session:

- `0` is the master, with the folder and branch it runs in. It shows a spinner while the master
  works, and a waiting dot when the master ended a turn, asked something or wants permission while
  you looked at another session, until you look at it again.
- Below it, the workers and plain sessions, grouped by what they need from you:
  - **NEEDS YOU**: waiting for your answer, blocked, or not responding,
  - **READY**: the turn ended and nothing was asked (`idle`),
  - **RUNNING**: at work, or a subagent is,
  - **DONE**: finished or ended.

  Each row shows its number (`1` is the top), state, name and age, then its steps
  (`●───●───○───○`), phase and task count (or, once finished, how long it took), then what it asks
  or does. A plain session has no steps; its second line says its state, such as `idle`. Rows that
  do not fit end in `… N more`.

States are drawn as:

| Mark | State |
| --- | --- |
| spinner (`◐◓◑◒`) | working or delegated |
| `●` in the waiting color | waiting for you |
| `○` | idle: its turn ended, nothing asked |
| `●` in the blocked color | blocked or not responding |
| `✓` | done |
| `○` faint | ended |

A row can also say `exited` (the session's process ended) or `failed` (it could not start).

**The rail.** In a terminal narrower than 110 columns, the sidebar folds to a rail: just the
numbers and marks. The title row then names the flows that need you. `C-space b` folds and unfolds
it by hand.

## Keys

flow ui's keys all start with a prefix, `C-space` (Control and Space) unless `[ui] prefix` sets
another (see [configuration.md](configuration.md#ui)). Press the prefix, let go, then the key.

| Key | Does |
| --- | --- |
| `C-space 0` to `9` | jump to a session; `0` is the master, `1` the top of the sidebar |
| `C-space j`, `C-space k` | move the [peek](#the-peek) down and up over the workers |
| `C-space enter` | jump into the peeked worker |
| `C-space n` | [start a flow](#starting-a-flow) |
| `C-space o` | open the [board](#the-board) |
| `C-space b` | fold the sidebar to a rail, and back |
| `C-space r` | resume a session that exited, or a flow that ended |
| `C-space [` | scroll back through the session's [history](#history) |
| `C-space d` | detach: leave flow ui, every session keeps running |
| `C-space Q` | stop flow (see [Leaving and stopping](#leaving-and-stopping)) |
| `C-space C-space` | send `C-space` itself to the pane |
| `C-space ?` | show these keys |

The sidebar orders flows by what they need, so a flow's number changes as its state does.

## The peek

`C-space j` and `C-space k` move a cursor over the workers in the sidebar, skipping the one in view.
Under the selected worker the sidebar shows what it did lately: its latest tool calls and text, read
from the harness's own transcript, never from its screen. `C-space enter` jumps into it. Moving past
either end closes the peek.

## The board

`C-space o` opens the board: a card per session, and under them the selected flow's timeline,
decisions and steps.

| Key | Does |
| --- | --- |
| `0` to `9` | select a card |
| `h` `j` `k` `l`, arrows | move between cards |
| `enter` | jump into the selected session |
| `n` | start a flow |
| `x` | clean up the selected flow, if it is finished |
| `a` | show every project's flows, and back to this project |
| `esc`, `q`, `o` | back to the pane |

With `a`, the board lists the flows of every project (see [cli.md](cli.md#several-projects)).
Flows of other projects are shown only: `enter` and `x` work on this project's flows.

## Starting a flow

`C-space n` opens the new-flow dialog, for the repository of the session in view:

```
╭─ new flow ──────────────────────────────────────────── in ~/projects/shop ─╮
│                                                                            │
│▌ request                                                                   │
│▌ ╭──────────────────────────────────────────────────────────────────────╮  │
│▌ │                                                                      │  │
│▌ ╰──────────────────────────────────────────────────────────────────────╯  │
│                                                                            │
│  agent   (•) claude   ( ) codex                                            │
│  mode    ( ) auto   (•) guided   ( ) plain    guided stops at every phase  │
│  creates a branch, a worktree and a slot                                   │
│                                                                            │
├────────────────────────────────────────────────────────────────────────────┤
│  enter start   tab next field   esc cancel                                 │
╰────────────────────────────────────────────────────────────────────────────╯
```

| Row | Holds |
| --- | --- |
| request | what to build, such as `SHOP-217 add CSV export`; in plain mode, the folder to open |
| type | the branch type, when the repository has a [branch convention](repository.md#branch-names) with types |
| agent | the agent to run, when there is more than one |
| mode | `auto`, `guided` or `plain` (see [flows.md](flows.md#guided-and-auto)) |
| creates | what the start will create: the branch, worktree and slot it will get |

| Key | Does |
| --- | --- |
| typing | edits the request or folder |
| `backspace` | deletes a character |
| `C-w` | deletes the last word, or in plain mode the last folder |
| `C-u` | clears the text |
| `tab` | goes to the next row |
| `space`, `l`, right arrow | picks the next option in a row |
| `h`, left arrow | picks the previous option |
| `enter` | starts it, once the text is not empty |
| `esc` | cancels |

It runs `flow start` for you, and the new session appears in the sidebar. A start that fails
shows why under the request.

## A session that ended

When a worker's session ends (you typed `/exit`, or the process stopped), its pane says so and
offers:

| Key | Does |
| --- | --- |
| `x` | clean up: run `flow remove` for the flow (worktree, branch, flow) |
| `r` | resume the session |
| `esc` | go back to the master |

`x` asks before it discards uncommitted changes or commits that are on no other branch:
`y` goes ahead, any other key keeps the flow.

When the master's session ends, `C-space r` resumes it and `C-space Q` stops flow.

## History

`C-space [` scrolls back through the session's output.

| Key | Does |
| --- | --- |
| `k`, `j`, arrows | one line back, one line forward |
| `C-u`, `C-d` | half a page |
| `C-b`, `C-f` | a page |
| `g`, `G` | the oldest line, the newest |
| `q`, `esc` | back to the live session |

## Mouse

Click a session in the sidebar or rail to jump to it. Mouse events inside the pane go to the
session, so an agent's own mouse support works there.

## Leaving and stopping

flow ui is only a window onto the sessions. A server, `flow server`, owns them and keeps them
running.

- **Detach** with `C-space d`, or close the terminal: every session keeps running. The next
  `flow ui` attaches again.
- **Stop** with `C-space Q`. With unfinished flows it lists them and asks:
  - `k` keeps them: the server stops, and their sessions resume the next time you run `flow ui`,
  - `e` ends them: the server stops and the flows are marked ended; their branches and worktrees
    stay until `flow clear`,
  - `esc` keeps flow running.

  With every flow finished it asks once: `y` stops, any other key keeps it running.
- `flow ui --stop` stops the server and every session from the command line, keeping unfinished
  flows like `k`.

Only one flow ui is attached at a time: a second one takes over from the first.

## After a crash or a reboot

The next `flow ui` starts the server again and resumes the master and every unfinished worker in
its conversation (`claude --resume`, `codex resume`). After a stop, the master starts a fresh
conversation; the workers still resume.
