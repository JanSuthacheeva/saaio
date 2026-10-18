# Changelog

All notable changes to saaio are listed here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and saaio uses
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] - not released yet

The first public release. The date is added when it is tagged. Planned next:
[ROADMAP.md](ROADMAP.md).

### Added

- saaio ui: the master, every worker and every plain session in one terminal, with the sidebar, the
  peek, the board and the new-flow dialog.
- saaio server: sessions outlive the UI and resume after a crash or a reboot.
- Claude Code workers, and Codex workers as experimental, followed through their own hooks. Any other command as a worker,
  shown as running or not.
- Configurable workflow: 1 to 4 phases, the skills that start them, every agent's first prompt.
- The master: starts and answers workers, woken by their news.
- Guided and auto flows. Plain sessions.
- One store, server and master per git repository, `SAAIO_HOME` to choose another.
- Per-worktree setup and teardown scripts, `SAAIO_SLOT` for ports, branch conventions per
  repository.
- Themes: the terminal's own colors, flow's dark and light themes, live reload, themes written by
  other tools.
- The command line for agents (TOON) and `saaio status --json` for programs.
- Commented example `config.toml` and `theme.toml`, written on the first run.
