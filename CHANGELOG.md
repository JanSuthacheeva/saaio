# Changelog

All notable changes to flow are listed here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and flow uses
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] - not released yet

The first public release. The date goes here once it's tagged. What comes next is in
[ROADMAP.md](ROADMAP.md).

### Added

- flow ui, one terminal for the master, every worker and every plain session, with the sidebar, the
  peek, the board and the new-flow dialog.
- flow server, so sessions outlive the UI and resume after a crash or a reboot.
- Claude Code and Codex workers, followed through their own hooks. Any other command can run as a
  worker too, and flow shows whether it's running.
- Your own workflow: one to four phases, the skills that start them, and every agent's first
  prompt.
- The master, which starts and answers workers and is woken when they need something.
- Guided and auto flows, and plain sessions.
- Setup and teardown scripts per worktree, `FLOW_SLOT` for ports, and branch name conventions per
  repository.
- Themes: your terminal's own colors, flow's dark and light themes, live reload, and themes written
  by another tool.
- A command line for agents (TOON), and `flow status --json` for programs.
- Example `config.toml` and `theme.toml`, every setting commented out, written on the first run.
