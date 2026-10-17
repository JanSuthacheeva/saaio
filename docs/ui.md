# flow ui

Most of your time with flow is spent here. Run it in a repository:

```sh
flow ui
```

You get one terminal with the master, every worker and every plain session in it.

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

The big pane on the left is the session you're looking at. When flow ui opens that's the master,
until you jump to a worker. Everything you type goes to that session, except the
[prefix key](#keys).

Above the pane, the title row names the session. For a worker it shows the state, name, steps,
phase, branch and slot. For a plain session it shows the state and folder, and for the master its
branch (plus its folder while the sidebar is folded). Notices show up on the right of that row,
such as a theme error or "starting the flow…".

The sidebar on the right lists every session. Row `0` is the master, with the folder and branch it
runs in. It shows a spinner while the master works. If the master finishes a turn, asks something
or wants permission while you're looking elsewhere, it gets a waiting dot until you look at it
again.

Below the master come the workers and plain sessions, grouped by what they need from you:

- **NEEDS YOU**: waiting for your answer, blocked, or not responding
- **READY**: the turn ended and nothing was asked (`idle`)
- **RUNNING**: at work, itself or through a subagent
- **DONE**: finished or ended

Each row starts with its number (`1` is the top), state, name and age. Under that come its steps
(`●───●───○───○`), phase and task count, or how long it took once it's finished. The last line is
what it's asking or doing. A plain session has no steps, so its second line just says its state,
such as `idle`. When there are more rows than fit, the list ends in `… N more`.

The mark in front of each row shows the state:

| Mark | State |
| --- | --- |
| spinner (`◐◓◑◒`) | working or delegated |
| `●` in the waiting color | waiting for you |
| `○` | idle: its turn ended, nothing asked |
| `●` in the blocked color | blocked or not responding |
| `✓` | done |
| `○` faint | ended |

A row can also say `exited`, when the session's process ended, or `failed`, when it couldn't start.

In a terminal narrower than 110 columns the sidebar folds into a rail, just the numbers and marks.
The title row then names the first flow that needs you and isn't in view. `C-space b` folds and
unfolds it by hand.

## Keys

Every flow ui key starts with a prefix, `C-space` (Control and Space) unless you set another with
`[ui] prefix` (see [configuration.md](configuration.md#ui)). Press the prefix, let go, then press
the key.

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

Since the sidebar sorts flows by what they need, a flow's number changes when its state does.

## The peek

The peek lets you check on a worker without leaving the session you're in. `C-space j` and
`C-space k` move a cursor over the workers in the sidebar, skipping the one in view. Under the
selected worker, the sidebar shows what it did lately: its latest tool calls and text, read from
the harness's own transcript rather than from its screen. `C-space enter` jumps into it, and moving
past either end closes the peek.

## The board

`C-space o` opens the board. It has a card per session, and under the cards the selected flow's
timeline, decisions and steps.

| Key | Does |
| --- | --- |
| `0` to `9` | select a card |
| `h` `j` `k` `l`, arrows | move between cards |
| `enter` | jump into the selected session |
| `n` | start a flow |
| `x` | clean up the selected flow, if it's finished |
| `a` | show every project's flows, and back to this project |
| `esc`, `q`, `o` | back to the pane |

With `a`, the board lists the flows of every project (see [cli.md](cli.md#several-projects)). You
can look at other projects' flows, but `enter` and `x` only work on this project's.

## Starting a flow

`C-space n` opens the new-flow dialog for the repository of the session in view:

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

Type what you want built, pick the options and press `enter`. The dialog runs `flow start` for you
and the new session appears in the sidebar. If the start fails, the reason shows under the request.

| Row | Holds |
| --- | --- |
| request | what to build, such as `SHOP-217 add CSV export`; in plain mode, the folder to open |
| type | the branch type, when the repository has a [branch convention](repository.md#branch-names) with types |
| agent | the agent to run, when there's more than one |
| mode | `auto`, `guided` or `plain` (see [flows.md](flows.md#guided-and-auto)) |
| creates | what the start will create: the branch, worktree and slot it'll get |

| Key | Does |
| --- | --- |
| typing | edits the request or folder |
| `backspace` | deletes a character |
| `C-w` | deletes the last word, or in plain mode the last folder |
| `C-u` | clears the text |
| `tab` | goes to the next row |
| `space`, `l`, right arrow | picks the next option in a row |
| `h`, left arrow | picks the previous option |
| `enter` | starts it, once the text isn't empty |
| `esc` | cancels |

## A session that ended

When a worker's session ends, because you typed `/exit` or the process stopped, its pane says so
and offers three keys:

| Key | Does |
| --- | --- |
| `x` | clean up: run `flow remove` for the flow (worktree, branch, flow) |
| `r` | resume the session |
| `esc` | go back to the master |

Before `x` throws away uncommitted changes or commits that aren't on any other branch, it asks.
`y` goes ahead; any other key keeps the flow.

If the master's session ends, `C-space r` resumes it and `C-space Q` stops flow.

## History

`C-space [` lets you scroll back through the session's output.

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
running, so leaving flow ui and stopping flow are two different things.

To leave, detach with `C-space d` or just close the terminal. Every session keeps running, and the
next `flow ui` attaches again.

To stop, press `C-space Q`. If there are unfinished flows, it lists them and asks what to do:

- `k` keeps them. The server stops, and their sessions resume the next time you run `flow ui`.
- `e` ends them. The server stops and the flows are marked ended, but their branches and worktrees
  stay until `flow clear`.
- `esc` keeps flow running.

If every flow is finished, it asks once: `y` stops, any other key keeps it running.

`flow ui --stop` does the same from the command line. It stops the server and every session, and
keeps unfinished flows like `k` does.

Only one flow ui can be attached at a time. Starting a second one takes over from the first.

## After a crash or a reboot

The next `flow ui` starts the server again and resumes the master and every unfinished worker in
its conversation (`claude --resume`, `codex resume`). After you stop flow on purpose, the master
starts a fresh conversation, but the workers still resume.
