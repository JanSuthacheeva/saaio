# Flows, workers and the master

A flow is one unit of work: one feature or ticket, from request to merge request. It has its own
git worktree and branch, a worker (the agent session doing the work) and the phases the worker
walks through. The master starts flows and answers their workers.

```sh
saaio start "SHOP-217 add CSV export"
saaio status shop-217
```

| Term | Definition |
| --- | --- |
| flow | one feature, from request to open merge request, with its own git worktree and branch |
| worker | the agent session the UI runs in a flow's worktree |
| master | the agent session that starts flows, answers workers and reports to you |
| plain session | an agent in any folder, without a flow |

saaio records each flow's state, phase, time per phase, task counts, decisions, questions and
timeline.

- [States](#states)
- [Phases](#phases)
- [Starting a flow](#starting-a-flow)
- [Guided and auto](#guided-and-auto)
- [The master](#the-master)
- [Plain sessions](#plain-sessions)
- [How a worker reports](#how-a-worker-reports)
- [Ending a flow](#ending-a-flow)

## States

| State | Means | Sidebar group |
| --- | --- | --- |
| `working` | the worker is at work | RUNNING |
| `delegated` | a subagent works on the flow, such as a review | RUNNING |
| `waiting` | the worker asks you something | NEEDS YOU |
| `idle` | the worker's turn ended without a question | READY |
| `blocked` | the flow cannot go on | NEEDS YOU |
| `not-responding` | a worker that sent heartbeats has sent none for 90 seconds | NEEDS YOU |
| `done` | the merge request is open or merged | DONE |
| `ended` | the worker's session ended for good | DONE |

| Session stops by | Result |
| --- | --- |
| `/exit` | the flow ends |
| anything else (crash, closed connection) | the flow stays, its row shows `exited`. `C-space r` resumes it. The next server start resumes it automatically. |

## Phases

| Built-in phase | Started by the skill |
| --- | --- |
| `plan` | `design-code` |
| `implement` | `implement-plan` |
| `review` | `review-changes` |
| `merge-request` | `write-merge-request` |

- Up to 4 phases, shown as steps: `●───●───○───○` (finished, current, open).
- Each step records the time spent in it.
- A flow keeps the phases it started with.
- Custom phases and skills: [configuration.md](configuration.md#phases-the-workflow).
- No matching skills: [configuration.md](configuration.md#without-matching-skills).

## Starting a flow

| From | How |
| --- | --- |
| the master | "start a flow for SHOP-217 add CSV export" |
| the UI | `C-space n` ([ui.md](ui.md#starting-a-flow)) |
| the shell, in the repository | `saaio start "SHOP-217 add CSV export"` ([cli.md](cli.md#saaio-start)) |

All three run the same steps: name the flow, create its worktree and branch, claim a slot, run the
repository's setup script, open the worker.

| Worker gets | Content |
| --- | --- |
| first prompt | default `/ship-feature SHOP-217 add CSV export` (Claude Code), `$ship-feature ...` (Codex) |
| system prompt from saaio | it runs as a worker of saaio. Messages from the saaio master are answers to act on. |

## Guided and auto

The mode decides who answers a worker's question.

| Mode | Questions go to |
| --- | --- |
| `guided` (default) | you. The master names the flow and the question, and sends your answer on. |
| `auto` | the master, for what the code, the plan or your earlier decisions settle. It reports what it answered. Product decisions and guesses go to you. |

- The worker does not know its mode.
- How often a worker asks depends on its skills. `ship-feature` stops at the decisions that are
  yours.
- Without the master, the mode has no effect. Every question waits in the sidebar.

## The master

- The big pane when the UI opens, row `0` in the sidebar.
- Starts flows, reads their state, answers workers, reports what needs you. Does not do the workers'
  work.
- Runs in the folder `saaio` was started in.
- System prompt includes `master.md` (next to `config.toml`). saaio writes the default on the first
  run and updates it with new versions until it is edited. Edits are never overwritten.
- Claude Code unless `[master] agent` names another agent ([configuration.md](configuration.md#master)).

### Waking

| Trigger | Delivery |
| --- | --- |
| a flow of its project becomes `waiting`, `blocked`, `idle`, `done` or `not-responding` | the master is woken. Flows that stop within two seconds arrive as one list. |
| `[master] wake = false` | no wake-ups. The master learns only by running `saaio status`. |

### Answering

```sh
saaio send shop-221 "Count them against the refund date."
```

saaio adds `master: <text>` to the flow's timeline and wakes the worker with it, or delivers it when
the worker's session runs again.

| Harness | How a session is woken |
| --- | --- |
| Claude Code | `saaio wait` runs in the background after each turn, sleeps until there is news, then wakes the session |
| Codex, busy | news arrives when the turn ends |
| Codex, idle | saaio's server types the news in as one paste, after ten seconds without typing in that pane. Never before the session's first turn. |

`saaio status <name> --full` shows a worker's whole last answer.

### Typing into a worker

```sh
saaio send --type shop-221 /compact
saaio send --type shop-221 "continue shipping"
```

saaio's server types each line and Enter into the worker's pane, as if the user typed it there, so a
slash command or a skill runs. A message from `saaio send` reaches the model as words, so `/compact`
sent that way does not compact. The lines are typed in order, each once the worker is idle again.
The master uses it when the user asks to compact or continue a worker from their phone, away from
the pane. Details in [cli.md](cli.md#--type).

## Plain sessions

```sh
saaio start --plain ~/projects/scratch
```

Or mode `plain` in the new-flow dialog.

| Has | Has not |
| --- | --- |
| a row in the sidebar | a flow, worktree, branch, slot |
| resume after a restart | a first prompt or worker prompt |
| state from its hooks | wake-ups for the master |
| | `saaio send` (refused) |

- The folder does not have to be a repository.
- `saaio remove` forgets the session and leaves the folder untouched.

## How a worker reports

- Through the harness's own hooks. The agent and its skills take no part.
- The UI adds the hooks to the command line of an agent with an `adapter`. They run next to your
  own hooks. Nothing is installed.
- Each hook runs `saaio hook`, which records only events that change the flow.

### Claude Code

`adapter = "claude-code"`. Hooks passed with `--settings`.

| Claude Code event | Reported |
| --- | --- |
| a prompt | `working`, and the phase of a typed `/skill` |
| the Skill tool | `phase`: the workflow's phase whose skills name the skill |
| the Agent tool | `delegated <description>` |
| AskUserQuestion | `waiting <the question>` |
| a permission prompt | `waiting permission: Bash(rm -rf build)` |
| a tool finishing | `working`. Task counts and blocks from a plan-progress tool (`mcp__plan-progress__report`), when the worker has one. A merge request printed by `gh pr create` or `glab mr create`, when it belongs to a remote of the worktree (origin, or a fork's upstream). |
| the end of a turn | `delegated <the task>` while a background task runs, else `waiting <the question>` when the last line asks one, else `idle <the last sentence>`. The whole closing message becomes the flow's `answer`. |
| `/exit` | `ended` |

### Codex

Experimental: Codex support works, but is less tested than Claude Code and has the gaps listed
under Codex in [ROADMAP.md](../ROADMAP.md). Phase tracking depends on the worker following
saaio's instruction to read each workflow skill's `SKILL.md` only when it begins that skill.

`adapter = "codex"`. Hooks, their per-launch trust state and the worker prompt pass as `-c`
arguments. A worker also trusts its own worktree for that launch. Nothing is written to `~/.codex`.

| Codex event | Reported |
| --- | --- |
| a prompt | `working`, and the phase of a `$skill` in it |
| a shell command reading one configured phase skill's `SKILL.md` | that skill's phase |
| a shell command reading skills from several configured phases | no phase; it is treated as read-ahead |
| a subagent (`spawn_agent`) | `delegated <its task>`, until the main agent goes on |
| a permission request | after three seconds without a tool completion, `waiting permission: Bash(touch notes.txt)` |
| a tool finishing | `working`. A merge request, as for Claude Code. |
| the end of a turn | as for Claude Code |
| Esc | `idle interrupted` |

- saaio trusts only its own hooks and the worker's own worktree. Your configured hooks and a
  repository's hooks still receive Codex's review.
- Codex runs the permission hook before automatic review or a user prompt. A quick automatic
  approval finishes the tool and cancels the pending wait. A slow automatic review can still show
  `waiting` after three seconds; the completed tool returns the flow to `working`.
- Codex has no Skill tool hook. A `$skill` named in a prompt starts its configured phase. For later
  phases, saaio observes a single `SKILL.md` read. The worker instruction makes that read a phase
  start by prohibiting skill reads ahead, including reads in separate commands. If a worker does
  read ahead despite the instruction, a separate read can still advance the displayed phase.
- A running Codex worker with no first turn after 30 seconds becomes `waiting`; answer the dialog
  in its pane, then its first prompt returns it to `working`.
- No plan-progress tool: no task counts.
- No `/exit` event: end the flow from the UI or with `saaio remove`.

### Other agents and subagents

- A subagent's events are ignored, except a phase skill it runs (a review in a fresh agent).
- An agent without an adapter reports nothing. The UI shows only whether it runs.
- Scripts and other tools report with `saaio report` ([cli.md](cli.md#saaio-report)).

## Merge requests opened or merged elsewhere

The UI asks GitHub or GitLab about each flow's branch, so a merge request counts whoever opened
or merged it: the worker, the master or you.

| Found on the flow's branch | Result |
| --- | --- |
| an open merge request the flow does not show | the flow is `done`. Sidebar `PR #42`, board `PR open`. |
| a merged one | the flow is `done` and merged. Sidebar `merged`, board `PR merged`, its detail `PR #42 merged`. Not asked again. |
| the flow's merge request, closed without a merge, worker still unfinished | the flow is `idle` in its last phase, timeline `PR #42 closed`. The master is woken. |
| the flow's merge request, closed without a merge, worker already ended | the flow stays `ended`, timeline `PR #42 closed`. Its worker does not restart or reclaim a slot. |
| nothing, or a closed one the flow does not show | no change |

- Asked: every flow with a branch that is not merged, ended ones included. Never a plain session.
- An open or merged request keeps the workflow's last phase even if later worker hooks mention an
  earlier skill. Closing an unmerged request returns an unfinished flow to `idle` in its last phase;
  a flow whose worker ended stays `ended`.
- Only merge requests opened after the flow started count. One from an earlier branch of the same
  name, such as a cleared flow's, is ignored.
- When: as the server starts, then every 2 minutes, one flow after another. About 30 requests per
  flow an hour.
- With `gh` for a remote on github.com, `glab` for a host with `gitlab` in its name. Every remote of
  the worktree counts: origin, a fork's upstream, an SSH host alias. Other hosts are not asked.
- Runs in the worktree, logged in as you are (`gh auth login`, `glab auth login`).
- No `gh` or `glab`, not logged in, offline, or no answer within a minute: no change. The error goes
  to `server.log` once.
- Only while the UI (its server) runs. `saaio status` asks no one.

## Ending a flow

| Action | Effect |
| --- | --- |
| merge request opened or merged, by anyone | the flow is `done`. It stays in the sidebar until cleared. |
| `saaio clear` | forgets every finished flow and removes its worktree. Keeps a worktree with uncommitted work. |
| `x` on the board | cleans up one finished flow |
| `saaio remove <name>` | any state: closes the session, runs the teardown script, removes worktree and branch, forgets the flow. Refuses uncommitted work and commits on no other branch, unless `--force` or `--delete-unmerged`. |
| `x` in an ended worker's pane | same as `saaio remove`, asks before discarding anything |

Details: [cli.md](cli.md#saaio-remove).
