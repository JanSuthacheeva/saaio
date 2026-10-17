# Files

Every file flow reads or writes. All plain text. Only the config folder is meant for editing.

## The config folder

Location: the folder of `$FLOW_CONFIG` if set, else `$XDG_CONFIG_HOME/flow`, else `~/.config/flow`.

| File | Holds |
| --- | --- |
| `config.toml` | settings ([configuration.md](configuration.md)) |
| `theme.toml` | colors ([themes.md](themes.md)) |
| `themes/<name>.toml` | themes `theme.toml` can extend, often written by another tool |
| `master.md` | the master's instructions ([flows.md](flows.md#the-master)) |

- flow ui writes `config.toml`, `theme.toml` and `master.md` on its first run.
- An edited file is never overwritten.
- A newer flow refreshes `master.md` while it still holds an earlier default.

## The store

Location: `$FLOW_HOME`, else `$XDG_STATE_HOME/flow`, else `~/.local/state/flow`.

| File | Holds |
| --- | --- |
| `flows/<id>.jsonl` | one flow's events, one JSON line each, append-only |
| `flows/<id>.alive` | modification time is the flow's last heartbeat |
| `waits/` | wake-up state: each session's waiter, messages given to a worker, whether a Codex session is busy, what the master last saw, the master's turn and its Codex session id |
| `server.sock` | the socket between flow ui and the server, owner-only. If its path would be 100 bytes or longer: `/tmp/flow-<uid>/<hash>.sock`, with the log next to it as `<hash>.log`. |
| `server.log` | what the server did and what failed. Rotated to `server.log.1` past 1 MiB. |
| `master.json` | the master's session id and folder, for resuming after a restart |

The event log is private. Read flows with `flow status --json` ([json.md](json.md)).

## The shared folder

Location: `$XDG_STATE_HOME/flow`, else `~/.local/state/flow`, regardless of `FLOW_HOME`. The same
folder as the default store.

| File | Holds |
| --- | --- |
| `stores/<hash>` | one store's folder, so `flow status --all` finds every project. Dropped when the folder is gone. |
| `slots/<n>` | the flow holding slot `n`. Counts only while that flow is unfinished. |
| `slots/.lock` | held while a slot is claimed, so two starts never get the same slot |

## In a repository

| File | Holds | Committed |
| --- | --- | --- |
| `.flow/config.toml` | the branch convention ([repository.md](repository.md#branch-names)) | yes |
| `.flow/setup`, `.flow/teardown` | the scripts run in each worktree ([repository.md](repository.md#setup-and-teardown)) | yes |
| `.flow/worktrees/<name>` | each flow's worktree. `.flow/worktrees/` is listed in `.git/info/exclude`. | no |

## Read elsewhere

| File | Purpose |
| --- | --- |
| `~/.claude/projects/...` (or under `$CLAUDE_CONFIG_DIR`) | a Claude Code worker's transcript, for the peek |
| the transcript Codex reports | a Codex worker's transcript, for the peek |

flow writes nothing to `~/.claude` or `~/.codex`. Hooks are passed on each agent's command line.

## Removing flow

```sh
flow ui --stop      # end the server and every session
flow clear          # or flow remove <name>, for flows whose worktrees you want gone
cargo uninstall flow
rm -rf ~/.config/flow ~/.local/state/flow   # and any FLOW_HOME you used
```
