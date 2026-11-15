# saaio documentation

## Getting started

| Page | Covers |
| --- | --- |
| [install.md](install.md) | requirements, installing saaio and the skills, agent settings, first run, updating, manual download, from source, shell completions |
| [ui.md](ui.md) | the terminal UI: screen, keys, new-flow dialog, peek, board, history, mouse and copying, notifications, stopping |

## Guides

| Page | Covers |
| --- | --- |
| [flows.md](flows.md) | flows, kinds, phases, states, guided and auto, the master, stacked flows, plain sessions, merge requests, hooks |
| [configuration.md](configuration.md) | `config.toml`: your own workflow, every setting, kinds of flows, agents |
| [agents.md](agents.md) | recommended worker permissions for Claude Code and Codex |
| [themes.md](themes.md) | `theme.toml`: built-in themes, every color, themes from other tools |
| [repository.md](repository.md) | per repository: worktrees, trust, setup and teardown, slots and ports, Docker, pausing idle services, branch names, heavy commands one at a time |
| [troubleshooting.md](troubleshooting.md) | where to look first, problems and fixes, reporting a bug |

## Reference

| Page | Covers |
| --- | --- |
| [cli.md](cli.md) | every command and option, exit codes, environment variables, several projects |
| [json.md](json.md) | the `saaio status --json` contract: the board, a flow, versions |
| [files.md](files.md) | every file saaio reads or writes, removing saaio |

## Project

| Page | Covers |
| --- | --- |
| [CHANGELOG.md](../CHANGELOG.md) | what changed in each release |
| [ROADMAP.md](../ROADMAP.md) | what is planned |

Example settings: [examples/config.toml](../examples/config.toml),
[examples/theme.toml](../examples/theme.toml).
