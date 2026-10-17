# Troubleshooting

## Where to look first

`flow status <name> --full` shows everything flow knows about a flow, with nothing cut short.

If that doesn't explain it, read `server.log` in the store (`~/.local/state/flow/server.log`, or
under `$FLOW_HOME`). It records the sessions the server opened, resumed or couldn't start, the files
it couldn't write, and why it stopped. Each line starts with a time and a process id:

```
2026-10-16 13:39:54 +07:00 [2739] started (flow 0.1.0 1de3dc4, protocol 5)
```

## flow ui

**flow ui says the server runs another protocol.** After an update, the server that's still
running is the old version, and you see `flow server runs protocol 4, this flow speaks 5`. Run
`flow ui --stop`, then `flow ui`. Unfinished flows resume.

**The prefix key does nothing.** Some terminals, or a tmux around flow ui, keep `C-space` for
themselves. Pick another prefix in `config.toml`, such as `prefix = "C-b"` under `[ui]`, then run
`flow ui --stop` and `flow ui` (see [configuration.md](configuration.md#ui)).

**flow ui refuses to start inside a worker.** A worker runs inside your flow ui, so a `flow ui` in
there would take over yours. Give it a store of its own with `FLOW_HOME=$(mktemp -d) flow ui`.

**A second flow ui took over the first.** Only one flow ui is attached at a time, and the newest
wins. Run `flow ui` again in the terminal you want.

**The colors are wrong or did not change.** A broken `theme.toml` keeps the last colors that
worked and says in the title row what is wrong and on which line (see
[themes.md](themes.md#errors)).

## Workers

**`pane: when flow ui runs`.** `flow start` recorded the flow, but flow ui isn't running. Run
`flow ui` and it opens the worker within half a second.

**A worker's row shows nothing but whether it runs.** Its agent has no `adapter`, so flow can't
see inside it. Set `adapter = "claude-code"` or `adapter = "codex"` on a custom agent that runs one
of them (see [configuration.md](configuration.md#agents)).

**A Codex worker reports nothing.** Codex asks you to review flow's hooks the first time, and again
when they change. Jump to the worker and pick "Trust all and continue".

**A flow's steps stay empty.** No skill the worker ran matches a phase, so the steps stay
`○───○───○───○` and the sidebar names the state instead of a phase. The flow still works. To see
progress, list your skills under `[[phases]]` (see
[configuration.md](configuration.md#use-your-own-workflow)).

**A worker sits at a question about trusting its folder.** The first time an agent opens in a new
folder, it may ask whether you trust it. Jump to the worker and answer.

**A Claude Code worker still shows a spinner after Esc.** Claude Code runs no hook when you
interrupt it, so flow doesn't see the turn end. It shows the right state again at the worker's
next prompt.

**A Codex flow shows no task counts and never ends by itself.** Codex has no tool for task counts
and no event for `/exit`. End its flow from flow ui (`x` on its pane once it exited, or on the
board when it is done) or with `flow remove`.

**A Codex flow jumps ahead to a later phase.** flow learns a Codex worker's phase from the skill
files it reads, so a worker that reads the next skills' `SKILL.md` early seems to start their phases
early. A line in your skills such as "read each skill only when you start it" helps.

**A Codex worker asks permission for every `git commit`.** Codex's sandbox only lets it write inside
its worktree, but a worktree's git data lives in the main repository's `.git`. Approve the commit in
its pane, or give Codex a sandbox setting that allows it.

**A flow is `not-responding`.** It reported heartbeats with `flow report heartbeat` and stopped for
90 seconds. Claude Code and Codex workers send no heartbeats; a session of theirs that stopped shows
`exited` in the sidebar instead.

**A session `exited` or `failed`.** `exited` means its process ended. `failed` means it couldn't
start, for example because its command isn't installed. Jump to it, then press `r` to resume it or
`x` to clean up the flow. `server.log` has the reason a start failed.

## Starting flows

**`flow start` says it needs a git repository.** Run it inside the repository the flow works on.
For an agent in any folder, use `flow start --plain <folder>`.

**`setup: failed: ...`.** The repository's `.flow/setup` exited with an error, and the text is its
last error line. The flow is blocked, but the worker opened anyway so you can look into it. Fix
the script, then remove the flow and start it again. The
[setup-flow](repository.md#the-setup-flow-skill) skill writes or repairs the script.

**`all 20 slots are in use`.** Twenty unfinished flows, across all your projects, hold every slot.
`flow status --all` shows them, and `flow clear` in a project with finished flows frees theirs.

**`... is not valid` about `config.toml`.** The message names the file, the line and the key. A key
from an earlier version names its new form (see [configuration.md](configuration.md#errors)).

**The request is too long.** `flow start` takes up to 20000 characters. Put the details in a file
in the repository and name the file in the request.

## Removing flows

**`flow remove` refuses: uncommitted work.** The worktree has changes that are not committed.
Commit or stash them, or pass `--force` to discard them.

**`flow remove` refuses: commits on no other branch.** The flow's branch has commits that exist
nowhere else, local or pushed. Merge or push the branch, or pass `--delete-unmerged` to delete it
with its commits.

**`flow clear` kept a worktree.** It had uncommitted work. Commit or discard it, then remove it with
`git worktree remove <path>`.
