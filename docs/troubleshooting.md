# Troubleshooting

## Where to look first

| Source | Shows |
| --- | --- |
| `flow status <name> --full` | everything flow knows about a flow, nothing truncated |
| `server.log` in the store (`~/.local/state/flow/server.log`, or under `$FLOW_HOME`) | sessions opened, resumed or failed to start, files that could not be written, why the server stopped |

Each `server.log` line starts with a time and a process id:

```
2026-10-16 13:39:54 +07:00 [2739] started (flow 0.1.0 1de3dc4, protocol 5)
```

## flow ui

| Problem | Cause | Fix |
| --- | --- | --- |
| `flow server runs protocol 4, this flow speaks 5` | after an update, the old server still runs | `flow ui --stop`, then `flow ui`. Unfinished flows resume. |
| the prefix key does nothing | the terminal or a surrounding tmux keeps `C-space` | set another prefix, such as `prefix = "C-b"` under `[ui]` in `config.toml`, then `flow ui --stop` and `flow ui` ([configuration.md](configuration.md#ui)) |
| flow ui refuses to start inside a worker | it would take over the flow ui the worker runs in | use a separate store: `FLOW_HOME=$(mktemp -d) flow ui` |
| a second flow ui took over the first | one flow ui is attached at a time, the newest wins | run `flow ui` again in the terminal you want |
| colors wrong or unchanged | `theme.toml` is broken. The last working colors stay. | read the error in the title row ([themes.md](themes.md#errors)) |

## Workers

| Problem | Cause | Fix |
| --- | --- | --- |
| `pane: when flow ui runs` | the flow is recorded, flow ui is not running | run `flow ui`. It opens the worker within half a second. |
| the row shows only whether it runs | the agent has no `adapter` | set `adapter = "claude-code"` or `"codex"` on a custom agent that runs one of them ([configuration.md](configuration.md#agents)) |
| a Codex worker reports nothing | Codex waits for you to review flow's hooks (first time, and after they change) | jump to the worker, pick "Trust all and continue" |
| steps stay `○───○───○───○`, the sidebar shows the state instead of a phase | no skill the worker ran matches a phase. The flow still works. | list your skills under `[[phases]]` ([configuration.md](configuration.md#use-your-own-workflow)) |
| the worker sits at a folder trust question | first time an agent opens in a new folder | jump to the worker and answer |
| a Claude Code worker keeps its spinner after Esc | Claude Code runs no hook on interrupt | none. The state corrects at the worker's next prompt. |
| a Codex flow shows no task counts and never ends | Codex has no task count tool and no `/exit` event | end it from flow ui (`x` on its exited pane, or on the board when done) or with `flow remove` |
| a Codex flow jumps to a later phase | flow reads a Codex worker's phase from the `SKILL.md` files it reads. Reading a later skill early starts its phase early. | add "read each skill only when you start it" to your skills |
| a Codex worker asks permission for every `git commit` | the sandbox only allows writes in the worktree. A worktree's git data is in the main repository's `.git`. | approve in its pane, or allow it in Codex's sandbox settings |
| a flow is `not-responding` | it reported heartbeats (`flow report heartbeat`) and stopped for 90 seconds | check the worker. Claude Code and Codex send no heartbeats. Their stopped sessions show `exited`. |
| a session shows `exited` | its process ended | jump to it: `r` resumes, `x` cleans up the flow |
| a session shows `failed` | it could not start, for example its command is not installed | read the reason in `server.log`, then `r` or `x` |

## Starting flows

| Problem | Cause | Fix |
| --- | --- | --- |
| `flow start` needs a git repository | run outside a repository | run it in the repository, or use `flow start --plain <folder>` |
| `setup: failed: ...` | `.flow/setup` exited with an error. The text is its last error line. The flow is blocked, the worker is open. | fix the script, remove the flow, start it again. The [setup-flow](repository.md#the-setup-flow-skill) skill writes or repairs the script. |
| `all 20 slots are in use` | 20 unfinished flows across all projects | `flow status --all` lists them. `flow clear` in a project with finished flows frees theirs. |
| `... is not valid` about `config.toml` | a broken config. The message names the file, line and key. | fix the key. A key from an earlier version names its new form ([configuration.md](configuration.md#errors)). |
| the request is too long | over 20000 characters | put the details in a file in the repository and name the file in the request |

## Removing flows

| Problem | Cause | Fix |
| --- | --- | --- |
| `flow remove` refuses: uncommitted work | uncommitted changes in the worktree | commit or stash them, or `--force` to discard them |
| `flow remove` refuses: commits on no other branch | the branch has commits that exist nowhere else, local or pushed | merge or push the branch, or `--delete-unmerged` to delete it with its commits |
| `flow clear` kept a worktree | it had uncommitted work | commit or discard it, then `git worktree remove <path>` |
