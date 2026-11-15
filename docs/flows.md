# Flows, workers and the master

- [Introduction](#introduction)
- [Starting a flow](#starting-a-flow)
- [Kinds](#kinds)
    - [Research flows](#research-flows)
- [Phases](#phases)
- [States](#states)
- [Guided and auto](#guided-and-auto)
- [The master](#the-master)
    - [Waking](#waking)
    - [Answering](#answering)
    - [Typing into a worker](#typing-into-a-worker)
- [Ending a flow](#ending-a-flow)
- [Stacked flows](#stacked-flows)
- [Plain sessions](#plain-sessions)
- [Merge requests opened or merged elsewhere](#merge-requests-opened-or-merged-elsewhere)
- [Paused services](#paused-services)
- [How a worker reports](#how-a-worker-reports)
    - [Claude Code](#claude-code)
    - [Codex](#codex)
    - [Other agents and subagents](#other-agents-and-subagents)

## Introduction

A flow is one unit of work: one feature or ticket, from the request to the merge request. Each
flow gets its own git worktree and branch, a worker (the agent session doing the work) and the
phases that worker walks through. Above all the flows sits the master, which starts them and
answers their workers, so you only step in where a decision is yours.

You can start a flow from your shell and ask how it stands at any time:

```sh
saaio start "SHOP-217 add CSV export"
saaio status shop-217
```

saaio records each flow's state, phase, time per phase, task counts, decisions, questions and
timeline. A few words come up on every page of these docs:

| Term | Definition |
| --- | --- |
| flow | one feature, fix or question, from request to open merge request or answer, with its own git worktree and branch |
| kind | what a flow is: `feature`, `fix` or `research`, each with its own phases, first prompt and branch |
| worker | the agent session the UI runs in a flow's worktree |
| master | the agent session that starts flows, answers workers and reports to you |
| plain session | an agent in any folder, without a flow |

## Starting a flow

Most of the time you simply ask the master: "start a flow for SHOP-217 add CSV export". You can
also press `C-space n` in the UI ([ui.md](ui.md#starting-a-flow)), or run `saaio start` in the
repository ([cli.md](cli.md#saaio-start)):

```sh
saaio start "SHOP-217 add CSV export"
```

All three run the same steps: pick the kind, name the flow, create its worktree and branch, claim
a slot, run the repository's setup script, and open the worker.

The worker begins with its kind's first prompt: `/ship-feature SHOP-217 add CSV export` for a
feature in Claude Code, `$ship-feature ...` in Codex, `/ship-fix ...` for a fix. saaio also adds
to its system prompt that it runs as a worker of saaio, and that messages from the saaio master
are answers to act on. A stacked flow is also told that its merge request targets its base.

## Kinds

Not every piece of work is a feature. A bug wants to be reproduced before it is fixed, and a
question wants an answer, not a branch. A flow's kind decides its first prompt, its phases and
its branch:

| Kind | For | First prompt | Phases | Branch |
| --- | --- | --- | --- | --- |
| `feature` (default) | anything to build | `/ship-feature <request>` | plan, implement, review, merge-request | `feature` type |
| `fix` | a bug report, a ticket of type bug | `/ship-fix <request>` | diagnose (`diagnose-bug`), implement, review, merge-request | `bugfix` type, or `fix`, `hotfix` |
| `research` | a question to answer | `Research this and answer here. Change no code and commit nothing: <request>` | research | none |

You pick the kind with `saaio start --kind fix` or the new-flow dialog's kind row. The master
picks it from the request. Codex gets `$ship-feature` and `$ship-fix`:

```sh
saaio start --kind fix "SHOP-230 fix totals on refunds"
```

A fix runs [`ship-fix`](https://github.com/JanSuthacheeva/agent-skills), which starts with
`diagnose-bug`: reproduce the bug end to end, find the cause, then plan the fix. A flow's branch
type is the first of its kind's types that the repository's convention lists
([repository.md](repository.md#branch-names)).

`saaio status`, `--json` (`kind`), the board's header and, for a kind other than feature, the
sidebar (`fix claude`) show a flow's kind.

> [!TIP]
> You can define kinds of your own, or give the built-in ones other phases and prompts, in
> [configuration.md](configuration.md#kinds-of-flows).

### Research flows

A research flow answers a question and changes nothing. Its worktree is detached at the base, with
no branch and nothing to merge. It still gets a slot and the repository's setup script, so the
worker can run the project:

```sh
saaio start --kind research "why do refunds round differently on the PDF?"
```

Its one phase, `research`, starts with its first turn. Once a turn ends without a question, the
flow is `done`: it moves to DONE, its services pause like any resting flow's, and the master tells
you its findings are ready. `saaio status <name> --full` shows its answer, up to 4000 characters;
its pane holds all of it. A follow-up you type or send makes it work again, and it is `done` again
when it answers.

Started with `--base`, a research flow stands on that branch's commits. When the base merges, its
base moves like any stacked flow's, but its worker is not told to rebase: it has nothing to rebase.

`saaio clear` forgets a research flow once it is done, like an ended flow: it closes its pane, runs
the teardown script, removes its worktree (kept with uncommitted work) and frees its slot. `x` on
the board clears one.

## Phases

A flow walks through the phases of its [kind](#kinds), and saaio shows them as up to 4 steps:
`●───●───○───○` (finished, current, open). Each step records the time spent in it. A phase starts
when the worker runs the skill that begins it. For the feature kind:

| Phase | Started by the skill |
| --- | --- |
| `plan` | `design-code` |
| `implement` | `implement-plan` |
| `review` | `review-changes` |
| `merge-request` | `write-merge-request` |

A flow keeps the phases it started with, even when you change them in your config later. Custom
phases and skills are set in [configuration.md](configuration.md#kinds-of-flows), and if your
agent has no matching skills, see [configuration.md](configuration.md#without-matching-skills).

## States

Where a flow stands is its state, and the sidebar groups flows by it, so the ones that need you
are always on top:

| State | Means | Sidebar group |
| --- | --- | --- |
| `working` | the worker is at work | RUNNING |
| `delegated` | a subagent works on the flow, such as a review | RUNNING |
| `waiting` | the worker asks you something | NEEDS YOU |
| `idle` | the worker's turn ended without a question | READY |
| `blocked` | the flow cannot go on | NEEDS YOU |
| `not-responding` | a worker that sent heartbeats has sent none for 90 seconds | NEEDS YOU |
| `done` | the merge request opened or merged, or a research flow answered; the worker can still report activity | DONE |
| `ended` | the worker's session ended for good | DONE |

A worker's session that ends with `/exit` ends the flow. When it stops any other way, a crash or a
closed connection, the flow stays and its row shows `exited`. `C-space r` resumes it, and the next
server start resumes it automatically. If it exits while the flow is `working`, the flow is
`blocked` with `agent exited (code N)` and the last lines of its pane.

## Guided and auto

When a worker asks a question, someone has to answer it. The flow's mode decides who:

- **`guided`** (the default): you. The master names the flow and the question, and sends your
  answer on.
- **`auto`**: the master, for what the code, the plan or your earlier decisions settle. It reports
  what it answered. Product decisions and guesses still go to you.

The worker does not know its mode. How often it asks depends on its skills: `ship-feature`, for
example, stops at the decisions that are yours.

> [!NOTE]
> Without the master, the mode has no effect. Every question waits in the sidebar for you.

## The master

The master is the big pane when the UI opens, row `0` in the sidebar. It starts flows, reads their
state, answers workers and reports what needs you, but it does not do the workers' work. It runs in
the folder `saaio` was started in, and is Claude Code unless `[master] agent` names another agent
([configuration.md](configuration.md#master)).

Its system prompt includes `master.md`, next to `config.toml`, so you can tell it how you like to
work. saaio writes the default on the first run and updates it with new versions until you edit
it; your edits are never overwritten.

### Waking

The master does not have to keep asking how the flows stand. When a flow of its project becomes
`waiting`, `blocked`, `idle`, `done` or `not-responding`, saaio wakes it. Flows that stop within two
seconds of each other arrive as one list.

If you would rather it learned only by running `saaio status` itself, turn the wake-ups off:

```toml
[master]
wake = false
```

### Answering

The master answers a worker with `saaio send`:

```sh
saaio send shop-221 "Count them against the refund date."
```

saaio adds `master: <text>` to the flow's timeline and wakes the worker with it, or delivers it when
the worker's session runs again. How a session is woken depends on its harness:

| Harness | How a session is woken |
| --- | --- |
| Claude Code | `saaio wait` runs in the background after each turn, sleeps until there is news, then wakes the session |
| Codex, busy | news arrives when the turn ends |
| Codex, idle | saaio's server types the news in as one paste, after ten seconds without typing in that pane. Never before the session's first turn. |

`saaio status <name> --full` shows a worker's last answer, up to 4000 characters.

A Claude Code worker can also ask with a question dialog: one to four questions, each with options
to pick. `saaio status <name>` lists them, and the master answers every question in one `saaio
send`. saaio's server then closes the dialog in the worker's pane, and the worker reads the message
as its answer ([cli.md](cli.md#saaio-send)). A dialog you answer in the pane needs nothing from the
master.

> [!NOTE]
> A worker on a permission prompt reads a message only once the prompt is answered in its pane.
> `saaio send` says so and names the prompt, so the master can ask you to answer it.

### Typing into a worker

A message from `saaio send` reaches the model as words, so `/compact` sent that way does not
compact. When the line should run as if you typed it in the pane, a slash command or a skill, the
master sends it with `--type`:

```sh
saaio send --type shop-221 /compact
saaio send --type shop-221 "continue shipping"
```

saaio's server types each line and Enter into the worker's pane, in order, each once the worker is
idle again. The master uses it when you ask it to compact or continue a worker from your phone,
away from the pane. The details are in [cli.md](cli.md#--type).

## Ending a flow

A flow whose merge request opened or merged, by anyone, can show `done`, but its worker still runs,
keeps its slot and can ask questions. Once the worker's session ends, the flow is `ended`: its slot
is released, and you can clear it. A research flow that answered is `done` and can be cleared
right away.

`saaio clear` forgets every ended flow, and every research flow that answered, and removes its
worktree. It keeps a worktree with uncommitted work. `x` on the board cleans up one such flow:

```sh
saaio clear
```

`saaio remove` ends a flow in any state. It closes the session, runs the teardown script, removes
the worktree and branch, and forgets the flow. It refuses uncommitted work and commits on no other
branch, unless you pass `--force` or `--delete-unmerged`:

```sh
saaio remove shop-217
```

`x` in an ended worker's pane does the same as `saaio remove`, and asks before it discards
anything. The details are in [cli.md](cli.md#saaio-remove).

## Stacked flows

Some work is too big for one merge request. A stack runs as one flow per merge request: start the
first as usual, then each next one on the branch before it, with `saaio start --base <branch>`
([cli.md](cli.md#stacked-flows)) or the dialog's base row ([ui.md](ui.md#starting-a-flow)):

```
main <- flow/shop-217 (shop-217) <- flow/shop-218 (shop-218, --base flow/shop-217)
```

The stacked flow's worktree starts from the base's commits at that moment. Its worker opens its
merge request against the base and rebases on it, and pushes the base first if the remote lacks
it. `saaio status <flow>` shows `base`, the board's header `on <base>`, and `--json`
`workspace.base`.

When the base's merge request merges ([how saaio learns](#merge-requests-opened-or-merged-elsewhere)),
every unfinished flow of the repository standing on it moves onto the branch it merged into. Its
base changes, its timeline says `base flow/shop-217 merged, now on main`, and its worker hears it
like a message from the master, with what to do: rebase its own commits onto the new base, point
its merge request there, and push with `--force-with-lease`:

```sh
git rebase --onto origin/main flow/shop-217
```

saaio runs no git in the worktree itself. A move that could not be written is made on the next
check, 2 minutes later. `saaio remove` and `saaio clear` of a base whose merge saaio saw make the
moves themselves, since the server no longer sees a removed flow.

> [!NOTE]
> Only a base that is a flow of the same store and repository is followed. A stack on a branch no
> flow runs keeps its base when that merges.

`saaio remove` of a base names the flows that stand on it and removes it all the same
([cli.md](cli.md#saaio-remove)). A base removed before saaio saw its merge leaves them where they
are.

## Plain sessions

Sometimes you just want an agent in a folder, without a worktree or a workflow, but still in the
same UI as your flows. That is a plain session:

```sh
saaio start --plain ~/projects/scratch
```

Or pick mode `plain` in the new-flow dialog. The folder does not have to be a repository.

A plain session gets a row in the sidebar, resumes after a restart, and shows the state its hooks
report. It has no flow, worktree, branch or slot, no first prompt or worker prompt, and it wakes
no master; `saaio send` to it is refused. `saaio remove` forgets the session and leaves the folder
untouched.

## Merge requests opened or merged elsewhere

A merge request counts whoever opened or merged it: the worker, the master or you. The UI asks
GitHub or GitLab about each flow's branch and acts on what it finds:

- **An open merge request the flow does not show:** a live flow is `done`. The sidebar shows
  `PR #42`, the board `PR open`. The worker keeps running and may later become `waiting`.
- **A merged one:** the flow records the merge. A live flow shows `done` if it had no newer worker
  state; an ended flow stays `ended`.
- **The flow's merge request, closed without a merge, while the worker is unfinished:** the flow is
  `idle` in its last phase, its timeline says `PR #42 closed`, and the master is woken.
- **The same, but the worker already ended:** the flow stays `ended`, its timeline says
  `PR #42 closed`. Its worker does not restart or reclaim a slot.
- **Nothing, or a closed one the flow does not show:** no change.

An open or merged request keeps the workflow's last phase, even if later worker hooks mention an
earlier skill. Later questions and work can still update the state while the worker remains live.

Only merge requests opened after the flow started count. One from an earlier branch of the same
name, such as a cleared flow's, is ignored.

saaio asks about every flow with a branch and a worktree that is not merged, ended ones included,
but never a plain session. It asks as the server starts, then every 2 minutes, one flow after
another: about 30 requests per flow an hour. It uses `gh` when a remote of the repository is on
github.com, and `glab` when one has `gitlab` in its host; an SSH host alias counts, and other hosts
are not asked. The tools run in the repository's main checkout, never the worktree, logged in as
you are:

```sh
gh auth login
glab auth login
```

Without `gh` or `glab`, when you are not logged in or offline, or when no answer comes within a
minute, nothing changes, and the error goes to `server.log` once.

> [!NOTE]
> saaio only asks while the UI (its server) runs. `saaio status` asks no one.

## Paused services

A flow that rests, `idle`, `waiting`, `blocked`, `done` or `ended`, for 5 minutes
(`[services] pause_after_minutes`) gets its services paused: `saaio server` runs the repo's
`.saaio/pause`. When its worker works again (a new prompt, or a tool call once the master woke
it), `.saaio/resume` runs, and the worker's tool calls wait until the services are back, up to 2
minutes.

This only happens in repos with both scripts; others are never paused. Pausing changes neither the
flow's state nor its age, and wakes no one. Plain sessions and flows that are `not-responding` are
never paused or resumed.

A failed pause or resume shows in the timeline (`resume failed: port 8003 is taken`). The worker is
not told, so its commands meet stopped services.

How to write the scripts is explained in [repository.md](repository.md#pause-and-resume).

## How a worker reports

saaio learns what a worker does through the harness's own hooks, so neither the agent nor its
skills take any part. The UI adds the hooks to the command line of an agent with an `adapter`.
They run next to your own hooks, and nothing is installed. Each hook runs `saaio hook`, which
records only the events that change the flow.

### Claude Code

With `adapter = "claude-code"`, saaio passes its hooks with `--settings`:

| Claude Code event | Reported |
| --- | --- |
| a prompt | `working`, and the phase of a typed `/skill` |
| the Skill tool | `phase`: the workflow's phase whose skills name the skill |
| the Agent tool, not in the background | `delegated <description>` |
| AskUserQuestion | `waiting <the question>` |
| a permission prompt | `waiting permission: Bash(rm -rf build)` |
| a tool finishing | `working`. Task counts and blocks from a plan-progress tool (`mcp__plan-progress__report`), when the worker has one. A merge request printed by `gh pr create` or `glab mr create`, when it belongs to a remote of the worktree (origin, or a fork's upstream). |
| the end of a turn | `waiting <the question>` when the last line asks one, else `idle <the last sentence>`, even while a background task runs. The closing message, up to 4000 characters, becomes the flow's `answer`. |
| `/exit` | `ended` |

### Codex

> [!WARNING]
> Codex support is experimental: it works, but is less tested than Claude Code and has the gaps
> listed under Codex in [ROADMAP.md](../ROADMAP.md).

With `adapter = "codex"`, the hooks, their per-launch trust state and the worker prompt pass as
`-c` arguments. A worker also trusts its own worktree for that launch once you
[trusted](repository.md#trust) the repository. Nothing is written to `~/.codex`.

| Codex event | Reported |
| --- | --- |
| a prompt | `working`, and the phase of a `$skill` in it |
| a shell command reading one configured phase skill's `SKILL.md` | that skill's phase |
| a shell command reading skills from several configured phases | no phase; it is treated as read-ahead |
| a subagent (`spawn_agent`) | `delegated <its task>`, until the main agent goes on |
| a permission request you answer | after three seconds without a tool completion, `waiting permission: Bash(touch notes.txt)` |
| a permission request automatic review answers | nothing; the flow stays `working` |
| a tool finishing | `working`. A merge request, as for Claude Code. |
| the end of a turn | as for Claude Code |
| Esc | `idle interrupted` |

Codex has no Skill tool hook. A `$skill` named in a prompt starts its configured phase, and for
later phases saaio watches for a single `SKILL.md` read. The worker instruction makes that read a
phase start by prohibiting skill reads ahead, including reads in separate commands. If a worker
does read ahead despite the instruction, a separate read can still advance the displayed phase.

The review phase may not show: Codex hands the review to a subagent without reading the review
skill itself, so no hook names it. A merge request still moves the flow to its last phase.

saaio trusts only its own hooks and, in a trusted repository, the worker's own worktree. Your
configured hooks and a repository's hooks still receive Codex's review.

Codex runs the same permission hook whether you or its automatic review (`--approve-for-me`)
answers, so saaio reads who answers from the turn in the session's rollout file. Automatic review
never waits for you, so its requests do not show `waiting`, however long they take. A rejected
request goes back to the worker and is not reported. If the rollout cannot be read, the request
counts as yours, and server.log says why.

A few more differences from Claude Code:

- A running Codex worker with no first turn after 30 seconds becomes `waiting`. Answer the dialog
  in its pane, and its first prompt returns it to `working`.
- There is no plan-progress tool, so no task counts.
- There is no `/exit` event: end the flow from the UI or with `saaio remove`.

### Other agents and subagents

A subagent's events are ignored, except a phase skill a Claude Code subagent runs, such as a
review in a fresh agent. An agent without an adapter reports nothing, and the UI shows only
whether it runs. Scripts and other tools can report with `saaio report`
([cli.md](cli.md#saaio-report)).
