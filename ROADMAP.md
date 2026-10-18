# Roadmap

Planned after 0.1.0. Subject to change. Shipped: [CHANGELOG.md](CHANGELOG.md).

## Implementing

Nothing in progress.

## Planned soon

| Item | Today | Planned |
| --- | --- | --- |
| More agents | Claude Code and Codex are followed through their hooks | other harnesses followed the same way |
| Desktop notifications | a waiting flow shows only in the sidebar and to the master | saaio ui has the terminal notify when a flow starts waiting or is blocked out of view. Switch in `config.toml`. |
| Every subagent in the sidebar | a worker with several background subagents looks merely working. Only the first shows, once its turn ends. | the sidebar counts every running subagent, the board lists their tasks |
| Working across repositories | the new-flow dialog starts flows in the repository of the session in view | pick the repository in the dialog, sidebar and board show each flow's repository, `saaio start --repo <path>` |
| Codex phases from the skill it starts | a Codex worker that reads a later skill's `SKILL.md` early seems to start that phase early | the phase follows the skill actually started |
| Codex commits without asking | the sandbox cannot write to the main repository's `.git`, so every commit asks permission | commits in the worktree without a prompt |
| First prompt read at saaio start | a `[worker] prompt` change needs a new `saaio ui`, while `[worker] agent` applies at once | the prompt applies at the next `saaio start` |
| `saaio example config`, `saaio example theme` | the examples exist only as written on the first run | print every setting with its default |
| Prebuilt binaries | install builds from source with `cargo install`, which needs Rust | binaries for macOS and Linux on every release, an install script at saaio.dev, a Homebrew tap and `cargo binstall saaio` |

## Planned future

| Item | Today | Planned |
| --- | --- | --- |
| The master asks a worker | no command for it | `saaio ask` puts the master's question to a worker and returns the answer, without waiting on the user |
| An interrupted Claude Code turn shown as idle | Claude Code runs no hook on Esc. The spinner stays until the next prompt. | the interrupted turn shows as idle |
| Task counts and an end event for Codex | a Codex flow shows no task counts and is ended from saaio ui or with `saaio remove` | both, as for Claude Code |
| Continuous integration | tests, clippy and fmt run locally before a merge | GitHub Actions run them on every pull request and build the release binaries |
