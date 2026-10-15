# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

flow is an AXI (https://axi.md): a CLI whose main user is an agent. It records each feature flow as an append-only event log and folds the logs into where every flow stands. Agents read TOON (the default output); programs read `flow status --json`. The designs and task lists live in `.claude/plans/` (`plan-flow-core.md`, `plan-flow-start.md`, `plan-flow-ui.md`, `plan-flow-adapters.md`, `plan-flow-per-project.md`, `plan-flow-theme.md`, `plan-flow-branch-names.md`, `plan-flow-talk.md`); the whole flows system (the other five pieces, every decision, verified Claude Code facts, gotchas) is in `docs/HANDOVER.md`, with the design review pages in `docs/design/`.

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

## Architecture

- **main.rs** - clap `Cli`, dispatch, exit codes (0 ok, 1 error, 2 usage). The only place that reads the environment and the clock and writes stdout.
- **event.rs** - what can be reported (`Event`, `EventKind`, `Phase`, `Mode`) and the JSON line format of the log.
- **store.rs** - every file operation: `flows/<id>.jsonl` logs and `flows/<id>.alive` heartbeats under `$FLOW_HOME`, `$XDG_STATE_HOME/flow` or `~/.local/state/flow`; `waits/` for `flow wait` (each session's waiter pid, deliveries, the master's seen marks).
- **registry.rs** - the shared folder (`$XDG_STATE_HOME/flow` or `~/.local/state/flow`) every store registers in (`stores/<hash>`), for `flow status --all`; a store's project name.
- **slots.rs** - `FLOW_SLOT` across every project: `flow start` claims `slots/<n>` under a flock before setup; a claim counts while its flow is unfinished.
- **wake.rs** - what is news for a session (pure): the master's undelivered messages for a worker, the flows that stopped since the master last saw them. `flow wait` (main.rs) polls it as Claude Code's `asyncRewake` hook and wakes the session with exit 2.
- **fold.rs** - turns a flow's events into its `FlowView`, views into the sorted `Board`, a name or id into one flow.
- **view.rs** - the derived types; serialised as the `--json` contract (`VIEW_VERSION`).
- **launch.rs** - what `flow start` creates and what a pane runs: name, slot, worktree path, branch, git command lines, agent and resume argv, pane environment (pure).
- **branching.rs** - a repo's branch convention (`.flow/config.toml` `[branch]`: template and types) and the branch name `flow start` gives a flow.
- **workspace.rs** - the processes `flow start` and `flow clear` run: git worktrees, the repo's `.flow/setup` and `.flow/teardown` hooks; the branch a folder is on.
- **adapter/** - how flow observes a harness through its own hooks: `mod.rs` (`Adapter`: the wiring a launch adds, `translate` of one hook event, the skill-to-phase lookup), `claude_code.rs` (Claude Code's `--settings` hooks and its events -> flow events, pure; fixtures are recorded hook payloads). `flow hook <adapter>` (main.rs) runs it, silently, appending only what changes the flow (`fold::changes`).
- **proto.rs** - the messages between `flow ui` and `flow server`: length-prefixed postcard over a Unix socket.
- **client.rs** - connecting to the server, starting it (`ensure_server`), closing a flow's pane for remove and clear.
- **server/** - `flow server`, one thread owning every session: `host.rs` (PTYs and processes), `terminal.rs` (the only file that knows alacritty_terminal), `master.rs` (master.json, master.md), `watch.rs` (when to refold), `reconcile.rs` (which flows need a pane, pure), `mod.rs` (the event loop).
- **ui/** - `flow ui`, a ratatui client: `app.rs` (state, input, drawing; no I/O), `input.rs` (the C-space prefix, mouse), `layout.rs`, `screen.rs` (the focused pane), `sidebar.rs`, `rail.rs`, `board.rs`, `newflow.rs`, `peek.rs` (Claude transcripts), `outer.rs` (the real terminal), `theme.rs` (Palette: the built-in terminal, flow and flow-light colors -> styles), `theme_file.rs` (theme.toml and what it extends, ThemeWatch), `mod.rs` (threads and the loop).
- **config.rs** - the agents a worker can run: Claude Code built in, more in `~/.config/flow/config.toml`.
- **output.rs** - the TOON documents agents read: home, status, report answers, errors, each with `help[]` next steps.

**Data flow:** `flow report` -> `Event` -> appended line -> `fold` -> `FlowView` / `Board` -> TOON (`output.rs`) or JSON (`view.rs`).

**flow ui:** `flow start` appends a Workspace event -> `flow server` sees the log change (`StoreWatch`), folds, `reconcile` -> opens the worker in a PTY and marks it `flow-ui` -> sends the Board and the focused pane's changed rows to `flow ui`, which sends keys back. Plan and design: `.claude/plans/plan-flow-ui.md`, `docs/design/ui-implementation.html`.

The log is private to this crate; readers depend only on the command line and the `--json` output.
