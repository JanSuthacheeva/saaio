# Bare `saaio --stop`

The requested CLI shape and behavior are specified in the feature request. Root `--stop` belongs to `Cli` in `src/cli.rs`, and dispatch calls the existing `ui::run` with `ui::Args { stop: true }`. The same UI command path handles both spellings, including the worker store guard, result text, and server shutdown. `main.rs` remains the process boundary.

## Units

```rust
// src/cli.rs
struct Cli {
    stop: bool,
    command: Option<Command>,
}

// src/cli/commands/ui.rs
struct Args {
    pub(crate) stop: bool,
}
```

## Call chain

1. `Cli::try_parse()` accepts root `--stop`, while `ui --stop` remains accepted by `ui::Args`.
2. `cli::run(Cli { stop: true, command: None }, env, clock)` resolves the store and calls `ui::run(&ui::Args { stop: true }, store, env, clock)` regardless of terminal or agent markers.
3. `ui::run` applies its existing own-flow guard, then stops the server or reports that none runs.

## Test seams

`tests/cli.rs` uses temporary `SAAIO_HOME` stores to check root stop with no server, a running server, worker guard, and terminal agent markers. The existing `ui --stop` coverage remains.

## Tasks

- [x] Add CLI tests for bare `--stop` with no server, running server, and terminal or worker contexts.
- [x] Parse and route bare `--stop` in `src/cli.rs` through `ui::run`; update CLI help.
- [x] Update README, CLI, UI, and troubleshooting docs with the plain `saaio --stop` reference style.
- [x] Run the end-to-end stop check, full tests, formatting, and `cargo clippy --all-targets -- -D warnings`.
