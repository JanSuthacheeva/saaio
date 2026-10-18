# Plan: bug reports

Approaches page: https://claude.ai/artifact/UPwpg1qb3mNri9FyywA7so

## Decision

A user uploads one file: what `saaio bug-report` writes. It is only useful when server.log holds
every error and says which saaio, OS and tools ran (approach A, approved 2026-10-18):

- Every process appends to the store's server.log itself with the existing `server_log::append`
  (O_APPEND, one short line per write): the server as before, `saaio hook` and the UI. No socket
  message, so an error is logged also when no server runs.
- `saaio hook` writes nothing to stdout or stderr except the Wake news it already prints. Each
  error goes to server.log instead. Before a store is found there is nowhere to log, so it stays
  silent. A broken config.toml is not logged by the hook (it would repeat on every tool call), the
  server already logs it when it changes.
- The UI logs its ending error, a panic (after the terminal restore), failed start and remove jobs
  (not the overview poll, which repeats) and a theme.toml that does not load. config.toml errors
  are the server's to log.
- New `system.rs` probes the OS (`uname -srm`) and `claude`, `codex`, `gh`, `glab --version` in
  parallel, each given up after 1 s. The server's start line and the report use it.
- `proto::BUILD` becomes `0.1.0 (e035b96)`, and clap prints it for `saaio --version`:
  `saaio 0.1.0 (e035b96)`.
- `saaio bug-report` runs without a store (like hook and wait), so a store that cannot be opened is
  in the report instead of stopping it. It writes `saaio-report-<YYYY-MM-DD>.txt` in the current
  folder (overwriting one of the same day), prints its path as TOON with a help line to check the
  file before uploading.
- `.github/ISSUE_TEMPLATE/bug_report.yml` and `docs/troubleshooting.md` "Reporting a bug".
  No CHANGELOG entry (none before 0.1.0).

## Units

`src/proto.rs` (modified)

```rust
/// This binary: `0.1.0 (e035b96)`; `unknown` for the commit outside a git checkout.
pub const BUILD: &str = concat!(env!("CARGO_PKG_VERSION"), " (", env!("SAAIO_COMMIT"), ")");
```

`src/cli.rs` (modified)

```rust
#[command(version = crate::proto::BUILD, ...)]
pub struct Cli { ... }

enum Command {
    ...
    /// Write a file for a bug report: versions, config, the store and the end of server.log
    BugReport,
}
// run: Some(Command::BugReport) => return bug_report::run(env, clock), beside Hook and Wait.
```

`src/system.rs` (new) - the system saaio runs on: the OS and the versions of the tools around it.

```rust
/// The tools whose versions a report names.
pub const TOOLS: [&str; 4] = ["claude", "codex", "gh", "glab"];
/// How long a tool gets to print its version.
const DEADLINE: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct System {
    /// `uname -srm`: `Darwin 25.6.0 arm64`; None when it did not answer.
    pub os: Option<String>,
    pub tools: Vec<(&'static str, Probe)>,
}

/// What `<tool> --version` gave.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Probe {
    /// From the first word that starts with a digit: `2.73.0 (2025-05-19)` of
    /// `gh version 2.73.0 (2025-05-19)`; the whole first line when no word does.
    Version(String),
    /// Not on PATH.
    Missing,
    /// It ran but failed, printed nothing or took longer than DEADLINE: why.
    Failed(String),
}

impl System {
    /// Asks the OS and every tool of TOOLS at once, waiting at most DEADLINE.
    pub fn probe() -> System;
    /// `Darwin 25.6.0 arm64, claude 2.1.302 (Claude Code), codex 0.163.0, gh 2.73.0 (2025-05-19), glab missing`.
    pub fn summary(&self) -> String;
}

/// Runs `program args`, killed at `deadline`; its Probe.
fn run(program: &str, args: &[&str], deadline: Instant) -> Probe;
/// The version part of a `--version` line.
fn version_of(line: &str) -> String;
```

`src/server.rs` (modified)

```rust
pub fn run(paths: &ServerPaths, store: Store, config: Config, env: LaunchEnv, cwd: &Path,
           system: &System, clock: fn() -> u64) -> Result<()>;
/// `started: saaio 0.1.0 (e035b96), protocol 6, Darwin 25.6.0 arm64, claude ..., glab missing`.
fn started(system: &System) -> String;
```

`src/cli/commands/server.rs` (modified): `System::probe()` before `server::run`, passed in. Server
tests pass `&System::default()`.

`src/client.rs` (modified)

```rust
/// server.log of `store`'s server, beside its socket.
pub fn log_path(store: &Store) -> PathBuf;
```

`src/server_log.rs` (modified)

```rust
/// From now on a panic appends `<source>: panic ...` and its backtrace to `log`, then runs the hook
/// before it; for a process whose stderr is not server.log (the UI).
pub fn append_on_panic(log: PathBuf, source: &'static str, clock: fn() -> u64);
```

`src/cli/commands/hook.rs` (modified)

```rust
/// Where `saaio hook` says what went wrong: the store's server.log, never stdout or stderr (a
/// hook's output reaches the harness, and the model).
struct Log {
    path: PathBuf,
    /// `hook claude-code flow 3f2a9c` or `hook codex master`.
    source: String,
    clock: fn() -> u64,
}

impl Log {
    fn new(store: &Store, adapter: &str, hooked: &Hooked, clock: fn() -> u64) -> Log;
    /// Appends `<source>: <text>`; a log that cannot be written is let go.
    fn error(&self, text: &str);
}
// Every eprintln! becomes log.error(..); hook, report, hand_over, record_master_session,
// record_master_turn and mark_busy take `log: &Log`. Before the store is known: no output at all.
```

`src/cli/commands/ui.rs` (modified): `client::log_path(store)` once; `server_log::append_on_panic(log, "ui", clock)`
before `ui::run` (Outer's restore hook, installed later, runs first and calls it); an `Err` from
`ensure_server` or `ui::run` is appended as `ui: failed: <error:#>` before it is returned.

`src/ui.rs` (modified)

```rust
pub struct UiEnv { ..., /// server.log, where the UI says what failed.
    pub log: PathBuf }
// Event::Ran(job, Err(error)) for a job other than Job::Overview: append
//   `ui: saaio <job.args() joined> failed: <error.message>`
// reload_theme Err: append `ui: theme: <error:#>` (ThemeWatch reports a change once).
fn log(env_log: &Path, clock: fn() -> u64, text: &str);  // server_log::append, error let go
```

`src/text.rs` (modified)

```rust
/// `2026-10-18` of a Unix ms time in a zone `offset_secs` east of UTC.
pub fn date(ms: u64, offset_secs: i64) -> String;
```

`src/bug_report.rs` (new, pure) - the text of a bug report file.

```rust
/// Lines of each log the report keeps, from its end.
pub const TAIL_LINES: usize = 300;

#[derive(Debug)]
pub struct Facts {
    pub at: u64,
    pub offset_secs: i64,
    pub system: System,
    /// The store's root, or why it could not be opened.
    pub store: Result<PathBuf, String>,
    pub config: FileCheck,
    pub theme: FileCheck,
    /// server.log.1 then server.log, oldest first; empty without a store.
    pub logs: Vec<LogTail>,
}

/// One settings file: where it is and whether it loads.
#[derive(Debug)]
pub struct FileCheck { pub path: Option<PathBuf>, pub loaded: Loaded }

#[derive(Debug, PartialEq, Eq)]
pub enum Loaded { Loads, Missing, Broken(String) }

/// The end of one log, or why it could not be read (None: it does not exist).
#[derive(Debug)]
pub struct LogTail { pub path: PathBuf, pub text: Option<Result<String, String>> }

/// `saaio-report-2026-10-18.txt`.
pub fn file_name(at: u64, offset_secs: i64) -> String;
/// The whole report, `home` replaced by `~` everywhere in it.
pub fn text(facts: &Facts, home: Option<&Path>) -> String;
/// The last `lines` lines of `text`.
pub fn tail(text: &str, lines: usize) -> &str;
/// `text` with every `home` path written `~`.
fn tilde(text: &str, home: &Path) -> String;
```

Report layout:

```
saaio bug report, 2026-10-18 14:02:11 +07:00

saaio      0.1.0 (e035b96), protocol 5
os         Darwin 25.6.0 arm64
claude     2.1.302 (Claude Code)
codex      0.163.0
gh         2.73.0 (2025-05-19)
glab       missing

store        ~/.local/state/saaio/projects/saaio
config.toml  ~/.config/saaio/config.toml: loads
theme.toml   ~/.config/saaio/theme.toml: ~/.config/saaio/theme.toml:3: unknown color `bg`

== ~/.local/state/saaio/projects/saaio/server.log.1 (last 300 lines) ==
...
== ~/.local/state/saaio/projects/saaio/server.log (last 300 lines) ==
...
```

`src/cli/commands/bug_report.rs` (new)

```rust
/// `saaio bug-report`: gathers the Facts, writes bug_report::text to file_name in the current
/// folder; its path and the check before uploading. Runs without a store.
pub fn run(env: &Env, clock: fn() -> u64) -> Result<Document, Failure>;
```

`src/cli/output/bug_report.rs` (new)

```rust
/// report: <path>, help: check the file before uploading it.
pub(crate) fn written(path: &Path) -> Document;
```

```
report: /Users/jan/projects/shop/saaio-report-2026-10-18.txt
help[1]: "Check the file before uploading it: it can hold flow names, requests and paths"
```

## Call chains

`saaio bug-report`

1. `main` -> `cli::run(cli, env, clock)`: `Command::BugReport` before the store is opened.
2. `cli::run` -> `bug_report::run(env, clock): Result<Document, Failure>` (command).
3. -> `System::probe(): System`.
4. -> `env.store(cwd)`: `Ok(Some(store))` -> `Ok(root)`; `Ok(None)` -> `Err("cannot find a state directory")`; `Err(e)` -> `Err(format!("{e:#}"))`.
5. -> `Config::read(env.config_file())`: file absent -> `Missing`, `Ok` -> `Loads`, `Err` -> `Broken`.
6. -> `theme_file::load(config_dir).0` the same way, `theme.toml` absent -> `Missing`.
7. With a store: `client::log_path(&store)` and `<log>.1` read -> `LogTail { text: tail(.., TAIL_LINES) }`; NotFound -> None, other error -> `Some(Err(..))`.
8. -> `bug_report::text(&facts, env.user_home()): String`.
9. -> `fs::write(cwd.join(file_name(now, offset)), text)`; error -> `Failure::error("could not write <path>: ..")`, exit 1.
10. -> `output::bug_report::written(&path): Document::Toon`.

Hook error

1. Harness -> `saaio hook claude-code` -> `hook::run`.
2. `env.store` fails or finds none -> `Document::Silent`, nothing written anywhere.
3. `Log::new(&store, adapter, &hooked, clock)`; each failure (unknown adapter, stdin, JSON, no flow,
   foreign merge request, append, set_busy, ...) -> `log.error(text)` -> `server_log::append(path, now, "<source>: text")`; its own failure is ignored.
4. stdout empty, stderr empty unless the Stop hands over news (`Document::Wake`, exit 2).

UI

1. `commands::ui::run` -> `server_log::append_on_panic(log, "ui", clock)` -> `ensure_server` / `ui::run`.
2. `Err(e)` -> `server_log::append(log, now, "ui: failed: {e:#}")` -> `Err` as before.
3. Panic -> Outer's hook restores the terminal -> append hook logs `ui: panic in thread ...` and backtrace -> default hook prints.
4. `Event::Ran(job, Err(error))`, job not Overview -> `ui: saaio start ... failed: <message>`.
5. `reload_theme` `Err` -> `ui: theme: <error>`.

Server start

1. `commands::server::run` -> `System::probe()` (at most 1 s) -> `server::run(.., &system, clock)`.
2. After `bind`: `server_log::write("started: saaio {BUILD}, protocol {PROTOCOL_VERSION}, {system.summary()}")`.

## Test seams

- `system.rs`: `version_of` of the four real lines; `summary`; `run` of a missing program -> Missing,
  of a script that exits 1 -> Failed, of one that sleeps past a short deadline -> Failed (killed).
- `bug_report.rs`: `text` of fixed Facts (layout, Missing/Broken, missing logs, store error), `tilde`
  (home inside paths, a home prefix of another folder name kept), `tail`, `file_name`.
- `server_log`: `append_on_panic` text built by `panic_text` (existing); no hook test.
- `hook.rs`: `Log::error` appends `<source>: text` to the file.
- `tests/cli.rs` (scratch SAAIO_HOME): `--version` is `saaio 0.1.0 (<commit>)`; `hook` with an
  unknown flow and with non-JSON stdin prints nothing on stdout or stderr and appends to server.log;
  `bug-report` in a scratch cwd writes the file with the store under `~`, a broken config.toml's
  error, and the end of a seeded server.log and server.log.1, and prints its path and help.
- Server tests: `started` of a fixed System (the server's stderr is not captured in tests).

## Assumptions

- The tail is the last 300 lines of each log.
- A second report the same day overwrites the first.
- The hook does not log config.toml errors (the server does).
- The UI does not log the overview poll failing, nor config.toml errors.
- Tools are found on PATH by name, not the configured agent commands.
- The report prints TOON like every command (AXI), with the path absolute.
- `BUILD` changes format everywhere it is shown (Peer, `--stop` line) - not a protocol change.

## Tasks

- [x] `proto::BUILD` as `0.1.0 (e035b96)`, clap `version = BUILD`, tests/cli.rs `--version` test updated
- [x] `system.rs`: `System::probe`, `summary`, `Probe`, `run`, `version_of` with unit tests
- [x] Server start line names the System: `server::run(.., system, ..)`, `commands::server` probes, server test for the line
- [x] `client::log_path`, hook `Log` replacing every `eprintln!` in `hook.rs`, unit test for `Log::error` and tests/cli.rs for a silent hook that logs
- [ ] UI logging: `server_log::append_on_panic`, `ui: failed:` in `commands::ui`, `UiEnv::log`, failed jobs and theme errors in `ui.rs`
- [ ] `text::date` and `bug_report.rs` (`Facts`, `text`, `tail`, `tilde`, `file_name`) with unit tests
- [ ] `saaio bug-report`: `Command::BugReport`, `commands/bug_report.rs`, `output/bug_report.rs`, tests/cli.rs end to end
- [ ] `.github/ISSUE_TEMPLATE/bug_report.yml`, `docs/troubleshooting.md` "Reporting a bug" and the new start line, `docs/cli.md` (glance row, section, `--version`), `docs/files.md` server.log row, CLAUDE.md modules and plan list
- [ ] `cargo fmt`, `cargo clippy --all-targets -- -D warnings`, `cargo test`
- [ ] End to end with a scratch SAAIO_HOME: open the UI in a PTY, check the start line, break theme.toml and see `ui: theme:`, run a hook with a bad payload, `saaio bug-report`, read the file
