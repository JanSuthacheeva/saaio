# Files

Every file saaio reads or writes. All plain text. Only the config folder is meant for editing.

## The config folder

Location: the folder of `$SAAIO_CONFIG` if set, else `$XDG_CONFIG_HOME/saaio`, else `~/.config/saaio`.

| File | Holds |
| --- | --- |
| `config.toml` | settings ([configuration.md](configuration.md)) |
| `theme.toml` | colors ([themes.md](themes.md)) |
| `themes/<name>.toml` | themes `theme.toml` can extend, often written by another tool |
| `master.md` | the master's instructions ([flows.md](flows.md#the-master)) |

- `saaio server`, which the UI starts, writes `config.toml`, `theme.toml` and `master.md` when they
  are missing.
- An edited file is never overwritten.
- A newer saaio refreshes `master.md` while it still holds an earlier default.

## The store

Location, the first that applies:

1. `$SAAIO_HOME`
2. in a git repository: `projects/<repository>` in the shared folder. A worktree or a subfolder uses
   its main repository's store.
3. the shared folder itself (`$XDG_STATE_HOME/saaio`, else `~/.local/state/saaio`)

`<repository>` is the repository's folder name. A second repository with the same folder name gets
`<repository>-2`, a third `-3`. A store stays with its repository even once that repository is
gone: a repository moved to another folder starts with a new store, and its earlier flows stay
readable with `SAAIO_HOME=~/.local/state/saaio/projects/<repository>`.

The UI passes its store to every session it opens as `SAAIO_HOME`, so a worker reports into its
flow's store from any folder.

| File | Holds |
| --- | --- |
| `flows/<id>.jsonl` | one flow's events, one JSON line each, append-only |
| `flows/<id>.alive` | modification time is the flow's last heartbeat |
| `waits/` | wake-up state: each session's waiter, messages given to a worker, lines typed into a worker's pane, whether a session is busy, what the master last saw, the master's turn and its Codex session id, and a hash of each session's key |
| `server.sock` | the socket between the UI and the server, owner-only. If its path would be 100 bytes or longer: `saaio-<uid>/<hash>.sock` in `$XDG_RUNTIME_DIR`, else `$TMPDIR`, else `/tmp`, with the log next to it as `<hash>.log`. saaio uses that folder only while it is yours and closed to others (`0700`), and talks only to processes of your user. |
| `server.log` | what the server did and what failed, with the errors of the hooks and the UI. The start line names the versions of saaio, the OS and the tools saaio calls. Rotated to `server.log.1` past 1 MiB when the server starts. |
| `master.json` | the master's session id and folder, for resuming after a restart |
| `repo` | in a project store: the git folder of the repository it belongs to. A store without it is taken by the first repository of its name. |

The event log is private. Read flows with `saaio status --json` ([json.md](json.md)).

## The shared folder

Location: `$XDG_STATE_HOME/saaio`, else `~/.local/state/saaio`, regardless of `SAAIO_HOME`. Also
the store outside a repository.

| File | Holds |
| --- | --- |
| `projects/<repository>/` | one repository's store |
| `stores/<hash>` | one store's folder, so `saaio status --all` finds every project. Dropped when the folder is gone. |
| `slots/<n>` | the flow holding slot `n`. Counts only while that flow is unfinished. |
| `slots/.lock` | held while a slot is claimed, so two starts never get the same slot |
| `trusted/<hash>` | one repository you trusted with `saaio trust` or in the new-flow dialog ([repository.md](repository.md#trust)) |
| `reports/` | the files `saaio bug-report` writes ([troubleshooting.md](troubleshooting.md#reporting-a-bug)) |
| `update.json` | when the UI last looked for a newer release, and the latest it found ([configuration.md](configuration.md#ui)) |

## In a repository

| File | Holds | Committed |
| --- | --- | --- |
| `.saaio/config.toml` | the branch convention ([repository.md](repository.md#branch-names)) | yes |
| `.saaio/setup`, `.saaio/teardown` | the scripts run in each worktree ([repository.md](repository.md#setup-and-teardown)) | yes |
| `.saaio/pause`, `.saaio/resume` | the scripts that pause and resume a resting flow's services ([repository.md](repository.md#pause-and-resume)) | yes |
| `.saaio/worktrees/<name>` | each flow's worktree. `.saaio/worktrees/` is listed in `.git/info/exclude`. | no |

## Read elsewhere

| File | Purpose |
| --- | --- |
| `~/.claude/projects/...` (or under `$CLAUDE_CONFIG_DIR`) | a Claude Code worker's transcript, for the peek |
| `~/.codex/sessions/...` (or under `$CODEX_HOME`) | a Codex worker's transcript, the one its hooks name, for the peek. A file elsewhere is not read. |

saaio writes nothing to `~/.claude` or `~/.codex`. Hooks are passed on each agent's command line.

## Removing saaio

```sh
saaio --stop      # end the server and every session
saaio clear       # or saaio remove <name>, for flows whose worktrees you want gone
rm ~/.local/bin/saaio    # or cargo uninstall saaio, when built from source
rm -rf ~/.config/saaio ~/.local/state/saaio   # and any SAAIO_HOME you used
# and a completion script you saved, such as ~/.zfunc/_saaio
```
