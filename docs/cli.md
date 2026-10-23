# The command line

- Output: [TOON](https://toonformat.dev), ending in `help[]` lines with suggested next commands.
- Programs read `saaio status --json` ([json.md](json.md)).
- Output below is from real runs, paths shortened.

- [Commands at a glance](#commands-at-a-glance)
- [saaio](#saaio)
- [saaio start](#saaio-start)
- [saaio status](#saaio-status)
- [saaio send](#saaio-send)
- [saaio report](#saaio-report)
- [saaio new](#saaio-new)
- [saaio remove](#saaio-remove)
- [saaio clear](#saaio-clear)
- [saaio trust](#saaio-trust)
- [saaio bug-report](#saaio-bug-report)
- [saaio completions](#saaio-completions)
- [saaio update](#saaio-update)
- [Commands saaio runs itself](#commands-saaio-runs-itself)
- [Output, errors and exit codes](#output-errors-and-exit-codes)
- [Environment variables](#environment-variables)
- [Several projects](#several-projects)

## Commands at a glance

| Command | Does |
| --- | --- |
| `saaio` | the terminal UI in your terminal. Every flow and what needs you for an agent or a script. |
| `saaio --stop` | stop the server and every session |
| `saaio start <request> [--name] [--mode] [--agent] [--type]` | start a worker in its own worktree |
| `saaio start --plain <folder> [--name] [--agent]` | start an agent in a folder, without a flow |
| `saaio status [<flow>] [--json] [--full] [--all]` | where flows stand |
| `saaio send <flow> <text>` | tell a worker something |
| `saaio send --type <flow> <line>` | type a line into a worker's pane, such as `/compact` |
| `saaio report [--flow <flow>] <event>` | add an event to a flow by hand |
| `saaio new --name <name> --request <text> [--mode]` | track a flow without starting a worker |
| `saaio remove <flow> [--force] [--delete-unmerged]` | end a flow and tear down its worktree |
| `saaio clear` | forget finished flows and tear down their worktrees |
| `saaio trust [<folder>] [--revoke] [--list]` | let saaio run a repository's `.saaio` scripts |
| `saaio bug-report` | write a file to attach to a bug report |
| `saaio completions <shell>` | print the completion script for a shell |
| `saaio update [--version <tag>]` | install the latest release over this binary |

- `<flow>`: a flow's name or id. A name means the unfinished flow of that name, else the most recent
  finished one.
- `saaio --version` prints the version and commit: `saaio 0.1.0 (e035b96)`, or `unknown` for a
  build from source outside a git checkout of saaio ([install.md](install.md#from-source)). Every
  command takes `--help`.

## saaio

```
saaio
saaio --stop
```

### In a terminal

- Opens the terminal UI ([ui.md](ui.md)). Starts the session server if none runs.
- On leaving, prints what happened, such as
  ``detached - the agents keep running; run `saaio` to attach again``.
- A broken `config.toml` stops it before the server starts.

### Run by an agent or a script

Flow counts, one line per flow, and the question or reason of each waiting or blocked flow.

- An agent or a script: stdin or stdout is not a terminal, or `SAAIO_FLOW_ID`, `SAAIO_MASTER=1`,
  `CLAUDECODE` or `CODEX_THREAD_ID` is set.
- The UI never opens there, so a worker or the master can run `saaio` safely.

```
$ saaio
bin: ~/.local/bin/saaio
description: "Tracks parallel feature flows: what each is doing, where it is blocked, how far it got"
count: 3
needs_you: 2
flows[3]{name,state,phase,tasks,age,services}:
  shop-221,waiting,plan,"",2s,""
  shop-217,blocked,implement,4/9,2s,""
  scratch,idle,"","",11s,""
blockers[2]{name,blocker}:
  shop-221,Should refunds count against the original order's totals or the refund date's?
  shop-217,"Task 5: the migration needs a default value"
help[2]: "Run `saaio status <name>` for one flow's phases, timeline and decisions",Run `saaio new --name <name> --request <text>` to track a new flow
```

- Order: needs you, ready (turn ended, nothing asked), running, finished. Most recent change first
  within each group.
- `age`: time since the flow's state, phase or blocker last changed.
- No flows: `flows: none tracked`. A log that cannot be read adds a `warnings[]` line.
- `services`: empty while the flow's services run or were never paused; else `pausing`, `paused`,
  `resuming`, `pause-failed` or `resume-failed` ([repository.md](repository.md#pause-and-resume)).

### --stop

- Stops the server and every session, typed in a terminal or run by an agent or a script.
- Unfinished flows are kept and resume the next time `saaio` opens the UI.
- Prints `stopped the saaio server and every session in it` or `no saaio server runs`.
- Inside a worker, refused on the worker's own store (it would end your sessions). Set
  `SAAIO_HOME` to another folder to stop a server there.

## saaio start

```
saaio start <request> [--name <name>] [--mode guided|auto] [--agent <agent>] [--type <type>]
saaio start --plain <folder> [--name <name>] [--agent <agent>]
```

Run in the repository the flow works on. Steps:

1. refuses a repository not [trusted](repository.md#trust) yet when it has `.saaio` scripts or the
   agent is Codex
2. names the flow
3. claims a slot (1 to 20, [repository.md](repository.md#slots-and-ports))
4. creates a git worktree in `.saaio/worktrees/<name>` on a new branch
5. runs the repository's `.saaio/setup` there, if present, for at most 10 minutes
6. records the flow, so the UI opens the agent in the worktree

```
$ saaio start "SHOP-217 add CSV export"
flow: shop-217
id: e514f1de-604d-4dbc-9b88-2cd76333d0a4
worktree: ~/projects/shop/.saaio/worktrees/shop-217
branch: flow/shop-217
slot: 1
setup: none
agent: claude
pane: when saaio opens
help[2]: Run `saaio` in a terminal to open the worker,Run `saaio` for all flows
```

| Field | Values |
| --- | --- |
| `setup` | `ran`, `none` (no executable `.saaio/setup`), `failed: <its last error line>`. A failed setup marks the flow blocked. The worker still opens. |
| `pane` | `opening` while the UI runs, else `when saaio opens` |

| Option | Default | Effect |
| --- | --- | --- |
| `<request>` | | what to build, such as `"SHOP-217 add CSV export"`. Up to 20000 characters. Part of the agent's first prompt. |
| `--name <name>` | from the request | the flow's name, lowercased, with anything but letters and digits turned into `-` (`Skills Reporting` becomes `skills-reporting`). Default: the ticket key at the start of the request, lowercased (`SHOP-217` becomes `shop-217`), else the first four words (`add-a-dark-mode`). A name held by an unfinished flow, given or not, gets `-2`, `-3` and so on. |
| `--mode <mode>` | `guided` | `guided` or `auto`: who answers the worker's questions ([flows.md](flows.md#guided-and-auto)) |
| `--agent <agent>` | `[worker] agent`, else `claude` | an agent from the config ([configuration.md](configuration.md#agents)) |
| `--type <type>` | the repository's first type | a branch type from the repository's `.saaio/config.toml` ([repository.md](repository.md#branch-names)) |
| `--plain <folder>` | | open the agent in this folder as a plain session |

The branch is `flow/<name>` unless the repository sets a convention. An existing branch gets `-2`,
`-3` and so on.

### Plain sessions

`--plain <folder>`: an agent in any folder, repository or not. No worktree, branch, slot or first
prompt. Ignored by the master ([flows.md](flows.md#plain-sessions)).

```
$ saaio start --plain ~/projects/scratch
flow: scratch
id: 5418e55e-581e-4117-a46c-0824b8e475d9
folder: ~/projects/scratch
agent: claude
pane: when saaio opens
help[2]: Run `saaio` in a terminal to open the session,Run `saaio` for all flows
```

- Name: the folder's name, unless `--name`.
- A leading `~` is the home folder.
- Not combinable with a request, `--mode` or `--type`.

## saaio status

```
saaio status [<flow>] [--json] [--full] [--all]
```

Without a flow: every flow, as `saaio`. With a flow: its request, mode, state, phases with time
spent, tasks, decisions and timeline.

```
$ saaio status shop-217
name: shop-217
id: e514f1de-604d-4dbc-9b88-2cd76333d0a4
request: SHOP-217 add CSV export
mode: guided
state: blocked
phase: implement
blocker: "Task 5: the migration needs a default value"
tasks: 4/9
branch: flow/shop-217
worktree: ~/projects/shop/.saaio/worktrees/shop-217
worktree_exists: true
steps[4]{phase,status,duration}:
  plan,done,5s
  implement,current,6s
  review,open,""
  merge-request,open,""
decisions[1]: "approach B: one exporter per format"
timeline[5]{ago,text}:
  11s,started
  11s,"phase: plan"
  8s,"decided: approach B: one exporter per format"
  6s,"phase: implement"
  2s,"blocked: Task 5: the migration needs a default value"
help[2]: Run `saaio report --flow shop-217 working` once the block is resolved,Run `saaio` for all flows
```

| Field | Shown |
| --- | --- |
| `blocker` | while there is one |
| `questions`, `options` | while the worker waits on a question dialog: each question with its header and whether it takes `one` option or `several`, then each option's label and description by question number |
| `delegate` | while a subagent works (what the worker handed to it) |
| `tasks` | with task counts |
| `review`, `merge_request` | once reported |
| `merged` | once the merge request is merged |
| `branch`, `worktree`, `worktree_exists` | for a flow with a worktree. `worktree_exists` is false once the folder is gone. |
| `folder` | for a plain session |
| `answer` | the closing message of the worker's last turn |
| `services`, `services_error` | while the flow's services are not running: `paused`, `resume-failed`, ..., and the last error line of a failed pause or resume |

A Claude Code worker that asks with a question dialog (AskUserQuestion) shows its questions and
options:

```
state: waiting
blocker: Which color?
questions[2]{n,header,question,pick}:
  1,Color,Which color?,one
  2,Toppings,Which toppings?,several
options[6]{question,n,label,description}:
  1,1,Red,"A warm, bold color"
  1,2,Green,"A calm, natural color"
  1,3,Blue,"A cool, steady color"
  2,1,Cheese,Melted cheese
  2,2,Ham,Sliced ham
  2,3,Olives,Black olives
```

| Option | Effect |
| --- | --- |
| `--json` | the JSON for programs instead of TOON ([json.md](json.md)) |
| `--full` | long texts in full. Without it, a text over 80 characters is cut and marked `(truncated, 87 chars - use --full)`. |
| `--all` | every project's flows, each line with its project ([Several projects](#several-projects)) |

## saaio send

```
saaio send <flow> <text>
```

The master's message to a worker: an answer, instruction or correction. Adds `master: <text>` to
the timeline and wakes the worker, or delivers it when the worker's session runs again.

```
$ saaio send shop-221 "Count them against the refund date."
sent: shop-221
state: waiting
help[2]: "saaio wakes its worker with it now, or once its session runs again",saaio wakes you when shop-221 changes again
```

A worker waiting on a question dialog cannot read a message while the dialog is open. Answer
every question in one message, by option label or in words. saaio's server then presses Esc in the
worker's pane, after ten seconds without typing there, so the dialog closes and the worker reads
the message as its answer. saaio presses Esc once per dialog: one still open after it waits for an
answer in the worker's pane. Answered in the pane first, the dialog is left alone. The message
waits while the UI is closed.

```
$ saaio send shop-217 "The user says: Blue; Cheese and Olives"
sent: shop-217
state: waiting
help[2]: saaio closes shop-217's question dialog and its worker reads this as the answer to every question in it,saaio wakes you when shop-217 changes again
```

| Refused | Reason |
| --- | --- |
| outside the master's session | `only the master sends`. Workers end their turn with the question. |
| an ended flow, a flow without a worker, a plain session | no worker to receive it. An open PR alone does not stop sends. |
| an empty text, a text over 4000 characters | |

### --type

```
saaio send --type <flow> <line>
```

Types the line and Enter into the worker's pane, as if the user typed it there, so a slash command
or a skill runs: `/compact`, `/review`. Adds `master typed: <line>` to the timeline.

```
$ saaio send --type shop-221 /compact
queued: shop-221
line: /compact
ahead: 0
help[2]: saaio types it into shop-221's pane once its session is idle,"Queue the next line with another `saaio send --type shop-221 \"<line>\"`. It waits for this one's turn to end"
```

- The line waits in the flow's log until saaio's server types it. `ahead` counts the lines queued
  before it.
- One line per call. Lines are typed in the order they were queued, each once the turn the line
  before it began has ended.
- A line is typed only into an idle session, after the session's first turn, and after ten seconds
  without typing in that pane.
- A line that begins no turn (a command that only opens a dialog, such as `/cost`) holds the next
  line for 30 seconds.
- Claude Code runs no hook on Esc. After an Esc in a Claude Code worker, its lines wait until the
  next prompt typed in that pane ends its turn.
- Lines wait while the UI is closed and are typed once it opens again.

```sh
saaio send --type shop-221 /compact
saaio send --type shop-221 "continue shipping"
```

| Refused | Reason |
| --- | --- |
| a line with a newline | `a typed line is one line`. Queue one call per line. |
| an agent without an adapter | it reports no turns, so saaio cannot tell when it is idle |
| everything `saaio send` refuses | |

## saaio report

```
saaio report [--flow <flow>] <event>
```

- Adds one event to a flow. For scripts, other tools and manual fixes. Workers report through their
  hooks ([flows.md](flows.md#how-a-worker-reports)).
- The flow: `--flow`, else `$SAAIO_FLOW_ID` (set in every worker). With neither, it does nothing and
  succeeds, so a skill can report outside a flow:

```
$ saaio report working
skipped: no flow id
help[1]: Set SAAIO_FLOW_ID or pass --flow <name>
```

| Event | Means |
| --- | --- |
| `phase <name>` | the flow entered a phase of its workflow (`plan`, `implement`, `review`, `merge-request` by default). A phase the flow does not have is refused. |
| `working` | work goes on, after a question, a block or a subagent |
| `delegated <description>` | a subagent works on the flow |
| `waiting <question>` | the flow waits for your answer |
| `idle <note>` | the turn ended without a question |
| `blocked <reason>` | the flow cannot go on |
| `decision <text>` | a decision made on your behalf |
| `tasks <done> <total>` | how many of the plan's tasks are done |
| `review --fixed <n> --open <n>` | review findings fixed and still open |
| `merge-request <url>` | the merge request is open. The flow shows `done` until its worker reports more activity; the session remains reachable. saaio keeps it as `.../pull/<n>` or `.../-/merge_requests/<n>`, without a trailing `/`, `?query`, `#fragment` or a page such as `/files`. Any other URL, or a merge request of another repository than any of the flow's remotes, is refused. |
| `ended` | the worker's session ended. The UI does not reopen it. |
| `heartbeat` | the worker is alive. Adds no event (see `not-responding` in [flows.md](flows.md#states)). |

Each answers with the flow's new state:

```
$ saaio report --flow shop-217 blocked "Task 5: the migration needs a default value"
flow: shop-217
state: blocked
phase: implement
help[1]: Run `saaio report working` once the block is resolved
```

- The same state, phase, tasks or decision reported twice changes nothing. Retries are safe.
- `review`, `merge-request` and `ended` add a timeline entry every time.
- Texts are kept up to 500 characters.

## saaio new

```
saaio new --name <name> --request <text> [--mode guided|auto]
```

Tracks a flow without a worktree or worker, for a worker run by hand. An existing unfinished flow
of that name is returned (`existing: true`). The request: up to 20000 characters, as for
`saaio start`.

```
$ saaio new --name shop-230 --request "SHOP-230 tracked by hand"
flow: shop-230
id: a825cdf3-a951-4333-bae2-e0c872be66bf
existing: false
help[2]: Run `saaio report --flow shop-230 phase plan` when plan starts,Or set SAAIO_FLOW_ID=a825cdf3-a951-4333-bae2-e0c872be66bf in the worker's environment and leave out --flow
```

## saaio remove

```
saaio remove <flow> [--force] [--delete-unmerged]
```

Ends one flow in any state: closes the worker's session in the UI, runs `.saaio/teardown`, removes
the worktree, deletes the branch, forgets the flow.

```
$ saaio remove shop-217
removed: shop-217
pane: none
teardown: none
worktree: removed
branch: deleted
help[1]: Run `saaio` to see what is left
```

Refuses and changes nothing when the worktree has uncommitted work, git cannot check it (git does
not list it as a worktree of the repository), or the branch has commits on no other branch, local
or pushed:

```
$ saaio remove shop-221
error: flow/shop-221 has 1 commit on no other branch
help[2]: "Merge or push flow/shop-221, then run `saaio remove shop-221` again",Run `saaio remove shop-221 --delete-unmerged` to delete it with its commits
```

| Field | Values |
| --- | --- |
| `pane` | `closed`, `none` or `timed out` |
| `teardown` | `ran`, `none` (no worktree or no executable `.saaio/teardown`), `failed: <its last error line>`, or `not run, the repository is not trusted` |
| `worktree` | `removed`, `none` or `kept: <path>`, with a help line to discard what is left |
| `branch` | `deleted`, `kept` or `none` |

| Option | Effect |
| --- | --- |
| `--force` | also remove a worktree with uncommitted work, discarding it, or one git cannot check |
| `--delete-unmerged` | also delete a branch whose commits are on no other branch |

A plain session: only the session is forgotten. The folder is never touched.

## saaio clear

```
saaio clear
```

Forgets every ended flow and tears down its worktree like `saaio remove`,
except:

- a worktree with uncommitted work is kept and listed under `kept`
- a flow whose teardown did not run, since the repository is no longer trusted, is listed under
  `untrusted`
- a branch is deleted only when all its commits are on another branch
- a pushed branch with its merge request stays on the remote

```
$ saaio clear
cleared: 2
names[2]: add-a-dark-mode,shop-230
kept[1]{name,path}:
  shop-230,~/projects/shop/.saaio/worktrees/shop-230
help[2]: Run `saaio` to see what is left,"Commit or discard the work in a kept worktree, then remove it with `git worktree remove <path>`"
```

## saaio trust

```
saaio trust [<folder>] [--revoke]
saaio trust --list
```

Trusts the repository `<folder>` is in (default: the current folder), so saaio runs its `.saaio`
scripts and lets Codex workers trust its worktrees ([repository.md](repository.md#trust)). Only a
person trusts: run without a terminal, or in an agent's or a worker's shell, it refuses.

```
$ saaio trust
trusted: ~/projects/shop
scripts[2]: .saaio/setup,.saaio/teardown
help[1]: Run `saaio trust --revoke ~/projects/shop` to take it back
```

| Option | Effect |
| --- | --- |
| `--revoke` | takes the trust back: `revoked: <repository>`, or `revoked: none` when it was not trusted. Works anywhere, also on a folder deleted since (pass the path `--list` shows). |
| `--list` | every trusted repository |

## saaio bug-report

```
saaio bug-report
```

Writes `saaio-report-<date>.txt` in the current folder, or `saaio-report-<date>-2.txt` and on
when that name is taken, readable only by you: the
versions of saaio, the OS, `claude`, `codex`, `gh` and `glab`, the store's path, whether
`config.toml` and `theme.toml` load, and the last 300 lines of `server.log` and `server.log.1`.
The home folder is written `~`, also as the real path a linked home leads to and in Claude Code's
project names (`-Users-you-...`). Works without a store, and then says why there is none.

```
$ saaio bug-report
report: /Users/jan/projects/shop/saaio-report-2026-10-18.txt
help[1]: "Check the file before uploading it: it can hold flow names, requests and paths"
```

How to report a bug: [troubleshooting.md](troubleshooting.md#reporting-a-bug).

## saaio completions

```
saaio completions <shell>
```

Prints the completion script for `bash`, `zsh`, `fish`, `elvish` or `powershell` on stdout, as
the shell reads it, not as TOON. It covers every command `saaio --help` lists, with their options
and values. Works without a store. Where each shell keeps it: [install.md](install.md#shell-completions).

```
$ saaio completions tcsh
error: invalid value 'tcsh' for '<SHELL>'; one of bash | elvish | fish | powershell | zsh
help[1]: Run `saaio completions --help` for the usage
```

## saaio update

```
saaio update [--version <tag>]
```

Installs the latest release, or the tag `--version` names (a pre-release too), over the `saaio`
being run, in the folder it is in. It runs that release's `install.sh`, which checks the download
against `SHA256SUMS` and renames the new binary over the old one. Works without a store.

```
$ saaio update
updated: saaio 0.1.0 -> 0.1.1
binary: ~/.local/bin/saaio
help[2]: A running saaio server keeps 0.1.0 until `saaio --stop`; unfinished flows resume after,"Changes: https://github.com/JanSuthacheeva/saaio/releases/tag/v0.1.1"
```

- Already on the latest release (or a newer pre-release), it changes nothing and says so.
  `--version` installs the tag it names even when it is older.
- The latest release never is a pre-release. Install one with `--version v0.1.2-rc.1`.
- A build from source (`cargo install`) is not replaced. It names the `cargo install` line for the
  new tag instead.
- A folder you cannot write, such as `/usr/local/bin`, is not touched. It names the `sudo` line
  that runs `install.sh` there.
- The running server and the UI attached to it keep the old version until `saaio --stop`.

```
$ saaio update
error: "this saaio was built from source, so saaio update does not replace it"
help[2]: "Run `cargo install --locked --git https://github.com/JanSuthacheeva/saaio --tag v0.1.1 --force`","Or install the release binary instead: docs/install.md"
```

## Commands saaio runs itself

Not listed in `saaio --help`. Not for manual use.

| Command | Run by |
| --- | --- |
| `saaio server` | `saaio`, when it opens the UI and no server runs. Owns every session. Log: `server.log` ([files.md](files.md)). |
| `saaio hook claude-code`, `saaio hook codex` | the hooks saaio adds to each worker. Reads the hook's event on stdin (an event over 16 MiB is skipped) and reports what changes the flow. Never prints to stdout, never fails a worker's turn. Its errors go to `server.log`. For Codex, exits 2 at Stop to hand over news. |
| `saaio wait [--master]` | Claude Code sessions, as a background hook. Sleeps until there is news for the session, prints it to stderr and exits 2, which wakes the session. `--master` waits for the master's news. |

## Output, errors and exit codes

Errors are TOON on stdout, with next steps:

```
$ saaio report --flow shop-999 working
error: no flow named shop-999
help[2]: Run `saaio` to list the flows,Run `saaio new --name shop-999 --request <text>` to start it
```

| Exit code | Means |
| --- | --- |
| 0 | success |
| 1 | an error, such as a flow that does not exist |
| 2 | a usage error, such as an unknown option. `saaio wait` also exits 2 when it has news. |

## Environment variables

Read by saaio:

| Variable | Effect |
| --- | --- |
| `SAAIO_HOME` | the folder saaio keeps its flows in. Default: the store of the git repository saaio runs in, else the shared folder (`$XDG_STATE_HOME/saaio`, else `~/.local/state/saaio`). [files.md](files.md#the-store) |
| `SAAIO_CONFIG` | the config file. Default: `$XDG_CONFIG_HOME/saaio/config.toml`, else `~/.config/saaio/config.toml`. |
| `XDG_STATE_HOME`, `XDG_CONFIG_HOME`, `HOME` | base of the defaults above |
| `SAAIO_FLOW_ID` | the flow `saaio report`, `saaio hook` and `saaio wait` act for. Set in every worker. While it is set, `saaio` prints the flows instead of opening the UI. |
| `SAAIO_MASTER` | `1` in the master's session, whose hooks report no flow |
| `SAAIO_PROOF` | the key the server hands each session it opens. `saaio hook` and `saaio wait` act for `SAAIO_FLOW_ID` or `SAAIO_MASTER` only with that session's key, and `saaio send` only with the master's. |
| `XDG_RUNTIME_DIR`, `TMPDIR` | where the socket goes when the store's path is too long for one ([files.md](files.md#the-store)) |
| `CLAUDECODE`, `CODEX_THREAD_ID` | set by Claude Code or Codex. `saaio` prints the flows instead of opening the UI. |
| `SSH_CONNECTION`, `SSH_TTY`, `WAYLAND_DISPLAY`, `DISPLAY` | how the UI copies: OSC 52 only over SSH, else also `pbcopy`, `wl-copy` or `xclip` ([ui.md](ui.md#copying)) |
| `CLAUDE_CONFIG_DIR` | Claude Code's folder, where the UI reads a worker's transcript for the peek. Default: `~/.claude`. |
| `SAAIO_RELEASES_URL` | a mirror of the [releases page](https://github.com/JanSuthacheeva/saaio/releases), laid out like it (`latest/download/VERSION`, `download/<tag>/...`; `file://` works). Read by the UI's update check and `saaio update`. |

Set by saaio in every worker's session and in the setup, teardown, pause and resume hooks:

| Variable | Value |
| --- | --- |
| `SAAIO_FLOW_ID` | the flow's id |
| `SAAIO_FLOW_NAME` | the flow's name |
| `SAAIO_SLOT` | the flow's slot, 1 to 20 |
| `SAAIO_WORKTREE` | the worktree's path |
| `SAAIO_REPO` | the repository's path |

- A plain session gets only `SAAIO_FLOW_ID`.
- Every session, the master's too, gets its own `SAAIO_PROOF`. Hooks get none.
- Every session also gets `SAAIO_HOME` (the store of the UI that opened it), `TERM=xterm-256color`
  and `COLORTERM=truecolor`.

## Several projects

Default: one store per git repository, picked from the folder saaio runs in
([files.md](files.md#the-store)). Each store has its own flows, server and master: run `saaio`
in a repository for its sidebar and master. Outside a repository saaio uses the shared folder.

`SAAIO_HOME` overrides the choice, for example to keep several repositories in one store:

```sh
export SAAIO_HOME=~/.local/state/saaio-work
```

Shared in the shared folder (`$XDG_STATE_HOME/saaio`, else `~/.local/state/saaio`) regardless of
`SAAIO_HOME`:

| Shared | Purpose |
| --- | --- |
| store registry | `saaio status --all` and the board's `a` key find every store |
| slots | no two unfinished flows in any project share a slot |

```
$ saaio status --all
count: 3
needs_you: 2
flows[3]{project,name,state,phase,tasks,age,services}:
  shop,shop-221,waiting,plan,"",2s,""
  shop,shop-217,blocked,implement,4/9,2s,""
  shop,scratch,idle,"","",11s,""
blockers[2]{project,name,blocker}:
  shop,shop-221,Should refunds count against the original order's totals or the refund date's?
  shop,shop-217,"Task 5: the migration needs a default value"
help[2]: "Run `saaio status <name>` in a flow's project for its phases, timeline and decisions",Run `saaio status --all --json` for every project's board
```

- A project is named after its store's folder (`shop` above). The shared folder's store is `default`.
  A second repository of the same name has the store `<name>-2` and shows so. Two stores in folders
  of the same name, such as two `SAAIO_HOME`s, show as `<name>` and `<name> (2)`.
- Projects without flows are left out.
- A store whose folder is gone is dropped.
