# Changelog

All notable changes to saaio are listed here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and saaio uses
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] - not released yet

The first public release. The date is added when it is tagged. Planned next:
[ROADMAP.md](ROADMAP.md).

An early version: expect rough edges, especially with Codex, which is experimental and not yet
well tested.

### Added

- The terminal UI (`saaio`): the master, every worker and every plain session in one terminal,
  with the sidebar, the peek, the board and the new-flow dialog.
- saaio server: sessions outlive the UI and resume after a crash or a reboot.
- Claude Code workers, and Codex workers as experimental, followed through their own hooks. Any
  other command as a worker, shown as running or not.
- Configurable workflow: 1 to 4 phases, the skills that start them, every agent's first prompt.
- The master: starts and answers workers, woken by their news, types a line such as `/compact`
  into a worker's pane (`saaio send --type`).
- Guided and auto flows. Plain sessions.
- Merge requests on GitHub and GitLab found on a flow's branch, whoever opened or merged them.
- One store, server and master per git repository, `SAAIO_HOME` to choose another.
- Per-worktree setup and teardown scripts, `SAAIO_SLOT` for ports, branch conventions per
  repository.
- Paused services: a flow that rests for `[services] pause_after_minutes` (5 by default) has its
  services stopped by the repository's `.saaio/pause`, and started again by `.saaio/resume` before
  its worker's next tool call.
- Copying text: select with the mouse to copy, through OSC 52 and `pbcopy`, `wl-copy` or `xclip`.
  The wheel scrolls a pane's history. What a session in view copies itself reaches the clipboard
  too.
- Themes: the terminal's own colors, saaio's dark and light themes, live reload, themes written
  by other tools.
- The command line for agents (TOON) and `saaio status --json` for programs.
- `saaio bug-report`: a file to attach to a bug report.
- Commented example `config.toml` and `theme.toml`, written on the first run.
- Prebuilt binaries for macOS (Apple silicon, Intel) and Linux (x86_64, aarch64, static) on every
  release, with checksums, and `install.sh`, which checks the binary for the machine against them
  and installs it to `~/.local/bin`.
- The store and everything saaio writes in it readable only by you.
