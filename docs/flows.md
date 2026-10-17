# Flows, workers and the master

```sh
flow start "SHOP-217 add CSV export"
```

That's a flow: one feature, from your request to an open merge request. flow makes a branch and a
worktree for it and opens an agent there, the flow's worker.

From then on flow keeps a log of what the worker does. `flow status shop-217`, or the board in
flow ui, shows what came of it so far. You'll see the phase it's in and how long the earlier ones
took, any decisions it made for you, what it's asking, and a timeline of the whole thing.

- [States](#states)
- [Phases](#phases)
- [Start a flow](#start-a-flow)
- [Guided and auto](#guided-and-auto)
- [The master](#the-master)
- [Plain sessions](#plain-sessions)
- [How a worker reports](#how-a-worker-reports)
- [End a flow](#end-a-flow)

## States

A flow is always in one of these states, and the sidebar groups it by what that means for you:

| State | Means | In the sidebar |
| --- | --- | --- |
| `working` | the worker is at work | RUNNING |
| `delegated` | a subagent is working on the flow, such as a review | RUNNING |
| `waiting` | the worker is asking you something | NEEDS YOU |
| `idle` | the worker's turn ended without a question; something may be ready to look at | READY |
| `blocked` | the flow can't go on | NEEDS YOU |
| `not-responding` | a worker that sent heartbeats has been silent for 90 seconds | NEEDS YOU |
| `done` | the merge request is open | DONE |
| `ended` | the worker's session ended for good | DONE |

Quitting a worker with `/exit` ends its flow. If the session stops any other way, say a crash or a
dropped connection, the flow carries on. Its row says `exited`, `C-space r` resumes it, and the
next time the server starts it resumes on its own.

## Phases

A flow walks through up to four phases, which flow ui draws as steps: `●───●───○───○`, finished,
current, still to come. Each step keeps the time spent in it.

The built-in phases are `plan`, `implement`, `review` and `merge-request`. Each starts when the
worker runs its skill: `design-code`, `implement-plan`, `review-changes` and `write-merge-request`.

You can name your own phases and the skills that start them, and a flow keeps working without any
matching skills at all. [configuration.md](configuration.md#phases-the-workflow) covers both. A
flow keeps the phases it started with.

## Start a flow

There are three ways, and they all do the same thing:

- ask the master, "start a flow for SHOP-217 add CSV export"
- press `C-space n` in flow ui (see [ui.md](ui.md#starting-a-flow))
- run `flow start "SHOP-217 add CSV export"` in the repository (see [cli.md](cli.md#flow-start))

flow names the flow, creates its worktree and branch, gives it a slot, runs the repository's setup
script and opens the worker. The worker starts with its first prompt. By default that's
`/ship-feature SHOP-217 add CSV export` in Claude Code and `$ship-feature ...` in Codex.

flow also gives each worker a short system prompt. It tells the worker that it runs as part of
flow, and that messages from the flow master are answers to act on.

## Guided and auto

Every flow is either `guided` or `auto`. The mode tells the master who answers when the worker asks
something.

In a **guided** flow, the default, every question comes to you. The master tells you which flow is
asking what, and passes your answer on.

In an **auto** flow, the master answers whatever the code, the plan or your earlier decisions
already settle, and tells you what it said. Product decisions, and anything that would still be a
guess, still come to you.

The worker doesn't know its own mode. How often it stops to ask depends on its skills; the
`ship-feature` skill, for example, stops at the decisions that are yours to make.

If you're not using the master, the mode makes no difference. Every question waits for you in the
sidebar.

## The master

The master is the agent you talk to. It's the big pane when flow ui opens, and row `0` in the
sidebar. You tell it what you want built; it starts the flows, keeps an eye on them, answers the
workers and tells you when something needs you. It doesn't do the workers' work itself. If you
think of yourself as the CTO, the master is your team lead and the workers are its developers.

It runs in the folder where you started `flow ui`, with the instructions from `master.md` (next to
`config.toml`) added to its system prompt. flow writes the default `master.md` on the first run and
keeps it up to date with new versions of flow until you edit it. Your edits are never overwritten.
The master is Claude Code unless `[master] agent` names another agent
(see [configuration.md](configuration.md#master)).

### When the master wakes up

The master wakes up when a flow in its project becomes `waiting`, `blocked`, `idle`, `done` or
`not-responding`. Flows that stop within two seconds of each other arrive together, as one list.
Set `[master] wake = false` to turn this off; the master then only finds out when it runs
`flow status`.

### How it answers

The master answers a worker with `flow send <name> "<text>"`. flow adds `master: <text>` to the
flow's timeline and wakes the worker with it, or hands it over once the worker's session is running
again.

Waking works differently per harness. A Claude Code session runs `flow wait` in the background
after each turn, which sleeps until there's news and then wakes the session with it.

Codex can't wake a session that's sitting at its prompt. A busy Codex session gets its news when
its turn ends. For an idle one, flow's server types the news in, as one paste, once nobody has
typed in that pane for ten seconds. flow never types into a Codex session before its first turn.

`flow status <name> --full` shows a worker's whole last answer, for you or the master.

## Plain sessions

Sometimes you just want an agent in some folder to tinker with. That's a plain session. It has no
flow, worktree, branch, slot or first prompt. Start one with `flow start --plain <folder>`, or pick
`plain` as the mode in the new-flow dialog. The folder doesn't have to be a repository.

It sits in the sidebar next to the workers and resumes after a restart like they do. Its hooks
still report, so you can see its state. But it's yours, not the master's. It gets no worker prompt
and never wakes the master, and `flow send` refuses it. `flow remove` forgets it without touching
the folder.

## How a worker reports

A worker reports through its harness's own hooks. Neither the agent nor its skills have to do
anything. When flow ui starts a worker whose agent has an `adapter`, it adds the hooks to the
agent's command line. They run alongside your own hooks, and flow installs nothing. Each hook runs
`flow hook`, which turns the event into flow's events and records only what changes the flow.

### Claude Code

With `adapter = "claude-code"`, flow passes its hooks with `--settings`.

| Claude Code event | Reported |
| --- | --- |
| a prompt | `working`, and the phase of a typed `/skill` |
| the Skill tool | `phase`: the workflow's phase whose skills name the skill |
| the Agent tool | `delegated <description>` |
| AskUserQuestion | `waiting <the question>` |
| a permission prompt | `waiting permission: Bash(rm -rf build)` |
| a tool finishing | `working`; task counts and blocks from a plan-progress tool (`mcp__plan-progress__report`), when the worker has one; a merge request that `gh pr create` or `glab mr create` printed, when it belongs to a remote of the worktree (origin, or a fork's upstream) |
| the end of a turn | `delegated <the task>` while a background task still runs, else `waiting <the question>` when the last line asks one, else `idle <the last sentence>`; the whole closing message becomes the flow's `answer` |
| `/exit` | `ended` |

### Codex

With `adapter = "codex"`, flow passes `-c hooks.<event>=[...]`, and the worker prompt as
`-c developer_instructions=...`. Nothing is written to `~/.codex`. The first time, Codex asks you
to review the hooks; pick "Trust all and continue". It only asks again when flow's hooks change. A
repository you trust in Codex covers its flow worktrees too.

| Codex event | Reported |
| --- | --- |
| a prompt | `working`, and the phase of a `$skill` in it |
| a shell command reading a skill's `SKILL.md` | `phase`: the workflow's phase whose skills name the skill |
| a subagent (`spawn_agent`) | `delegated <its task>`, until the main agent goes on |
| a permission prompt | `waiting permission: Bash(touch notes.txt)` |
| a tool finishing | `working`; a merge request, as for Claude Code |
| the end of a turn | as for Claude Code |
| Esc | `idle interrupted` |

Codex has no plan-progress tool, so a Codex flow shows no task counts. It also has no `/exit`
event, so you end a Codex flow from flow ui or with `flow remove`.

### Everything else

A subagent's own events are ignored, with one exception: a phase skill it runs still counts, as
when a review runs in a fresh agent.

An agent without an adapter reports nothing, and flow ui only shows whether it's running. Scripts
and other tools can report by hand with `flow report` (see [cli.md](cli.md#flow-report)).

## End a flow

A flow is `done` once its merge request is open. It stays in the sidebar until you clear it.

`flow clear` forgets every finished flow and removes its worktree, but keeps a worktree with
uncommitted work. In flow ui, `x` on the board cleans up one finished flow.

`flow remove <name>` ends a flow in any state. It closes the session, runs the teardown script,
removes the worktree and branch, and forgets the flow. It refuses if there's uncommitted work or
commits that aren't on any other branch, unless you pass `--force` or `--delete-unmerged`. In a
worker's ended pane, `x` does the same as `flow remove` and asks before throwing anything away.

[cli.md](cli.md#flow-remove) has the details.
