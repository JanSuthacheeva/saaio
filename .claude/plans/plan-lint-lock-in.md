# Lint lock-in (rust-quality-audit item 7)

Implementation page: https://claude.ai/artifact/SxDauoHNxxgG6UnjV3ZjzC
Approaches page: https://claude.ai/artifact/EwFgwN43byobYM7fPfW925

## Decision

Approach A. `clippy::unwrap_used`, `clippy::print_stdout` and `clippy::print_stderr` become warnings
(CI's `-D warnings` makes them errors). `expect_used` is not enabled: `expect("why it holds")` stays
the form of an invariant (35 production sites today, all stated invariants). The one print outside
the process boundary, `src/cli/commands/wait.rs` (the `set_busy` error), moves to server.log: on
stderr it became part of the news `main.rs` prints to stderr with exit 2, so the agent was woken
with saaio's own error in its message.

## Units

### Cargo.toml `[lints.clippy]`, beside `mod_module_files`

```toml
# The process boundary prints (main.rs); production code never unwraps, an invariant is
# expect("why it holds"). Tests are exempt at their crate roots.
unwrap_used = "warn"
print_stdout = "warn"
print_stderr = "warn"
```

### src/main.rs

```rust
#[expect(clippy::print_stderr, reason = "the process boundary prints the wake news")]
fn answer(result: Result<Document, Failure>) -> ExitCode; // body unchanged

#[expect(clippy::print_stderr, reason = "the process boundary reports a failed write")]
fn print(text: &str, code: ExitCode) -> ExitCode; // body unchanged
```

### src/bin/site/main.rs

```rust
#[expect(
    clippy::print_stdout,
    clippy::print_stderr,
    reason = "the site builder's process boundary prints"
)]
fn main() -> ExitCode; // body unchanged
```

On the functions, not `#![expect]` at the crate root, so a print in the site's other modules fires.

Its tests unwrap, and the binary is a crate root of its own, so it gets the test exemption too:

```rust
#![cfg_attr(test, allow(clippy::unwrap_used, reason = "tests unwrap directly"))]
```

### src/cli/commands/wait.rs

```rust
pub fn run(args: &Args, ctx: &Context) -> Document;
// passes ctx.env.server_log(&store) to wait; the proof-failure line keeps its text,
// built with who(&waiter) as its prefix

fn wait(
    store: &Store,
    waiter: &Waiter,
    configs: &mut ConfigWatch,
    clock: fn() -> u64,
    log: &Path,
) -> Option<String>;
// on a set_busy error: let _ = server_log::append(log, clock(), &format!("{}: {error:#}", who(waiter)));

/// How server.log names the waiter: "wait master" or "wait flow <id>".
fn who(waiter: &Waiter) -> String;
```

### src/lib.rs crate-root test exemption

```rust
#![cfg_attr(
    test,
    allow(
        clippy::disallowed_methods,
        clippy::unwrap_used,
        clippy::print_stderr,
        reason = "tests set up their files, read PATH, start threads, unwrap and print skips directly"
    )
)]
```

`print_stderr` is for `src/workspace/git.rs` test's `eprintln!("skipped: no ssh")`.

### tests/cli/main.rs, tests/install.rs crate roots

```rust
#![allow(
    clippy::disallowed_methods,
    clippy::unwrap_used,
    reason = "tests set up their files, read PATH and unwrap directly"
)]
```

### tests/cli/support.rs (moved from tests/cli/identity.rs:23)

```rust
/// home/server.log, where the CLI's own errors go; empty when there is none.
pub fn server_log(home: &Path) -> String;
```

### tests/cli/wait.rs

```rust
#[test]
fn a_busy_mark_that_cannot_be_written_stays_out_of_the_wake_message();
```

## Data

None. The server.log line: `wait flow <id>: could not write <path>: <cause>`, stamped by
`server_log::append`.

## Call chain: a worker is woken and its busy mark cannot be written

1. `main -> cli::run -> wait::run(args, ctx): Document`
2. `wait::run -> Env::server_log(&store): PathBuf`
3. `wait::run -> wait(store, waiter, configs, clock, &log): Option<String>`
4. `wait -> deliver::worker_news(store, id, now): WorkerNews::News(notes)`
5. `wait -> Store::set_busy(waiter, Busy::Turn): Err`
6. `wait -> server_log::append(log, clock(), "wait flow <id>: ..."): io::Result<()>`; a failure to
   log is let go, as for the proof line
7. `wait -> Some(news)`; `wait::run -> Document::Wake(news)`
8. `main::answer` prints only the news to stderr, exit 2

A lint that fires: a new `unwrap()` or print in production code fails clippy with `-D warnings`.
A deliberate exception is `#[expect(clippy::..., reason = "...")]` at its site.

## Test seams

- `wait.rs`: end to end in `tests/cli/wait.rs`. A non-empty folder at `waits/<id>.busy` makes the
  atomic rename fail. Assert exit 2, stderr exactly the news, server.log has `wait flow <id>:`.
  It fails on the current code first.
- The lints: clippy clean with `--all-targets --all-features -- -D warnings`; a throwaway `unwrap()`
  and `eprintln!` in a lib file fire, then are removed.

## Assumptions

- `build.rs` keeps its `println!("cargo:...")`: clippy does not lint prints in build scripts.
- Test crate roots use `allow`, not `expect`, as they already do.
- No CONTRIBUTING change: it already names the clippy command.
- CLAUDE.md's Lints line gains one sentence on unwrap, expect and printing.
- CHANGELOG `## [Unreleased]` gets a Fixed line: a woken worker's message no longer carries
  saaio's own error when its busy mark cannot be written.
- Rebased on main after PR #123 (d156e5e) before the build.

## Tasks

- [x] Move `server_log(home)` from tests/cli/identity.rs to tests/cli/support.rs and add `a_busy_mark_that_cannot_be_written_stays_out_of_the_wake_message` to tests/cli/wait.rs; see it fail
- [x] wait.rs: `wait(.., log: &Path)` and `who(waiter)`, the `set_busy` error to server.log; the test passes
- [x] Cargo.toml: `unwrap_used`, `print_stdout`, `print_stderr`; the `#[expect]`s in src/main.rs and src/bin/site/main.rs; the crate-root test allows in src/lib.rs, src/bin/site/main.rs, tests/cli/main.rs, tests/install.rs; clippy clean, each lint seen firing once
- [x] CLAUDE.md Lints line and CLAUDE.md plan list, CHANGELOG Fixed line
- [ ] End to end: in a scratch `SAAIO_HOME`, a worker's `saaio wait` with a message sent and `waits/<id>.busy` a folder wakes with exactly the message and logs the error; fmt, clippy and the full test suite pass
