# Changelog

All notable changes to saaio are listed here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and saaio uses
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Fixed

- `saaio hook` skips an event over 16 MiB and notes it in server.log.
- A UI that stops reading, or a session that prints without pause, no longer slows the server
  for other UIs and commands.
- server.log stays bounded while the server runs: past 1 MiB it moves to server.log.1 whoever
  writes to it.
- The server copes with very large windows and malformed messages.

## [0.1.1] - 2026-10-22

Updating from 0.1.0, which has no `saaio update`: run the install line once
(`curl -fsSL https://github.com/JanSuthacheeva/saaio/releases/latest/download/install.sh | sh`),
run `saaio trust` once in each repository with `.saaio` scripts, and run `saaio --stop` so the
server starts again on the new version.

### Added

- The UI names a newer release in its title row (`0.1.1 is out: saaio update`), looking once a
  day. `[ui] update_check = false` turns it off.
- `saaio update` installs the latest release, or `--version <tag>`, over the binary you run.

### Changed

- When the store's path is too long for a socket, the socket and its log move from
  `/tmp/saaio-<uid>/` to `$XDG_RUNTIME_DIR` or `$TMPDIR` (`/tmp` when neither fits).
  `saaio --stop` still stops a server 0.1.0 started there.
- saaio runs a repository's `.saaio` scripts, and lets Codex workers trust its worktrees, only
  once you trusted the repository. `saaio start` refuses until then and names the scripts; the
  new-flow dialog asks you. `saaio trust` trusts the repository you are in (from your own
  terminal only), `saaio trust --revoke` takes it back, `saaio trust --list` shows every one.
- `saaio remove` says what the teardown did (`teardown: ran`, `none`, `failed: ...`).

### Fixed

- A `.saaio` script that never ends no longer blocks `saaio start`, `saaio remove` or
  `saaio clear`: setup is stopped after 10 minutes and teardown after 2, and the flow says so.
- `saaio hook`, `saaio wait` and `saaio send` act only for the session the server opened: each
  session gets its own key in `SAAIO_PROOF`. Setting `SAAIO_FLOW_ID` or `SAAIO_MASTER` no longer
  reports into another flow, takes the master's news or sends as the master.
- saaio uses the socket's folder only when it belongs to you and is closed to others, and the UI
  and the server talk only to processes of your own user.
- The server takes requests only after a connection's hello, closes one that says none within
  5 seconds, lets `saaio remove` and `saaio clear` do nothing but close a pane, and takes at most
  16 connections at once.
- A request that contains `{request}` or another placeholder is passed to the worker as written.
  Before, it was filled in again, and a request repeating one could grow the worker's command line
  past the system's limit, so the worker never opened.
- A request or flow name that starts with `-` no longer fails with a usage error when the UI starts
  or cleans up a flow.
- `saaio report merge-request` takes only a merge request of the flow's repository, by any of its
  remotes, and saaio passes a merge request to `gh` and `glab` after `--`.
- `saaio bug-report` also writes the home folder as `~` in `file://` URLs, as the real path a linked
  home leads to, and in Claude Code's project names (`-Users-you-...`).
- The server keeps running when the system is out of open files or threads, instead of spinning
  or stopping.
- A flow that ended, or went on to a subagent, no longer shows the question it asked earlier as
  its blocker in `saaio status`, its `--json` and the master's news.
- A `.saaio` script stopped for running too long now takes what it started with it (a
  `docker compose up` it began, a dev server): it runs in a process group of its own, and the
  whole group is stopped.
- `saaio start` refuses a repository whose `.saaio` or `.saaio/worktrees` folder is a symbolic
  link, so a worktree is never created outside the repository.

## [0.1.0] - 2026-10-21

The first public release. Planned next: [ROADMAP.md](ROADMAP.md).

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
- Shell completions for bash, zsh, fish, elvish and PowerShell: `saaio completions <shell>`, and
  in `complete/` in every release tarball.
- Commented example `config.toml` and `theme.toml`, written on the first run.
- Prebuilt binaries for macOS (Apple silicon, Intel) and Linux (x86_64, aarch64, static) on every
  release, with checksums, and `install.sh`, which checks the binary for the machine against them
  and installs it to `~/.local/bin`.
- The store and everything saaio writes in it readable only by you.
