# Flows, workers and the master

- [What a flow is](#what-a-flow-is)
- [States](#states)
- [Phases and steps](#phases-and-steps)
- [Starting a flow](#starting-a-flow)
- [Guided and auto](#guided-and-auto)
- [The master](#the-master)
- [Plain sessions](#plain-sessions)
- [How a worker reports](#how-a-worker-reports)
- [Ending a flow](#ending-a-flow)

## What a flow is

A flow is one feature on its way to a merge request: the request, a git worktree and branch of its
own, and a worker, an agent session flow ui runs in that worktree. flow writes down what happens in
each flow as it happens, and shows where each one stands: its state, its phase and how long each
phase took, task counts, decisions, what it asks, and a timeline.

## States

| State | Means | In the sidebar |
| --- | --- | --- |
| `working` | the worker is at work | RUNNING |
| `delegated` | a subagent works on the flow, such as a review | RUNNING |
| `waiting` | the worker asks you something | NEEDS YOU |
| `idle` | the worker's turn ended without a question; something may be ready to look at | READY |
| `blocked` | the flow cannot go on | NEEDS YOU |
| `not-responding` | a worker that sent heartbeats stopped for 90 seconds | NEEDS YOU |
| `done` | the merge request is open | DONE |
| `ended` | the worker's session ended for good | DONE |

A worker that quits with `/exit` ends its flow. A session that stops for any other reason, such as
a crash or a closed connection, does not: flow ui resumes it.

## Phases and steps

A flow walks through up to four phases. flow ui shows them as steps, `●───●───○───○`: finished,
current and still open. The built-in phases are `plan`, `implement`, `review` and `merge-request`;
a phase starts when the worker runs its skill (`design-code`, `implement-plan`, `review-changes`,
`write-merge-request`). Each step keeps the time spent in it.

You can set your own phases and the skills that start them; see
[configuration.md](configuration.md#phases-the-workflow). A flow keeps the phases it started with.

## Starting a flow

Three ways, all doing the same:

- Ask the master: "start a flow for SHOP-217 add CSV export".
- Press `C-space n` in flow ui (see [ui.md](ui.md#starting-a-flow)).
- Run `flow start "SHOP-217 add CSV export"` in the repository (see [cli.md](cli.md#flow-start)).

flow names the flow, creates its worktree and branch, gives it a slot, runs the repository's setup
script and opens the worker. The worker starts with its first prompt, by default
`/ship-feature SHOP-217 add CSV export` for Claude Code and `$ship-feature ...` for Codex.

Each worker also gets a short system prompt from flow, which tells it that it runs as a worker of
flow, and that messages from the flow master are answers to act on.

## Guided and auto

A flow is `guided` or `auto`. The mode tells the master who answers when the worker asks
something:

- **guided** (the default): the question goes to you. The master tells you which flow asks what,
  and sends your answer on.
- **auto**: the master answers what the code, the plan or your earlier decisions already settle,
  and tells you what it answered. A product decision, or anything that is still a guess, comes to
  you.

The worker itself does not know its mode. How often it stops to ask is up to its skills: the
`ship-feature` skill, for example, stops at the decisions that are yours.

Without the master, the mode changes nothing: every question waits for you in the sidebar.

## The master

The master is an agent session that works with flow for you. It is the big pane when flow ui
opens, and row `0` in the sidebar. It starts flows, reads where they stand, answers workers and
tells you what needs you. It does not do the workers' work.

It runs in the folder you ran `flow ui` in when it started, with the instructions in `master.md`
beside `config.toml` added to its system prompt. flow writes the default `master.md` on its first
run and refreshes it with new versions of flow, until you edit it; your edits are never
overwritten. The master is Claude Code unless `[master] agent` names another
(see [configuration.md](configuration.md#master)).

**Being woken.** The master is woken when a flow of its project becomes `waiting`, `blocked`,
`idle`, `done` or `not-responding`. Flows that stop within two seconds of each other arrive as one
list. `[master] wake = false` turns this off; the master then learns only when it runs
`flow status`.

**Answering.** The master answers a worker with `flow send <name> "<text>"`. flow adds
`master: <text>` to the flow's timeline and wakes the worker with it, or gives it to the worker
once its session runs again.

How a session is woken depends on its harness:

- Claude Code sessions run `flow wait` in the background after each turn. It sleeps until there is
  news and then wakes the session with it.
- Codex cannot wake a session that sits at its prompt, so a busy Codex session gets its news as its
  turn ends, and an idle one has it typed in by flow's server, as one paste, once nobody has typed into
  that pane for ten seconds. flow never types into a Codex session before its first turn.

`flow status <name> --full` shows a worker's whole last answer, for you or the master.

## Plain sessions

A plain session is an agent in a folder of your choice, for tinkering: no flow, worktree, branch,
slot or first prompt. Start one with `flow start --plain <folder>`, or in the new-flow dialog with
the mode set to `plain`. The folder does not have to be a repository.

It shows in the sidebar beside the workers and resumes after a restart like them. Its hooks still
report, so its state shows. It is yours, not the master's: it gets no worker prompt, never wakes
the master, and `flow send` refuses it. `flow remove` forgets it and never touches the folder.

## How a worker reports

A worker reports through its harness's own hooks; the agent and its skills take no part. When flow
ui starts a worker whose agent has an `adapter`, it adds the hooks to the agent's command line.
They run beside your own hooks, and flow installs nothing. Each hook runs `flow hook`, which turns
the event into flow's events and adds only what changes the flow.

**Claude Code** (`adapter = "claude-code"`): flow passes `--settings` with its hooks.

| Claude Code event | Reported |
| --- | --- |
| a prompt | `working`, and the phase of a typed `/skill` |
| the Skill tool | `phase`: the workflow's phase whose skills name the skill |
| the Agent tool | `delegated <description>` |
| AskUserQuestion | `waiting <the question>` |
| a permission prompt | `waiting permission: Bash(rm -rf build)` |
| a tool finishing | `working`; task counts and blocks from a plan-progress tool (`mcp__plan-progress__report`), when the worker has one; a merge request that `gh pr create` or `glab mr create` printed, when it belongs to a remote of the worktree (origin, or a fork's upstream) |
| the end of a turn | `waiting <the question>` when the last line asks one, else `idle <the last sentence>`; the whole closing message becomes the flow's `answer` |
| `/exit` | `ended` |

**Codex** (`adapter = "codex"`): flow passes `-c hooks.<event>=[...]` and the worker prompt as
`-c developer_instructions=...`; nothing is written to `~/.codex`. The first time, Codex asks you
to review the hooks: pick "Trust all and continue". It asks again only when flow's hooks change. A
repository you trust in Codex covers its flow worktrees.

| Codex event | Reported |
| --- | --- |
| a prompt | `working`, and the phase of a `$skill` in it |
| a shell command reading a skill's `SKILL.md` | `phase`: the workflow's phase whose skills name the skill |
| a subagent (`spawn_agent`) | `delegated <its task>`, until the main agent goes on |
| a permission prompt | `waiting permission: Bash(touch notes.txt)` |
| a tool finishing | `working`; a merge request, as for Claude Code |
| the end of a turn | as for Claude Code |
| Esc | `idle interrupted` |

Codex has no plan-progress tool, so a Codex flow shows no task counts, and no `/exit` event, so you
end its flow from flow ui or with `flow remove`.

A subagent's own events are ignored, except that a phase skill it runs counts, as when a review
runs in a fresh agent.

An agent without an adapter reports nothing; flow ui shows only whether it runs. Scripts and other
tools can report by hand with `flow report` (see [cli.md](cli.md#flow-report)).

## Ending a flow

- A flow is `done` once its merge request is open. It stays in the sidebar until you clear it.
- `flow clear` forgets every finished flow and removes its worktree, keeping one with uncommitted
  work. In flow ui, `x` on the board cleans up one finished flow.
- `flow remove <name>` ends a flow in any state: it closes its session, runs the teardown script,
  removes the worktree and branch, and forgets the flow. It refuses uncommitted work and commits
  that are on no other branch, unless you pass `--force` or `--delete-unmerged`.
- In a worker's ended pane, `x` does the same as `flow remove`, and asks before discarding
  anything.

See [cli.md](cli.md#flow-remove) for the details.
