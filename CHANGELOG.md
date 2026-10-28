# Changelog

All notable changes to saaio are listed here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and saaio uses
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0] - not released yet

Updating from 0.1.x: run `saaio update`, then `saaio --stop`. 0.2.0 refuses a 0.1.x server that
still runs; once it is stopped, the next `saaio` starts the server on the new version. An agent
you defined in config.toml with its own `prompt` now sends it for every kind of flow: leave the
`prompt` out to get each kind's.

### Added

- `saaio start --base <branch>` and a base row in the new-flow dialog start a flow's worktree from
  another branch, such as the branch of the flow before it, so each merge request of a stack runs
  as a flow of its own. The worker is told its merge request targets that branch; `saaio status`
  and `--json` (`workspace.base`) show it. When the base merges, the flows on it move onto the
  branch it merged into and their workers are told to rebase, also when the base is removed or
  cleared after its merge. `saaio remove` of a base names the flows that stand on it.
- Desktop notifications: when a flow or the master starts waiting or is blocked while you aren't
  looking at it, in another session or another window, the UI has your terminal raise a
  notification (OSC 9 in Ghostty, iTerm2, kitty and WezTerm, OSC 777 in foot, urxvt and VTE
  terminals, the bell in Alacritty and Terminal.app), also through tmux with
  `allow-passthrough all`. `[ui] notify = false` in config.toml turns them off.
- Kinds of flows: `feature`, `fix` and `research`, each with its own phases, first prompt and
  branch type. `saaio start --kind fix`, `saaio new --kind` and a kind row in the new-flow dialog
  pick one; the master picks it from the request. A fix runs `/ship-fix` (`$ship-fix` in Codex),
  starting with a `diagnose` phase, on a `bugfix`-type branch where the repository's convention has
  one. A research flow answers a question in its pane from a worktree without a branch, is `done`
  once it has answered, and `saaio clear` (or `x` on the board) forgets it. `saaio status`,
  `--json` (`kind`, `merges`), the board and the sidebar (`fix claude`) show a flow's kind.
  `[kinds.<name>]` in config.toml changes a kind or adds one; `[worker] kind` sets the default.
- The UI warns when the running server is another build than the `saaio` you ran, or when another
  `saaio` (such as an old `cargo install` in `~/.cargo/bin`) comes first on PATH: a dialog names
  each binary with its build and `saaio --stop`, then the title row keeps a short line.
  `saaio update` lists the same: `shadowed` and every running server on another version
  (`servers[]`), each with what to run.

### Changed

- The built-in `claude` and `codex` agents no longer set a `prompt`: a worker's first prompt is its
  flow's kind's, written for its harness. An agent's own `prompt` still wins, now for every kind,
  so an agent you defined with `prompt = "$ship-feature {request}"` sends that for fixes too:
  leave it out to get each kind's. A top-level `[[phases]]` and `[worker] prompt` keep working as
  the feature kind's.
- Protocol 9: the server says which build it is and which binary it started from, so the UI can
  warn about a mismatch. The refusal from an earlier server now names its pid, this binary and
  `saaio --stop`. `saaio send` in a pane an earlier server opened says the pane has no
  key and to run `saaio --stop`, instead of `only the master sends`.

### Fixed

- A server that started while another saaio process moved server.log aside now writes to the
  new server.log, instead of to server.log.1 for as long as it runs.

## [0.1.2] - 2026-10-24

Updating from 0.1.1: run `saaio update`, then `saaio --stop` so the server starts again on the
new version. A `.saaio/teardown`, `.saaio/pause` or `.saaio/resume` script that runs git on the
main checkout (`git -C "$SAAIO_REPO" ...`) now acts on the worktree instead: run it as
`env -u GIT_DIR -u GIT_WORK_TREE git -C "$SAAIO_REPO" ...`.

### Added

- `saaio status --json` gives each flow `permission`: true while it waits on a permission prompt
  in its pane.

### Changed

- `saaio send` to a worker waiting on a permission prompt names the prompt and says the worker
  reads the message once the prompt is answered in its pane. Before, it said the worker would be
  woken with it now.
- `saaio report merge-request` keeps a merge request URL as `.../pull/<n>` or
  `.../-/merge_requests/<n>`, dropping a trailing `/`, `?query`, `#fragment` or a page such as
  `/files`, and names that form when it refuses a URL.

### Fixed

- `saaio bug-report` writes its file into `reports/` of saaio's state folder
  (`~/.local/state/saaio/reports`), never into the current folder, so it no longer lands in a
  repository.
- The peek reads a Codex worker's transcript only from Codex's sessions folder
  (`~/.codex/sessions`, or under `$CODEX_HOME`).
- saaio's state folder is closed to other users (`0700`) also when an earlier version created it.
- A repository no longer takes over the store of a removed repository with the same folder name:
  it gets a store of its own (`<name>-2`). A repository moved to another folder starts with a new
  store; [files.md](docs/files.md#the-store) says how to read its earlier flows.
- Git in `.saaio/teardown`, `.saaio/pause` and `.saaio/resume` always acts on the flow's own
  worktree: they run with `GIT_DIR` and `GIT_WORK_TREE` set. A script that wants git on the main
  checkout clears them ([repository.md](docs/repository.md#setup-and-teardown)).
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
