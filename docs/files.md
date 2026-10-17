# Files

Every file flow reads or writes. All of them are plain text, and none needs editing by hand
except the three in the config folder.

## The config folder

The folder of `$FLOW_CONFIG` when it is set, else `$XDG_CONFIG_HOME/flow`, else `~/.config/flow`.

| File | Holds |
| --- | --- |
| `config.toml` | your settings (see [configuration.md](configuration.md)) |
| `theme.toml` | your colors (see [themes.md](themes.md)) |
| `themes/<name>.toml` | themes `theme.toml` can extend, often written by another tool |
| `master.md` | the master's instructions (see [flows.md](flows.md#the-master)) |

flow ui writes `config.toml`, `theme.toml` and `master.md` on its first run. It never overwrites
one you edited. A newer flow refreshes `master.md` while it still holds an earlier default.

## The store

Where flow keeps its flows: `$FLOW_HOME`, else `$XDG_STATE_HOME/flow`, else
`~/.local/state/flow`.

| File | Holds |
| --- | --- |
| `flows/<id>.jsonl` | one flow's events, one JSON line each, only ever appended to |
| `flows/<id>.alive` | its modification time is the flow's last heartbeat |
| `waits/` | what the sessions' wake-ups need: each session's waiter, which messages a worker was given, whether a Codex session is busy, what the master last saw, the master's turn and its Codex session id |
| `server.sock` | the socket flow ui talks to the server through; only you can open it. When its path would be 100 bytes or longer, it is `/tmp/flow-<uid>/<hash>.sock` instead, with the log beside it as `<hash>.log` |
| `server.log` | what the server did and what went wrong; past 1 MiB it moves to `server.log.1` |
| `master.json` | the master's session id and the folder it runs in, so it resumes after a restart |

The event log is private to flow: read flows through `flow status --json` (see [json.md](json.md)),
not from these files.

## The shared folder

`$XDG_STATE_HOME/flow`, else `~/.local/state/flow`, whatever `FLOW_HOME` says. With the default
store it is the same folder as the store.

| File | Holds |
| --- | --- |
| `stores/<hash>` | the folder of one store, so `flow status --all` finds every project; dropped when the folder is gone |
| `slots/<n>` | which flow holds slot `n`; a claim counts only while its flow is unfinished |
| `slots/.lock` | taken while a slot is claimed, so two starts never get the same one |

## In a repository

| File | Holds |
| --- | --- |
| `.flow/config.toml` | the branch convention (see [repository.md](repository.md#branch-names)); committed |
| `.flow/setup`, `.flow/teardown` | the scripts flow runs in each worktree (see [repository.md](repository.md#setup-and-teardown)); committed |
| `.flow/worktrees/<name>` | each flow's worktree; flow lists `.flow/worktrees/` in `.git/info/exclude`, so git ignores it |

## What flow reads elsewhere

| File | Why |
| --- | --- |
| `~/.claude/projects/...` (or under `$CLAUDE_CONFIG_DIR`) | a Claude Code worker's transcript, for the peek in flow ui |
| the transcript Codex reports | a Codex worker's transcript, for the peek |

flow writes nothing to `~/.claude` or `~/.codex`. It passes its hooks on each agent's command line.

## Removing flow

1. `flow ui --stop`, to end the server and every session.
2. `flow remove <name>` or `flow clear` for flows whose worktrees you want gone.
3. `cargo uninstall flow`.
4. Delete `~/.config/flow` and `~/.local/state/flow` (and any `FLOW_HOME` you used).
