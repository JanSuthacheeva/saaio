# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

saaio is an AXI (https://axi.md): a CLI whose main user is an agent. It records each feature flow as an append-only event log and folds the logs into where every flow stands. Agents read TOON (the default output); programs read `saaio status --json`. The designs and task lists live in `.claude/plans/` (`plan-flow-core.md`, `plan-flow-start.md`, `plan-flow-ui.md`, `plan-flow-adapters.md`, `plan-flow-per-project.md`, `plan-flow-theme.md`, `plan-flow-branch-names.md`, `plan-flow-talk.md`, `plan-flow-file-structure.md`, `plan-flow-codex.md`, `plan-flow-tui-polish.md`, `plan-flow-master-indicator.md`, `plan-flow-configurable-workflow.md`, `plan-flow-release-docs.md`, `plan-saaio-repo-store.md`, `plan-flow-codex-startup.md`, `plan-flow-bug-report.md`); the whole flows system (the other five pieces, every decision, verified Claude Code facts, gotchas) is in `docs/HANDOVER.md`, with the design review pages in `docs/design/`. The user documentation is `README.md`, `ROADMAP.md` and `docs/*.md` (index `docs/README.md`); a change a user can see updates it in the same PR.

## Build Commands

```bash
cargo build                    # debug build
cargo run -- <args>            # run, e.g. cargo run -- status
cargo test                     # unit and integration tests
SAAIO_UPDATE_GOLDEN=1 cargo test  # rewrite src/ui/golden after a change to the look that is meant
cargo clippy --all-targets -- -D warnings  # lint, tests included
cargo deny check               # advisories, licences, sources (deny.toml)
cargo fmt                      # format
cargo install --path .         # install the saaio binary
```

CI (`.github/workflows/ci.yml`) runs fmt, clippy and test on Linux and macOS, and deny, on the toolchain `rust-toolchain.toml` pins; CONTRIBUTING.md names the checks. A test waits for what happens soon with a limit only a failure reaches (30 s), never a sleep that guesses how long it takes.

E2E runs of the UI, `saaio server` or `saaio --stop` use a scratch store: `SAAIO_HOME=$(mktemp -d)`. On the project's real store a second UI takes over the user's, and `--stop` ends every session in it; inside a worker, `saaio --stop` refuses its own store. Bare `saaio` opens the UI only in a person's terminal, so an agent runs it in a PTY with its markers unset: `SAAIO_HOME=$(mktemp -d) env -u CLAUDECODE -u CODEX_THREAD_ID -u SAAIO_FLOW_ID -u SAAIO_MASTER saaio`.

## Structure

- One crate: a library (`lib.rs`) with a thin binary. `main.rs` parses, reads the environment variables and the clock, prints, and maps exit codes; nothing else. `build.rs` only sets `SAAIO_COMMIT`, the commit `proto::BUILD` names.
- Releases: a `v*` tag runs `.github/workflows/release.yml` (native builds for macOS and Linux, tests, `SHA256SUMS`, the GitHub release); `install.sh` installs a release's binary (`tests/install.rs`). Steps: CONTRIBUTING.md, Releasing.
- One responsibility per file, nameable without "and". Past about 400 production lines a file splits along a real seam; a uniform file (a table of documents, one match of key bindings) may run longer.
- Unit tests stay inline in `mod tests`. Past about 400 lines of tests they move to a sibling `tests.rs` (`#[cfg(test)] mod tests;`), split further into `tests/*.rs` by subject when large; fixtures several test modules share go in a `#[cfg(test)] mod fixtures;`. CLI behaviour is tested end to end in `tests/cli.rs`.
- Module files are `foo.rs` beside `foo/`, never `foo/mod.rs` (`clippy::mod_module_files`). An `impl` may span the files of its module's children: they see its private fields.
- A command is `cli/commands/<name>.rs`: its clap `Args` and one `run`. The TOON it prints lives in `cli/output/`, one file per document family.
- Modules are nouns for what they hold, never `utils` or `helpers`; functions do not repeat their module's name (`output::status::list`). Paths are `use crate::...`. Every module is private in `lib.rs` except `cli`, so `pub` inside them stays crate-only and dead code still warns. Re-export (`pub use`) only where it gives an item its natural path (`cli::Env`, `ui::app::Effect`).

## Architecture

- **main.rs** - the process boundary: `Cli::try_parse`, the environment variables into `cli::Env`, the clock, stdout/stderr and exit codes (0 ok, 1 error, 2 usage; `saaio wait` exits 2 with news).
- **lib.rs** - the module tree.
- **fixtures.rs** - what tests across the crate share: `executable`, a script written through a child process, since exec of a file a forked test thread still holds open fails with ETXTBSY on Linux.
- **cli/** - the command line: `cli.rs` (`Cli`, `Command`, `run`: dispatch; Hook, Wait and BugReport run without a store), `env.rs` (`Env`: the environment as values, and the store, registry, config and homes it points to), `commands.rs` + `commands/` (one file per command; bare `saaio` runs `home.rs`, `ui.rs` or `stop.rs`; `retire.rs` is what remove and clear share: closing the pane, the teardown hook, the worktree and branch; `bug_report.rs` gathers what `saaio bug-report` writes), `output.rs` + `output/` (`Document`, `Failure`, and the TOON documents agents read by family: `status`, `flow` (new, report, send), `start`, `remove`, `news`, `bug_report`, each with `help[]` next steps). Commands read `current_dir`, `current_exe` and stdin themselves when they need them.
- **event.rs** - what can be reported (`Event`, `EventKind`, `Phase`, `Mode`) and the JSON line format of the log.
- **store.rs** - every file operation: `flows/<id>.jsonl` logs and `flows/<id>.alive` heartbeats under the store `Env::store` picks (`$SAAIO_HOME`, else the git repository's project store, else the shared folder); `waits/` for `saaio wait` (each session's waiter pid, deliveries, the master's seen marks) and the master's turn (`master.turn`).
- **private.rs** - files and folders for the user alone: folders 0700, files 0600, never written through a link (`write_new`), replaced whole (`write_atomic`).
- **registry.rs** - the shared folder (`$XDG_STATE_HOME/saaio` or `~/.local/state/saaio`) every store registers in (`stores/<hash>`), for `saaio status --all`; a repository's store (`projects/<name>`, its `repo` file naming the owner, `<name>-2` for a second repository of that name); a store's project name.
- **slots.rs** - `SAAIO_SLOT` across every project: `saaio start` claims `slots/<n>` under a flock before setup; a claim counts while its flow is unfinished; the slots a project's flows hold.
- **wake.rs** - what is news for a session (pure): the master's undelivered messages for a worker, the flows that stopped since the master last saw them.
- **deliver.rs** - one look for news on the store: a worker's undelivered messages (marked delivered), the master's stopped flows (collected for a moment, marked seen; `master_news_now` without the wait). `saaio wait` (`cli/commands/wait.rs`) polls it as Claude Code's `asyncRewake` hook; for Codex, which cannot wake itself, `saaio hook` hands it over at Stop and `server/news.rs` types it into an idle pane.
- **fold.rs** - turns a flow's events into its `FlowView`, views into the sorted `Board`, a name or id into one flow; its tests in `fold/tests.rs`.
- **view.rs** - the derived types; serialised as the `--json` contract (`VIEW_VERSION`).
- **text.rs** - how counts, durations, clock times and log stamps are written, in documents, the UI and server.log; the local zone's offset.
- **launch/** - what `saaio start` creates and what a pane runs (pure): `name.rs` (a flow's name, the ticket and slug of a request), `worktree.rs` (worktree path, git command lines, the hooks' environment), `pane.rs` (agent and resume argv, `PaneSpec`, pane environment).
- **workflow.rs** - the phases a flow walks through (`[[phases]]`: 1 to 4 names, the skills that start each; the built-in plan, implement, review, merge-request) and which phase a skill starts. `saaio start`/`saaio new` record the names in the flow's `started` event; `fold` reads them from there, never from config, and folds a log without them with the built-in four.
- **branching.rs** - a repo's branch convention (`.saaio/config.toml` `[branch]`: template and types) and the branch name `saaio start` gives a flow.
- **workspace.rs** - the processes `saaio start`, `saaio clear` and the server run: git worktrees, the repo's `.saaio/setup`, `.saaio/teardown`, `.saaio/pause` and `.saaio/resume` hooks; the branch a folder is on; the git common dir a folder's repository shares.
- **pause.rs** - when a flow's services pause and resume, and whether its tool calls wait (pure).
- **adapter/** - how saaio observes a harness through its own hooks: `adapter.rs` (`Adapter`: the wiring a launch adds, the instructions, whether a session wakes itself, `translate` of one hook event with the workflow, `WORKER_PROMPT`), `claude_code.rs` (Claude Code's `--settings` hooks and its events -> flow events, pure), `codex.rs` (Codex's `-c hooks.*` arguments and its events -> flow events, pure), `answer.rs` (how a turn's closing message reads), `calls.rs` (how a tool call reads); fixtures are recorded hook payloads. `saaio hook <adapter>` (`cli/commands/hook.rs`; the master's pane sets `SAAIO_MASTER=1`) runs it, silently (its errors go to server.log), appending only what changes the flow (`fold::changes`), or for the master keeping where its turn stands (`fold::master_state`); for Codex it also marks the session busy or idle and hands news over at Stop (exit 2).
- **proto.rs** - the messages between the UI and `saaio server`: length-prefixed postcard over a Unix socket; `Peer`, who a connection is (pid, folder, `BUILD`).
- **server_log.rs** - server.log beside the socket: rotation past 1 MiB to `server.log.1`, lines stamped with time and pid (the server writes them to stderr, which is the log; `saaio --stop`, `saaio hook` and the UI append their own), the panic hook that logs a panic and ends the server, and the one that logs a UI panic.
- **system.rs** - the system saaio runs on: the OS (`uname -srm`) and the versions of claude, codex, gh and glab, probed at once with a 1 s deadline, for the server's start line and the bug report.
- **bug_report.rs** - the text of the file `saaio bug-report` writes (pure): versions, store, whether config.toml and theme.toml load, the end of server.log and server.log.1, the home folder as `~`.
- **client.rs** - connecting to the server, starting it (`ensure_server`), closing a flow's pane for remove and clear.
- **server/** - `saaio server`, one thread owning every session: `server.rs` (the event loop: `run`, `serve`, `handle`, `stop`), `conn.rs` (UI connections: hello, messages, frames, board, panes and the master's place and turn sent), `panes.rs` (which panes are open: refresh, reconcile, open, resume, close), `host.rs` (PTYs and processes), `terminal.rs` (the only file that knows alacritty_terminal), `master.rs` (master.json, master.md), `watch.rs` (when to refold), `reconcile.rs` (which flows need a pane, pure), `news.rs` (news typed into an idle Codex pane), `startup.rs` (a Codex worker with no first turn), `pause_watch.rs` (the repo's pause and resume scripts for resting flows, off the server thread), `timing.rs` (`Timing`: how long the server waits before it types into a pane, calls a start stalled or lets a typed line go; tests pick their own), `tests.rs` (over a real socket).
- **ui/** - the terminal UI bare `saaio` opens, a ratatui client: `ui.rs` (threads and the loop), `app.rs` + `app/` (the Elm-style core, no I/O: `app.rs` state and queries, `messages.rs` server and job answers, `keys.rs` input, `board_keys.rs` the board's keys, `pointer.rs` mouse selection, clicks and the wheel, `draw.rs` the view, `chrome.rs` the title row above the pane, `effect.rs` what App asks of the loop; tests in `app/tests/` by subject), `board.rs` + `board/` (`cards.rs`, `detail.rs`), `sidebar.rs` + `sidebar/` (`rows.rs`, `labels.rs`), `dialog.rs` (`Dialog`, the one frame every box over a session is drawn in), `fit.rs` (text in terminal columns), `focus.rs` (the one focus look: the selection fill and the accent bar), `hints.rs` (key hints fitted to a width, inactive ones faint), `input.rs` (keys split at the prefix, mouse events to the pane or saaio), `selection.rs` (text selected with the mouse: read and highlighted), `clipboard.rs` (copies through OSC 52 and pbcopy, wl-copy or xclip), `prefix.rs` (the prefix key: `[ui] prefix`, its name and bytes), `layout.rs` (the pane beside the sidebar or rail, `RAIL_BELOW`), `screen.rs` (the focused pane), `rail.rs`, `newflow.rs` + `newflow/` (the new-flow dialog's form and keys; `view.rs` draws it), `peek.rs` + `peek/` (a worker's transcript: `claude.rs`, `codex.rs`), `outer.rs` (the real terminal), `theme.rs` (Palette: the built-in terminal, saaio and saaio-light colors -> styles), `theme_file.rs` (theme.toml and what it extends, ThemeWatch).
- **default_file.rs** - `DefaultFile`: a file saaio writes with its default text when it is missing or still an earlier default, never once edited; master.md, and the examples beside config.toml.
- **examples/** - `config.toml` and `theme.toml`, every setting commented out at its default (`#key = value`; `# text` explains). `saaio server` writes them on its first run (`config::EXAMPLE`, `theme_file::EXAMPLE`); tests uncomment them and compare them with the built-in defaults, so a new setting fails the tests until its example line exists.
- **config.rs** - the agents a worker can run: Claude Code and Codex built in, more in `~/.config/saaio/config.toml`; `[worker]` (agent, prompt: every agent's first prompt unless it sets its own), `[[phases]]` (the workflow), `[master]` (wake, agent), `[ui]` (prefix), `[services]` (pause_after_minutes); errors read `file:line: message`. `config/watch.rs` (`ConfigWatch`): the file again when it changed, for the server, the UI and the master's `saaio wait`, which keep the last good config when it breaks.

**Data flow:** `saaio report` -> `cli::run` -> `commands::report` -> `Event` -> appended line -> `fold` -> `FlowView` / `Board` -> TOON (`cli/output/`) or JSON (`view.rs`).

**The UI:** `saaio start` appends a Workspace event -> `saaio server` sees the log change (`StoreWatch`), folds, `reconcile` -> opens the worker in a PTY and marks it `saaio-ui` -> sends the Board and the focused pane's changed rows to the UI, which sends keys back. Plan and design: `.claude/plans/plan-flow-ui.md`, `docs/design/ui-implementation.html`.

The log is private to this crate; readers depend only on the command line and the `--json` output.

## Changelog

`CHANGELOG.md` holds only the 0.1.0 entry until 0.1.0 is released; nothing done before the release gets an entry. Entries start after it: every PR adds its line under `## [Unreleased]` (Added, Changed, Fixed, Removed), written for users, not for reviewers, and a release renames Unreleased to the version and its date. Known gaps and planned work go in `ROADMAP.md` (Implementing, Planned soon, Planned future), never in the CHANGELOG; a PR that builds or drops a roadmap item updates it.
