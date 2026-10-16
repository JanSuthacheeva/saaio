# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

flow is an AXI (https://axi.md): a CLI whose main user is an agent. It records each feature flow as an append-only event log and folds the logs into where every flow stands. Agents read TOON (the default output); programs read `flow status --json`. The designs and task lists live in `.claude/plans/` (`plan-flow-core.md`, `plan-flow-start.md`, `plan-flow-ui.md`, `plan-flow-adapters.md`, `plan-flow-per-project.md`, `plan-flow-theme.md`, `plan-flow-branch-names.md`, `plan-flow-talk.md`, `plan-flow-file-structure.md`); the whole flows system (the other five pieces, every decision, verified Claude Code facts, gotchas) is in `docs/HANDOVER.md`, with the design review pages in `docs/design/`.

## Build Commands

```bash
cargo build                    # debug build
cargo run -- <args>            # run, e.g. cargo run -- status
cargo test                     # unit and integration tests
FLOW_UPDATE_GOLDEN=1 cargo test  # rewrite src/ui/golden after a change to the look that is meant
cargo clippy -- -D warnings    # lint
cargo fmt                      # format
cargo install --path .         # install the flow binary
```

## Structure

- One crate: a library (`lib.rs`) with a thin binary. `main.rs` parses, reads the environment variables and the clock, prints, and maps exit codes; nothing else.
- One responsibility per file, nameable without "and". Past about 400 production lines a file splits along a real seam; a uniform file (a table of documents, one match of key bindings) may run longer.
- Unit tests stay inline in `mod tests`. Past about 400 lines of tests they move to a sibling `tests.rs` (`#[cfg(test)] mod tests;`), split further into `tests/*.rs` by subject when large; fixtures several test modules share go in a `#[cfg(test)] mod fixtures;`. CLI behaviour is tested end to end in `tests/cli.rs`.
- Module files are `foo.rs` beside `foo/`, never `foo/mod.rs` (`clippy::mod_module_files`). An `impl` may span the files of its module's children: they see its private fields.
- A command is `cli/commands/<name>.rs`: its clap `Args` and one `run`. The TOON it prints lives in `cli/output/`, one file per document family.
- Modules are nouns for what they hold, never `utils` or `helpers`; functions do not repeat their module's name (`output::status::list`). Paths are `use crate::...`. Every module is private in `lib.rs` except `cli`, so `pub` inside them stays crate-only and dead code still warns. Re-export (`pub use`) only where it gives an item its natural path (`cli::Env`, `ui::app::Effect`).

## Architecture

- **main.rs** - the process boundary: `Cli::try_parse`, the environment variables into `cli::Env`, the clock, stdout/stderr and exit codes (0 ok, 1 error, 2 usage; `flow wait` exits 2 with news).
- **lib.rs** - the module tree.
- **cli/** - the command line: `cli.rs` (`Cli`, `Command`, `run`: dispatch; Hook and Wait run without a store), `env.rs` (`Env`: the environment as values, and the store, registry, config and homes it points to), `commands.rs` + `commands/` (one file per command; `retire.rs` is what remove and clear share: closing the pane, the teardown hook, the worktree and branch), `output.rs` + `output/` (`Document`, `Failure`, and the TOON documents agents read by family: `status`, `flow` (new, report, send), `start`, `remove`, `news`, each with `help[]` next steps). Commands read `current_dir`, `current_exe` and stdin themselves when they need them.
- **event.rs** - what can be reported (`Event`, `EventKind`, `Phase`, `Mode`) and the JSON line format of the log.
- **store.rs** - every file operation: `flows/<id>.jsonl` logs and `flows/<id>.alive` heartbeats under `$FLOW_HOME`, `$XDG_STATE_HOME/flow` or `~/.local/state/flow`; `waits/` for `flow wait` (each session's waiter pid, deliveries, the master's seen marks).
- **registry.rs** - the shared folder (`$XDG_STATE_HOME/flow` or `~/.local/state/flow`) every store registers in (`stores/<hash>`), for `flow status --all`; a store's project name.
- **slots.rs** - `FLOW_SLOT` across every project: `flow start` claims `slots/<n>` under a flock before setup; a claim counts while its flow is unfinished; the slots a project's flows hold.
- **wake.rs** - what is news for a session (pure): the master's undelivered messages for a worker, the flows that stopped since the master last saw them.
- **deliver.rs** - one look for news on the store: a worker's undelivered messages (marked delivered), the master's stopped flows (collected for a moment, marked seen). `flow wait` (`cli/commands/wait.rs`) polls it as Claude Code's `asyncRewake` hook and prints it with `output::news`.
- **fold.rs** - turns a flow's events into its `FlowView`, views into the sorted `Board`, a name or id into one flow; its tests in `fold/tests.rs`.
- **view.rs** - the derived types; serialised as the `--json` contract (`VIEW_VERSION`).
- **text.rs** - how counts, durations and clock times are written, in documents and the UI.
- **launch/** - what `flow start` creates and what a pane runs (pure): `name.rs` (a flow's name, the ticket and slug of a request), `worktree.rs` (worktree path, git command lines, the hooks' environment), `pane.rs` (agent and resume argv, `PaneSpec`, pane environment).
- **branching.rs** - a repo's branch convention (`.flow/config.toml` `[branch]`: template and types) and the branch name `flow start` gives a flow.
- **workspace.rs** - the processes `flow start` and `flow clear` run: git worktrees, the repo's `.flow/setup` and `.flow/teardown` hooks; the branch a folder is on.
- **adapter/** - how flow observes a harness through its own hooks: `adapter.rs` (`Adapter`: the wiring a launch adds, `translate` of one hook event, the skill-to-phase lookup), `claude_code.rs` (Claude Code's `--settings` hooks and its events -> flow events, pure; fixtures are recorded hook payloads). `flow hook <adapter>` (`cli/commands/hook.rs`) runs it, silently, appending only what changes the flow (`fold::changes`).
- **proto.rs** - the messages between `flow ui` and `flow server`: length-prefixed postcard over a Unix socket.
- **client.rs** - connecting to the server, starting it (`ensure_server`), closing a flow's pane for remove and clear.
- **server/** - `flow server`, one thread owning every session: `server.rs` (the event loop: `run`, `serve`, `handle`, `stop`), `conn.rs` (UI connections: hello, messages, frames, board and panes sent), `panes.rs` (which panes are open: refresh, reconcile, open, resume, close), `host.rs` (PTYs and processes), `terminal.rs` (the only file that knows alacritty_terminal), `master.rs` (master.json, master.md), `watch.rs` (when to refold), `reconcile.rs` (which flows need a pane, pure), `tests.rs` (over a real socket).
- **ui/** - `flow ui`, a ratatui client: `ui.rs` (threads and the loop), `app.rs` + `app/` (the Elm-style core, no I/O: `app.rs` state and queries, `messages.rs` server and job answers, `keys.rs` input, `board_keys.rs` the board's keys, `draw.rs` the view, `chrome.rs` the title row and the bar around the pane, `effect.rs` what App asks of the loop; tests in `app/tests/` by subject), `board.rs` + `board/` (`cards.rs`, `detail.rs`), `sidebar.rs` + `sidebar/` (`rows.rs`, `labels.rs`), `dialog.rs` (the boxes over a pane), `fit.rs` (text in terminal columns), `input.rs` (the C-space prefix, mouse), `layout.rs`, `screen.rs` (the focused pane), `rail.rs`, `newflow.rs`, `peek.rs` (Claude transcripts), `outer.rs` (the real terminal), `theme.rs` (Palette: the built-in terminal, flow and flow-light colors -> styles), `theme_file.rs` (theme.toml and what it extends, ThemeWatch).
- **config.rs** - the agents a worker can run: Claude Code built in, more in `~/.config/flow/config.toml`.

**Data flow:** `flow report` -> `cli::run` -> `commands::report` -> `Event` -> appended line -> `fold` -> `FlowView` / `Board` -> TOON (`cli/output/`) or JSON (`view.rs`).

**flow ui:** `flow start` appends a Workspace event -> `flow server` sees the log change (`StoreWatch`), folds, `reconcile` -> opens the worker in a PTY and marks it `flow-ui` -> sends the Board and the focused pane's changed rows to `flow ui`, which sends keys back. Plan and design: `.claude/plans/plan-flow-ui.md`, `docs/design/ui-implementation.html`.

The log is private to this crate; readers depend only on the command line and the `--json` output.
