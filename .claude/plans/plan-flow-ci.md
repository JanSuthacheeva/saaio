# CI on GitHub Actions for 0.1.0

Implementation page: https://claude.ai/artifact/1eTjoauRtLEXvf6j9qizTh
(approaches: https://claude.ai/artifact/J9mrRGsxqnDZicPWzEfWK5)

## Decision

1A: `rust-toolchain.toml` pins 1.95.0 for CI and contributors alike. One `.github/workflows/ci.yml`
runs six named jobs on every pull request and push to main. `deny` also runs alone on a daily
schedule. cargo-deny is configured in `deny.toml`.

2A: the server is started with a `Timing` that replaces the `cfg(test)` constants `QUIET`,
`FIRST_TURN_GRACE` and `UNSTARTED`. Each server test picks the timing that makes its outcome
independent of speed.

- Timers that decide behaviour are never raised to hide a race.
- Waits that only bound a failure become polls with a generous limit.

Also decided:

- CI is separate from the release workflow that the release-binaries flow writes.
- Branch protection stays out of this flow; it is set on release day.
- The PR names the required checks: `fmt`, `clippy (linux)`, `clippy (macos)`, `test (linux)`,
  `test (macos)`, `deny`.
- No CHANGELOG entry (none before 0.1.0).

## Flakes found (stress runs before the build)

| Test | Seen | Cause | Fix |
| --- | --- | --- | --- |
| `tests/cli.rs` `stop_ends_a_running_server_and_says_when_none_runs` | 1/10 macOS under load | `server::run` binds before `stop_on_signals`; SIGTERM in that window kills it | signals before bind |
| `tests/cli.rs` `a_store_is_for_its_user_alone` | 2/40 macOS under load | `master.json` written after the socket listens | test waits for `master.json` |
| `config::watch::tests::reads_the_file_again_once_it_changed` | 15/15 Linux 2-CPU | `agnet` rewrite has the same length as `agent`; one mtime tick | write a new length |
| `server::tests::types_queued_lines_after_pr_once_the_turn_before_it_ended` | on demand (stand-in 3.5 s slow) | `UNSTARTED` 3 s in tests lets the next line go mid-turn | `Timing`, this test on 30 s |
| `system::tests::reads_the_first_line_a_tool_prints` | 2/60 Linux 2-CPU | `ETXTBSY`: another test forks while the script is open for writing | scripts written by a child process |
| `server::tests::opens_the_next_worker_with_the_config_saved_while_it_runs` | 1/60 Linux 2-CPU | most likely the same `ETXTBSY` on a fake agent; confirm | shared script helper; `wait_until` |

Reproduce them with:

- macOS: `cargo test` 10 times beside 6 `yes > /dev/null` processes.
- Linux: `docker run --cpus=2 rust:1.95` as a non-root user, running `cargo test --lib -- --test-threads=32`
  in a loop.

## Units

`rust-toolchain.toml` (new):

```toml
[toolchain]
channel = "1.95.0"
components = ["rustfmt", "clippy"]
profile = "minimal"
```

`deny.toml` (new):

```toml
[graph]
targets = ["x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu",
           "x86_64-unknown-linux-musl", "aarch64-unknown-linux-musl",
           "x86_64-apple-darwin", "aarch64-apple-darwin"]
[advisories]
yanked = "deny"
ignore = []
[licenses]
allow = [...]   # only what `cargo deny check licenses` needs for this graph
[bans]
multiple-versions = "warn"
wildcards = "deny"
[sources]
unknown-registry = "deny"
unknown-git = "deny"
```

Check that `atomic-polyfill` (RUSTSEC-2023-0089, via postcard -> heapless 0.7) is out of the graph
for these targets. If it is not, add an `ignore` entry with the reason.

`.github/workflows/ci.yml` (new):

```yaml
name: CI
on:
  pull_request:
  push: { branches: [main] }
  schedule: [{ cron: "0 6 * * *" }]
  workflow_dispatch:
permissions: { contents: read }
concurrency:
  group: ci-${{ github.ref }}
  cancel-in-progress: ${{ github.event_name == 'pull_request' }}
env: { CARGO_TERM_COLOR: always, RUST_BACKTRACE: 1 }
jobs:
  fmt:    name: fmt              # ubuntu-24.04, cargo fmt --check
  clippy: name: clippy (${{ matrix.os }})   # linux -> ubuntu-24.04, macos -> macos-15
  test:   name: test (${{ matrix.os }})
  deny:   name: deny             # ubuntu-24.04
```

- fmt, clippy and test skip on the schedule (`if: github.event_name != 'schedule'`).
- Steps:
  - Checkout: `actions/checkout@<sha> # v5`.
  - Toolchain: `rustup toolchain install` reads the file.
  - Cache: `Swatinem/rust-cache@<sha> # v2` with `save-if: ${{ github.ref == 'refs/heads/main' }}`.
  - clippy: `cargo clippy --all-targets --locked -- -D warnings`.
  - test: `cargo test --locked`, then `git diff --exit-code`.
  - deny: `taiki-e/install-action@<sha>` with `tool: cargo-deny`, then `cargo deny --locked check`.
- Action SHAs come from `gh api repos/<owner>/<repo>/git/ref/tags/<tag>`.

`src/server/timing.rs` (new; `mod timing;` in `server.rs`, a line in CLAUDE.md's server/ list):

```rust
/// How long the server waits before it acts on a session; `default` is what saaio runs with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timing {
    /// No key into a pane for this long before the server types into it.
    pub quiet: Duration,            // 10 s
    /// A Codex worker with no first turn this long shows waiting.
    pub first_turn_grace: Duration, // 30 s
    /// A typed line that began no turn this long lets the next one follow.
    pub unstarted: Duration,        // 30 s
}

impl Default for Timing {
    fn default() -> Timing;
}
```

`src/server.rs` (modified):

```rust
pub struct ServerPaths {
    // socket, master, instructions, config, theme as today
    /// The folder a new master opens in.
    pub cwd: PathBuf,
}

pub fn run(
    paths: &ServerPaths,
    store: Store,
    config: Config,
    env: LaunchEnv,
    system: &System,
    timing: Timing,
    clock: fn() -> u64,
) -> Result<()>;

struct Server { /* ... */ timing: Timing }
```

- `mpsc::channel()` and `stop_on_signals(events.clone())` move before `bind`.
- `cwd` moves into `ServerPaths`, which keeps `run` at 7 parameters (clippy `too_many_arguments`).

`src/server/news.rs`, `typing.rs`, `dismiss.rs`, `startup.rs` (modified): drop the constants;
each pure check takes its duration, and `Server` passes `self.timing.*`.

```rust
pub(super) fn quiet(busy: Busy, typed_at: Option<Instant>, now: Instant, wait: Duration) -> bool;
fn ready(busy: Busy, opened_at: Option<Instant>, typed_at: Option<Instant>, now: Instant, quiet: Duration) -> bool;
fn unstarted(busy: Busy, line_at: Option<Instant>, now: Instant, wait: Duration) -> bool;
fn due(typed_at: Option<Instant>, opened_at: Option<Instant>, now: Instant, quiet: Duration) -> bool;
// startup.rs: the stalled-start check takes first_turn_grace the same way
```

`src/cli/commands/server.rs` (modified): sets `ServerPaths.cwd` and calls
`server::run(&paths, store, config, launch_env, &system, Timing::default(), clock)`.

`src/fixtures.rs` (new; `#[cfg(test)] mod fixtures;` in `lib.rs`):

```rust
/// Writes an executable script through a child process, so this process never holds it open for
/// writing while another test thread forks: exec of such a file fails with ETXTBSY on Linux.
pub fn executable(path: &Path, body: &str);
```

All `fs::write` + `0o755` sites in the lib tests use it: `system.rs`, `workspace.rs`, and
`server/tests.rs` (`fake_codex`, `fake_claude`, `fake_claude_keys`, the stalled Codex).
`tests/cli.rs` gets the same function in place of `set_executable`, for the hooks, gh, ssh and
monitor stubs.

`src/server/tests.rs` (modified):

```rust
/// quiet 200 ms, first_turn_grace 300 ms, unstarted 30 s: quick to type and to call a start
/// stalled, never quick to give up on a typed line.
fn quick() -> Timing;
fn start_timed(dir: &Path, agents: BTreeMap<String, Agent>, timing: Timing)
    -> (ServerPaths, thread::JoinHandle<Result<()>>);
/// Polls `done` until it holds; fails with `what` after 30 s, which only a failing test waits for.
fn wait_until(what: &str, done: impl FnMut() -> bool);
```

- `start_with_agents` calls `start_timed(dir, agents, quick())`.
- `a_typed_line_that_begins_no_turn_lets_the_next_one_follow` runs with `unstarted: 300 ms`.
- `a_codex_worker_that_never_starts_a_turn_waits_once` polls until one Waiting appears, then
  sleeps `WATCH_EVERY * 2` and asserts still exactly one.
- The hand-written deadline loops and `wait_until_typed` go through `wait_until`.

`src/config/watch.rs` test: the broken file is `[worker]\nagents = "codex"\n` (a new length); it
expects ``unknown field `agents` ``.

`tests/cli.rs`:

- `a_store_is_for_its_user_alone` waits for `master.json` before it reads the file's mode.
- `wait_until` limits that only bound a failure (`wait_for_socket` among them) go to 30 s.

Docs:

- `CONTRIBUTING.md` gets a "CI" section: the six checks, the pinned toolchain, `cargo deny check`
  locally, how to bump Rust (toolchain file and `rust-version` in one PR), and the stress
  reproduction above.
- `CLAUDE.md` build commands: `cargo clippy --all-targets -- -D warnings` and `cargo deny check`;
  `fixtures.rs` and `server/timing.rs` in Architecture.
- PR template: the check boxes CI now runs become one line pointing at CI.

## Call chains

1. `commands::server::run -> server::run(&paths, store, config, env, &system, Timing::default(), clock)`
2. `server::run -> private::create_dir(store root)`, then `mpsc::channel()`, then
   `stop_on_signals(events.clone())`. From here on, SIGTERM, SIGINT and SIGHUP become `Event::Signal`.
3. `server::run -> conn::bind(&paths.socket): Result<UnixListener>`. A second server fails here
   with "a saaio server already runs".
4. `client::stop_server -> kill(pid, SIGTERM)` at any point after 2: the Signal waits in the channel.
5. `server::run -> MasterRecord::load_or_create(&paths.master, &paths.cwd)`, `open_master`,
   `serve(&inbox)`. `serve` reads the Signal, runs `stop`, removes the socket and logs "stopped".
6. `Server::type_lines -> unstarted(busy, line_at, now, self.timing.unstarted)`. If true, it calls
   `let_go`. Then `ready(busy, opened_at, typed_at, now, self.timing.quiet) -> type_into`.

## Test seams

- `Timing::default()`: a unit test pins 10 s, 30 s and 30 s.
- The pure checks: unit tests with explicit durations.
- The startup order: a new `tests/cli.rs` test starts `saaio server`, sends SIGTERM as soon as
  the socket file exists (before `wait_for_socket`), and expects exit 0 and no socket file left.
  Run it 40 times under load.
- `fixtures::executable`: run the probe tests 60 times on 2-CPU Linux with `--test-threads=32`.
- The workflow: all six checks green on the PR on GitHub's runners.

## Assumptions

- Runner images are pinned (`ubuntu-24.04`, `macos-15`).
- Actions are pinned by SHA, with the version in a comment.
- Caches are saved from main only.
- The daily `deny` runs at 06:00 UTC and emails the owner on failure.
- The licence allow list holds exactly what the graph needs.
- `multiple-versions` only warns.
- `git diff --exit-code` runs after the tests.
- No README badge.

## Tasks

### Server startup and timing

- [x] `server::run` installs `stop_on_signals` before `bind`; new `tests/cli.rs` test sends SIGTERM as soon as the socket file appears and expects a clean exit with no socket left
- [x] Add `server::Timing` (`timing.rs`) with `Default` and its unit test; `ServerPaths.cwd`; `run(.., system, timing, clock)`; `Server.timing`; `commands::server` passes `Timing::default()`
- [x] Replace `QUIET`, `FIRST_TURN_GRACE`, `UNSTARTED` with `self.timing.*` in news, typing, dismiss and startup; their pure checks take a duration; unit tests use `Timing::default()` values
- [ ] `server/tests.rs`: `quick()`, `start_timed`, `wait_until` (30 s); the let-go test on `unstarted: 300 ms`; the stalled-start test polls; every hand-written deadline loop and `wait_until_typed` through `wait_until`

### Flaky tests

- [ ] `fixtures::executable` in `src/fixtures.rs`, used by every script the lib tests write (system, workspace, server tests); the same in `tests/cli.rs` in place of `set_executable`; confirm `opens_the_next_worker_with_the_config_saved_while_it_runs` was ETXTBSY and fix it at its cause if not
- [ ] `config::watch` test writes a new length; `a_store_is_for_its_user_alone` waits for `master.json`; `tests/cli.rs` failure-only limits to 30 s
- [ ] Stress the fixed tree: 10 macOS runs beside a CPU hog, 60 Linux runs on 2 CPUs with `--test-threads=32`, 40 runs of the stop tests; fix anything else found at its cause

### CI

- [ ] `rust-toolchain.toml` (1.95.0, rustfmt, clippy)
- [ ] `deny.toml` with the licence list the graph needs; `cargo deny check` passes locally; the atomic-polyfill advisory is out of the target graph or ignored with its reason
- [ ] `.github/workflows/ci.yml` with the six named jobs, the daily deny-only schedule, rust-cache saved from main, actions pinned by SHA; checked with `actionlint` if available

### Docs

- [ ] `CONTRIBUTING.md` CI section; `CLAUDE.md` build commands and Architecture lines; PR template check list

### End to end

- [ ] Push the branch and open the PR as a draft (on the user's OK) so the workflow runs on GitHub's runners; all six checks green on ubuntu-24.04 and macos-15; rerun once to see the cache hit
