# Roadmap

Planned after 0.1.0. Subject to change. Shipped: [CHANGELOG.md](CHANGELOG.md).

## Implementing

Nothing in progress.

## Planned soon

| Item | Today | Planned |
| --- | --- | --- |
| More agents | Claude Code and Codex are followed through their hooks | other harnesses followed the same way |
| Every subagent in the sidebar | a worker's background subagents do not show: it looks working while its turn runs, idle or waiting once it ends. | the sidebar counts every running subagent, the board lists their tasks |
| Working across repositories | each repository has its own UI. In one store shared through `SAAIO_HOME`, the new-flow dialog starts flows in the repository of the session in view. | pick the repository in the dialog, sidebar and board show each flow's repository, `saaio start --repo <path>` |
| Codex phases from the skill it starts | a Codex worker that reads a later skill's `SKILL.md` early seems to start that phase early | the phase follows the skill actually started |
| `saaio example config`, `saaio example theme` | the examples exist only as written on the first run | print every setting with its default |
| Docs site at saaio.dev | the docs are Markdown files in the repository, read on GitHub | a searchable docs site at saaio.dev, built from `docs/` on every merge |
| Shared memory for the master | each agent keeps its own memory (Claude Code per project, Codex none), so switching the master's agent loses what it knew | one memory per project in its store (`MEMORY.md` index, one file per note) that every master reads at the start and writes to, whatever its agent. Claude Code's memory folder is linked to it. Workers can read it too. |
| The master answers permission prompts | a worker waiting on a permission prompt reads a message only once the prompt is answered in its pane; `saaio send` names the prompt and says so. A Claude Code prompt denied in the pane still shows as open until the worker's next turn. | the master answers the prompt itself, approving or denying the tool call for the user |
| Answers typed in a worker's pane | the master learns only that a flow stopped. When you answer in the worker's pane first, the master can still relay the question it already has. | what you type in a worker's pane shows in its timeline, and news for a flow that left `waiting` before the master woke says it was answered |
| Mismatched saaio binaries | `saaio update` installs over the binary it runs from. Another saaio earlier on PATH (an old `cargo install`) keeps running, and a server started from it serves an older version than the command line: panes get no key, so `saaio send` and the hooks are refused | `saaio update` and the UI warn when another saaio comes first on PATH, and when the running server is another version than the command line, naming the binaries and `saaio --stop` |
| Package managers | prebuilt binaries come from the install script or the GitHub release, else `cargo install` builds from source | a Homebrew tap and `cargo binstall saaio` |
| Windows | saaio runs on macOS and Linux | Windows support: the UI and `saaio server` talk over something other than a Unix socket, panes run in ConPTY, `saaio start` claims slots without `flock`, and the `.saaio/setup` and `.saaio/teardown` hooks run without a Unix shell |

## Planned future

| Item | Today | Planned |
| --- | --- | --- |
| Flow names in shell completions | `saaio completions` completes commands, options and their values | `saaio send`, `saaio status` and `saaio remove` complete the names of the flows in the store, once clap_complete's completion through the binary is stable |
| The master asks a worker | no command for it | `saaio ask` puts the master's question to a worker and returns the answer, without waiting on the user |
| An interrupted Claude Code turn shown as idle | Claude Code runs no hook on Esc. The spinner stays until the next prompt. | the interrupted turn shows as idle |
| Task counts and an end event for Codex | a Codex flow shows no task counts and is ended from the UI or with `saaio remove` | both, as for Claude Code |
| Parts in a flow | a flow has one merge request and walks its phases once, so a ticket split into stacked merge requests shows every step done after the first | a plan declares its parts. Implement, review and merge request repeat for each, `saaio report merge-request` adds to a list of merge requests, and status shows `part 2 of 3`. |
| Screen reading as a fallback to hooks | saaio learns a worker's state only from its harness's hooks, so what they do not report goes unseen: a dialog or permission prompt open right now, or what a Codex worker held at a startup dialog is asked (it shows only as waiting, 30 seconds after launch) | the pane's screen, which saaio already emulates, fills those gaps and nothing more. Hooks stay the source of truth, since a screen pattern breaks silently when a harness changes its look. |
