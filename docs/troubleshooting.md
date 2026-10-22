# Troubleshooting

## Where to look first

| Source | Shows |
| --- | --- |
| `saaio status <name> --full` | everything saaio knows about a flow, nothing truncated |
| `server.log` in the store (`~/.local/state/saaio/projects/<repository>/server.log`, `~/.local/state/saaio/server.log` outside a repository, or under `$SAAIO_HOME`; `saaio-<uid>/<hash>.log` in `$XDG_RUNTIME_DIR`, `$TMPDIR` or `/tmp` when the store's path is long, [files.md](files.md#the-store)) | sessions opened, resumed or failed to start, files that could not be written, why the server stopped. Also errors of the hooks in each worker (`hook ...`) and of the UI (`ui: ...`). |

Each `server.log` line starts with a time and a process id. The start line names the saaio
version and commit, the OS and the versions of `claude`, `codex`, `gh` and `glab`:

```
2026-10-18 13:39:54 +07:00 [2739] started: saaio 0.1.0 (e035b96), protocol 7, Darwin 25.6.0 arm64, claude 2.1.302 (Claude Code), codex 0.163.0, gh 2.73.0 (2025-05-19), glab missing
2026-10-18 13:41:02 +07:00 [2811] hook claude-code flow 3f2a9c: no flow 3f2a9c
```

## Reporting a bug

1. Run `saaio bug-report` in the repository where the problem happened.
2. Check the file it names before uploading it. It can hold flow names, requests and paths.
3. Open an [issue](https://github.com/JanSuthacheeva/saaio/issues/new?template=bug_report.yml)
   with what happened, what was expected and the steps, and attach the file.

```
$ saaio bug-report
report: /Users/jan/projects/shop/saaio-report-2026-10-18.txt
help[1]: "Check the file before uploading it: it can hold flow names, requests and paths"
```

The file holds:

- the saaio version, commit and protocol, the OS and the versions of `claude`, `codex`, `gh` and
  `glab`
- the store's path
- where `config.toml` and `theme.toml` are and whether they load, with the error when one does not
- the last 300 lines of `server.log` and `server.log.1`

The home folder is written `~`, also as the real path a linked home leads to and in Claude Code's
project names. A second report on the same day is written beside the first, as
`saaio-report-<date>-2.txt`.

## The terminal UI

| Problem | Cause | Fix |
| --- | --- | --- |
| `saaio server runs protocol 7, this saaio speaks 8` | after an update, the old server still runs | `saaio --stop`, then `saaio`. Unfinished flows resume. |
| `a saaio server of an earlier version still runs on /tmp/saaio-<uid>/...` | after updating from 0.1.0, its server of a store with a long path still runs | `saaio --stop`, then `saaio` |
| `saaio connects to its server only in a private folder: ... is open to others` | the socket's folder is not yours alone (`0700`) | `chmod 700` it if it is yours, else remove it, then `saaio` |
| a session's state no longer changes; server.log says `SAAIO_PROOF does not prove ...` | the session was not opened by the running server, such as one still open from before an update | `saaio --stop`, then `saaio`: every session reopens with its key |
| the prefix key does nothing | the terminal or a surrounding tmux keeps `C-space` | set another prefix, such as `prefix = "C-b"` under `[ui]` in `config.toml`. It applies on save ([configuration.md](configuration.md#ui)) |
| `saaio` prints the flows instead of opening the UI | it runs inside an agent or a worker (`CLAUDECODE`, `CODEX_THREAD_ID`, `SAAIO_FLOW_ID` or `SAAIO_MASTER` set), or stdin or stdout is not a terminal | run it in a terminal of its own |
| the UI or `saaio status` shows none of a project's flows | each git repository has its own store, picked from the folder saaio runs in | run it in that repository, or `saaio status --all` for every project ([cli.md](cli.md#several-projects)) |
| another terminal took over the UI | one UI is attached at a time, the newest wins | run `saaio` again in the terminal you want |
| colors wrong or unchanged | `theme.toml` is broken. The last working colors stay. | read the error in the title row ([themes.md](themes.md#errors)) |

## Workers

| Problem | Cause | Fix |
| --- | --- | --- |
| `pane: when saaio opens` | the flow is recorded, the UI is not running | run `saaio` in a terminal. It opens the worker within half a second. |
| the row shows only whether it runs | the agent has no `adapter` | set `adapter = "claude-code"` or `"codex"` on a custom agent that runs one of them ([configuration.md](configuration.md#agents)) |
| a Codex flow is waiting with "no turn began after launch" | its pane stopped at a Codex dialog before its first turn | jump to the worker and answer the dialog |
| steps stay `○───○───○───○`, the sidebar shows the state instead of a phase | no skill the worker ran matches a phase. The flow still works. | list your skills under `[[phases]]` ([configuration.md](configuration.md#use-your-own-workflow)) |
| a Claude Code worker keeps its spinner after Esc | Claude Code runs no hook on interrupt | none. The state corrects at the worker's next prompt. |
| a Codex flow shows no task counts and never ends | Codex has no task count tool and no `/exit` event | end it from the UI (`x` on its exited pane, or on the board when done) or with `saaio remove` |
| a Codex flow does not change phase after reading a skill | one command read the skills of several phases, which saaio treats as read-ahead | the next single read of the skill's `SKILL.md` starts it, or `saaio report --flow <name> phase <phase>` |
| a Codex flow shows `waiting permission` under automatic review | saaio could not read the session's rollout file, so it counts the request as yours | read `server.log`. The flow returns to `working` when the tool finishes. |
| a Codex worker asks permission for every `git commit` | the sandbox only allows writes in the worktree. A worktree's git data is in the main repository's `.git`. | approve in its pane, or allow it in Codex's sandbox settings |
| a flow stays at `review` after its merge request was opened or merged | the UI is not running, `gh` or `glab` is missing or not logged in, or the remote's host is neither github.com nor a GitLab | read `server.log`, run `gh auth login` or `glab auth login`, keep the UI running. It asks every 2 minutes ([flows.md](flows.md#merge-requests-opened-or-merged-elsewhere)). |
| a flow is `not-responding` | it reported heartbeats (`saaio report heartbeat`) and stopped for 90 seconds | check the worker. Claude Code and Codex send no heartbeats. Their stopped sessions show `exited`. |
| a session shows `exited` | its process ended | jump to it: `r` resumes, `x` cleans up the flow |
| a session shows `failed` | it could not start, for example its command is not installed | read the reason in `server.log`, then `r` or `x` |

## Starting flows

| Problem | Cause | Fix |
| --- | --- | --- |
| `saaio start` needs a git repository | run outside a repository | run it in the repository, or use `saaio start --plain <folder>` |
| `setup: failed: ...` | `.saaio/setup` exited with an error. The text is its last error line. The flow is blocked, the worker is open. | fix the script, remove the flow, start it again. The [setup-saaio](repository.md#the-setup-saaio-skill) skill writes or repairs the script. |
| `all 20 slots are in use` | 20 unfinished flows across all projects | `saaio status --all` lists them. A done flow keeps its slot until its session ends: end it, or `saaio remove` flows you no longer need. |
| `config.toml:<line>: ...` | a broken config. The message names the file, line and key. | fix the key. A key from an earlier version names its new form ([configuration.md](configuration.md#errors)). |
| `the request is <n> chars, over 20000` | the request is too long | put the details in a file in the repository and name the file in the request |

## Removing flows

| Problem | Cause | Fix |
| --- | --- | --- |
| `saaio remove` refuses: uncommitted work | uncommitted changes in the worktree | commit or stash them, or `--force` to discard them |
| `saaio remove` refuses: git cannot check the worktree for uncommitted changes | git does not list the folder as a worktree of the repository, or git failed | `git worktree list` in the repository shows what git knows; `--force` removes it without the check |
| `saaio remove` refuses: commits on no other branch | the branch has commits that exist nowhere else, local or pushed | merge or push the branch, or `--delete-unmerged` to delete it with its commits |
| `saaio clear` kept a worktree | it had uncommitted work | commit or discard it, then `git worktree remove <path>` |
