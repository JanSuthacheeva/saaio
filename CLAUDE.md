# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

flow is an AXI (https://axi.md): a CLI whose main user is an agent. It records each feature flow as an append-only event log and folds the logs into where every flow stands. Agents read TOON (the default output); programs read `flow status --json`. The design and task list live in `.claude/plans/plan-flow-core.md`; the whole flows system (the other five pieces, every decision, verified Claude Code facts, gotchas) is in `docs/HANDOVER.md`, with the design review pages in `docs/design/`.

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
- **output.rs** - the TOON documents agents read: home, status, report answers, errors, each with `help[]` next steps.

**Data flow:** `flow report` -> `Event` -> appended line -> `fold` -> `FlowView` / `Board` -> TOON (`output.rs`) or JSON (`view.rs`).

The log is private to this crate; readers depend only on the command line and the `--json` output.
