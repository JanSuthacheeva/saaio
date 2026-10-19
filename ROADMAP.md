# Roadmap

Planned after 0.1.0. Subject to change. Shipped: [CHANGELOG.md](CHANGELOG.md).

## Implementing

Nothing in progress.

## Planned soon

| Item | Today | Planned |
| --- | --- | --- |
| More agents | Claude Code and Codex are followed through their hooks | other harnesses followed the same way |
| Desktop notifications | a waiting flow shows only in the sidebar and to the master | the UI has the terminal notify when a flow starts waiting or is blocked out of view. Switch in `config.toml`. |
| Every subagent in the sidebar | a worker with several background subagents looks merely working. Only the first shows, once its turn ends. | the sidebar counts every running subagent, the board lists their tasks |
| Working across repositories | each repository has its own UI. In one store shared through `SAAIO_HOME`, the new-flow dialog starts flows in the repository of the session in view. | pick the repository in the dialog, sidebar and board show each flow's repository, `saaio start --repo <path>` |
| Codex phases from the skill it starts | a Codex worker that reads a later skill's `SKILL.md` early seems to start that phase early | the phase follows the skill actually started |
| `saaio example config`, `saaio example theme` | the examples exist only as written on the first run | print every setting with its default |
| Kinds of flows | every flow walks the same `[[phases]]` and gets the same first prompt, so a fix or a research task runs through the feature workflow | named workflows (feature, fix, research), each with its own phases, skills, first prompt and branch type. `saaio start --kind fix`, a field in the new-flow dialog, the master picks one from the request. Research needs no branch to merge. |
| Docs site at saaio.dev | the docs are Markdown files in the repository, read on GitHub | a searchable docs site at saaio.dev, built from `docs/` on every merge |
| Shared memory for the master | each agent keeps its own memory (Claude Code per project, Codex none), so switching the master's agent loses what it knew | one memory per project in its store (`MEMORY.md` index, one file per note) that every master reads at the start and writes to, whatever its agent. Claude Code's memory folder is linked to it. Workers can read it too. |
| `saaio start --base <branch>` | a flow's worktree branches from what the repository has checked out, so a stack of merge requests cannot run as one flow each | `saaio start --base <branch>` creates the worktree from the given branch, one flow per merge request in the stack |
| Messages to a worker at a permission prompt | `saaio send` to a worker waiting on a permission prompt says sent, but the worker reads it only once the prompt is answered in its pane | `saaio send` says the prompt is open, or the master answers the prompt itself |
| Prebuilt binaries | install builds from source with `cargo install`, which needs Rust | binaries for macOS and Linux on every release, an install script at saaio.dev, a Homebrew tap and `cargo binstall saaio` |

## Planned future

| Item | Today | Planned |
| --- | --- | --- |
| The master asks a worker | no command for it | `saaio ask` puts the master's question to a worker and returns the answer, without waiting on the user |
| An interrupted Claude Code turn shown as idle | Claude Code runs no hook on Esc. The spinner stays until the next prompt. | the interrupted turn shows as idle |
| Task counts and an end event for Codex | a Codex flow shows no task counts and is ended from the UI or with `saaio remove` | both, as for Claude Code |
| Parts in a flow | a flow has one merge request and walks its phases once, so a ticket split into stacked merge requests shows every step done after the first | a plan declares its parts. Implement, review and merge request repeat for each, `saaio report merge-request` adds to a list of merge requests, and status shows `part 2 of 3`. |
| Continuous integration | tests, clippy and fmt run locally before a merge | GitHub Actions run them on every pull request and build the release binaries |
