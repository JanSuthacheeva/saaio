# Changelog

All notable changes to flow are listed here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and flow uses
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] - not released yet

The first public release. Its date goes here when it is tagged. What comes next is in
[ROADMAP.md](ROADMAP.md).

### Added

- flow ui: the master, every worker and every plain session in one terminal, with the sidebar,
  the peek, the board and the new-flow dialog.
- flow server: sessions outlive the UI, and resume after a crash or a reboot.
- Claude Code and Codex workers, followed through their own hooks; any other command as a worker
  that flow only watches run.
- A configurable workflow: 1 to 4 phases and the skills that start them, and every agent's first
  prompt.
- The master: it starts and answers workers, and is woken by their news.
- Guided and auto flows, and plain sessions.
- Per-worktree setup and teardown scripts, `FLOW_SLOT` for ports, and branch conventions per
  repository.
- Themes: the terminal's own colors, flow's dark and light themes, live reload, themes another
  tool writes.
- The command line for agents (TOON) and `flow status --json` for programs.
- Commented example `config.toml` and `theme.toml`, written on the first run.
