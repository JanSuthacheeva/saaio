# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

flow is an AXI (https://axi.md): a CLI whose main user is an agent. It records each feature flow as an append-only event log and folds the logs into where every flow stands. Agents read TOON (the default output); programs read `flow status --json`. The designs and task lists live in `.claude/plans/` (`plan-flow-core.md`, `plan-flow-start.md`, `plan-flow-ui.md`); the whole flows system (the other five pieces, every decision, verified Claude Code facts, gotchas) is in `docs/HANDOVER.md`, with the design review pages in `docs/design/`.

## Build Commands

```bash
cargo build                    # debug build
cargo run -- <args>            # run, e.g. cargo run -- status
cargo test                     # unit and integration tests
cargo clippy -- -D warnings    # lint
cargo fmt                      # format
cargo install --path .         # install the flow binary
```

## Architecture

- **main.rs** - clap `Cli`, dispatch, exit codes (0 ok, 1 error, 2 usage). The only place that reads the environment and the clock and writes stdout.
- **event.rs** - what can be reported (`Event`, `EventKind`, `Phase`, `Mode`) and the JSON line format of the log.
- **store.rs** - every file operation: `flows/<id>.jsonl` logs and `flows/<id>.alive` heartbeats under `$FLOW_HOME`, `$XDG_STATE_HOME/flow` or `~/.local/state/flow`.
- **fold.rs** - turns a flow's events into its `FlowView`, views into the sorted `Board`, a name or id into one flow.
- **view.rs** - the derived types; serialised as the `--json` contract (`VIEW_VERSION`).
- **launch.rs** - what `flow start` creates and what a pane runs: name, slot, worktree path, branch, git command lines, agent and resume argv, pane environment (pure).
- **workspace.rs** - the processes `flow start` and `flow clear` run: git worktrees, the repo's `.flow/setup` and `.flow/teardown` hooks; the branch a folder is on.
- **proto.rs** - the messages between `flow ui` and `flow server`: length-prefixed postcard over a Unix socket.
- **client.rs** - connecting to the server, starting it (`ensure_server`), closing a flow's pane for remove and clear.
- **server/** - `flow server`, one thread owning every session: `host.rs` (PTYs and processes), `terminal.rs` (the only file that knows alacritty_terminal), `master.rs` (master.json, master.md), `watch.rs` (when to refold), `reconcile.rs` (which flows need a pane, pure), `mod.rs` (the event loop).
- **ui/** - `flow ui`, a ratatui client: `app.rs` (state, input, drawing; no I/O), `input.rs` (the C-space prefix, mouse), `layout.rs`, `screen.rs` (the focused pane), `sidebar.rs`, `rail.rs`, `board.rs`, `newflow.rs`, `peek.rs` (Claude transcripts), `outer.rs` (the real terminal), `theme.rs`, `mod.rs` (threads and the loop).
- **config.rs** - the agents a worker can run: Claude Code built in, more in `~/.config/flow/config.toml`.
- **output.rs** - the TOON documents agents read: home, status, report answers, errors, each with `help[]` next steps.

**Data flow:** `flow report` -> `Event` -> appended line -> `fold` -> `FlowView` / `Board` -> TOON (`output.rs`) or JSON (`view.rs`).

**flow ui:** `flow start` appends a Workspace event -> `flow server` sees the log change (`StoreWatch`), folds, `reconcile` -> opens the worker in a PTY and marks it `flow-ui` -> sends the Board and the focused pane's changed rows to `flow ui`, which sends keys back. Plan and design: `.claude/plans/plan-flow-ui.md`, `docs/design/ui-implementation.html`.

The log is private to this crate; readers depend only on the command line and the `--json` output.
