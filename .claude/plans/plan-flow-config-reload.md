# Plan: config.toml reloads in the running saaio server and saaio ui

Approaches page: https://claude.ai/artifact/9wKRK3zHSvUxiE5VFueA9c (approach A). No separate
implementation page: the user approved the plan through the master and asked to go with the
recommendations on every open question.

## Decision

Each process watches config.toml itself with a `ConfigWatch` (size and mtime stamps, like
`ThemeWatch`). `saaio server` looks at it at the top of every refresh, before it reconciles, and
swaps `Server.config`; a broken file keeps the last good config and goes to server.log. `saaio ui`
looks at it on the theme tick and swaps the prefix (Router, App hints) and the new-flow dialog's
agents; a broken file keeps the last good config and shows `config: <file>:<line>: <message>` as
a notice until it reads again. No protocol change.

What a running session started with stays: the server keeps each open pane's adapter, so an
edited `[agents]` entry applies to the next session opened or resumed with it. The master's
Claude Code waiter is always wired and reads `[master] wake` itself, so wake applies at once.

| Setting | Applies |
| --- | --- |
| `[worker] agent` | at once: the next `saaio start`, the new-flow dialog's preselection |
| `[worker] prompt`, an agent's `prompt` | the next worker that starts |
| `[[phases]]` | the next `saaio start` (unchanged) |
| `[agents]` | the next session opened or resumed with that agent |
| `[master] wake` | at once |
| `[master] agent` | the next time the master starts |
| `[ui] prefix` | at once |

Config errors everywhere (CLI too) read `<file>:<line>: <message>`. A broken file when `saaio ui`
or `saaio server` starts still refuses: there is no last good config yet.

Rejected: recording the first prompt in the started event. The server reloads before it
reconciles, so the prompt it fills in is the one saved before the start.

## Units

### `src/store.rs` (modified)

```rust
impl Stamp {
    /// The file's size and time; None when it is missing or unreadable.
    pub fn of(path: &Path) -> Option<Stamp>;
}
```

`ui/theme_file.rs`' private `stamp_of` goes; `ThemeWatch` uses `Stamp::of`.

### `src/text.rs` (modified)

```rust
/// "path:line", where a file's error is.
pub fn place(path: &Path, line: usize) -> String;
/// The 1-based line `offset` is on.
pub fn line_of(text: &str, offset: usize) -> usize;
```

Moved from `ui/theme_file.rs` (`at`, `line`), which then uses them.

### `src/config.rs` (modified)

```rust
mod watch;

impl Config {
    /// The file's config with the built-in agents; the built-in alone when `file` is None or missing.
    /// Errors read "<file>:<line>: <message>".
    pub fn read(file: Option<&Path>) -> Result<Config>;
    /// parse() takes the path so its errors name file and line; the two refusals (top-level
    /// `agent`, the [phases] table) name the line of their key.
    fn parse(path: &Path, text: &str) -> Result<Config>;
}
```

### `src/config/watch.rs` (new)

```rust
/// Knows the config file by size and time; a missing file counts too.
#[derive(Debug)]
pub struct ConfigWatch {
    file: Option<PathBuf>,
    stamp: Option<Option<Stamp>>,
}

impl ConfigWatch {
    pub fn new(file: Option<PathBuf>) -> ConfigWatch;
    /// The config again when the file appeared, changed or went away; always on the first call.
    pub fn changed(&mut self) -> Option<Result<Config>>;
}
```

### `src/launch/pane.rs` (modified)

```rust
pub struct PaneSpec {
    pub key: PaneKey,
    pub argv: Vec<String>,
    pub cwd: PathBuf,
    pub env: Vec<(String, String)>,
    /// The harness the session is followed through, as it was when it opened.
    pub adapter: Option<Adapter>,
}
```

`worker_spec` and `MasterRecord::spec` set it from the agent.

### `src/server.rs`, `src/server/panes.rs`, `news.rs`, `typing.rs` (modified)

```rust
struct Server {
    // ...
    /// config.toml, looked at before every refresh.
    configs: ConfigWatch,
    /// The adapter each open pane started with.
    adapters: HashMap<PaneKey, Adapter>,
}

impl Server {
    /// Takes config.toml again when it changed; a broken one keeps the last and is logged.
    fn reload_config(&mut self);
}
```

- `run(paths, ...)` builds `ConfigWatch::new(paths.config.clone())` and calls `changed()` once so
  the config it was handed is not read twice.
- `refresh` calls `reload_config` first.
- `open_pane` records `spec.adapter` in `adapters` (removes the key when None).
- `server_delivered` (news.rs) and `takes_lines` (typing.rs) read the pane's adapter from
  `adapters`, not from the config's agent.

### `src/server/master.rs`, `src/adapter.rs`, `src/adapter/claude_code.rs` (modified)

```rust
// MasterRecord::spec loses `wake`
pub fn spec(&self, start: Start, agent: &Agent, env: &LaunchEnv, instructions: &Instructions, session: Option<&str>) -> Option<PaneSpec>;
// Adapter::wire_master loses `wake`
pub fn wire_master(self, argv: Vec<String>, flow_exe: &Path, instructions: &str, fresh: bool) -> Vec<String>;
/// The master's --settings JSON: report_hooks and `<flow_exe> wait --master` on each WAIT_EVENTS event.
pub fn master_settings(flow_exe: &Path) -> String;
```

### `src/cli/commands/wait.rs` (modified)

`wait` gets a `ConfigWatch` (from `Env::config_file`) for the master: while `[master] wake` is
false it looks for no news (nothing marked seen) and keeps sleeping; a broken file keeps the last
value (true before any good read).

```rust
fn wait(store: &Store, waiter: &Waiter, configs: &mut ConfigWatch, clock: fn() -> u64) -> Option<String>;
```

### `src/cli/env.rs` (modified)

```rust
/// The config file $SAAIO_CONFIG, $XDG_CONFIG_HOME or $HOME point to.
pub(crate) fn config_file(&self) -> Option<PathBuf>;
```

`config()` and `config_dir` use it. `Config::load` goes: `Env::config` was its only caller
(changed in the build).

### `src/ui/input.rs`, `src/ui/app.rs`, `src/ui.rs`, `src/cli/commands/ui.rs` (modified)

```rust
impl Router {
    /// Takes `prefix` from now on; a half-pressed prefix is forgotten.
    pub fn set_prefix(&mut self, prefix: Prefix);
}

impl App {
    /// The agents the new-flow dialog offers, the default first; an open dialog keeps its list.
    pub fn set_agents(&mut self, agents: Vec<String>);
    /// The key before every command, for the hints that name it.
    pub fn set_prefix(&mut self, prefix: Prefix);
}

pub struct UiEnv {
    // agents and prefix go
    /// config.toml, watched while the ui runs.
    pub config_file: Option<PathBuf>,
    // ...
}

/// How a notice about the config begins.
const CONFIG_NOTICE: &str = "config: ";
/// Takes config.toml again when it changed; a broken one keeps the last and says why until it
/// reads again. True when something changed on screen.
fn reload_config(configs: &mut ConfigWatch, app: &mut App, router: &mut Router) -> bool;
```

`App::with_agents` and `with_prefix` become the setters. `cli/commands/ui.rs` still calls
`env.config()?` to refuse a broken file before it starts anything.

## Call chains

1. Server, every refresh: `Server::refresh -> Server::reload_config -> ConfigWatch::changed(): Option<Result<Config>>`
   - `None` -> nothing.
   - `Some(Ok(config))` -> `self.config = config`; `server_log::write("config.toml read again")`.
   - `Some(Err(e))` -> `server_log::write("config: {e:#}")`; `self.config` stays.
   - then the store fold and `reconcile(..., &self.config, ...)`: a new worker gets the new prompt and command.
2. Server, news for a running pane: `Server::server_delivered(key) -> self.adapters.get(key)`; `[master] wake` read from `self.config` (live).
3. UI, every 10 ticks: `ui::run -> reload_config -> ConfigWatch::changed()`
   - `Some(Ok(config))` -> `router.set_prefix(config.ui.prefix)`, `app.set_prefix(...)`, `app.set_agents(config.worker_agents())`, `app.take_back(CONFIG_NOTICE)`; redraw.
   - `Some(Err(e))` -> `app.say_about_file("config: {e:#}")`; prefix and agents stay.
4. UI start: `ui::run` builds `Router::new(Prefix::default())`, then `reload_config` once before the loop.
5. Master waiter: `wait --master` loop -> `configs.changed()` updates `wake` -> `wake == false`: sleep, no `master_news`; true: `deliver::master_news` as today.
6. CLI with a broken file: `Env::config -> Config::read -> parse(path, text)`: `Err("<file>:<line>: <message>")` -> Failure, exit 1.

## Test seams

- `Stamp::of`, `text::place`, `text::line_of`: unit.
- `Config::read`/`parse`: unit, errors with file and line for a parse error, an unknown key, both refusals.
- `ConfigWatch`: unit over a tempdir: first call, unchanged, edited, broken then fixed, removed.
- `Router::set_prefix`, `App::set_agents` (open dialog keeps its list), `App::set_prefix` (hints): unit.
- `claude_code::master_settings`, `Adapter::wire_master`, `MasterRecord::spec`: existing tests adjusted to always arm the waiter.
- Server: `server/tests.rs` over a real socket: a prompt saved while the server runs reaches the next worker's argv; a broken file keeps the last config and the server keeps serving.
- Server adapters: a running pane keeps its adapter after its agent's entry changes (unit on `server_delivered` through the server tests).
- `wait --master`: `tests/cli.rs` that wake = false yields no news and true does.
- CLI error format: `tests/cli.rs` updated to `<file>:<line>:`.

## Assumptions

- The ui looks at config.toml once a second (the theme tick); the server before every refresh.
- An open new-flow dialog keeps the agent list it opened with.
- A prefix change forgets a half-pressed old prefix.
- With wake off, the master's stopped flows are not marked seen, so turning it on brings them.
- A removed config file reads as the built-in config, as at start.
- The server logs every reread of the file, good or broken.
- No CHANGELOG entry: 0.1.0 is not released; the docs and ROADMAP change.

## Tasks

- [x] Add `Stamp::of` in store.rs and `text::place`/`text::line_of`; ThemeWatch and theme errors use them; tests for each
- [x] Add `Config::read(file)` and `parse(path, text)` with `<file>:<line>: <message>` errors; `Env::config_file`; unit tests and tests/cli.rs error assertions updated
- [x] Add `config::watch::ConfigWatch` with unit tests (first call, unchanged, edited, broken then fixed, removed)
- [x] Add `PaneSpec.adapter` and `Server.adapters`; news.rs and typing.rs read the pane's adapter; tests
- [x] `Server::reload_config` at the top of refresh, logging errors; server tests: a saved prompt reaches the next worker, a broken file keeps the last config
- [ ] Always wire the master's waiter (`master_settings`, `wire_master`, `MasterRecord::spec` lose `wake`); `wait --master` reads `[master] wake` through a ConfigWatch; tests
- [ ] `Router::set_prefix`, `App::set_agents`/`set_prefix`, `UiEnv.config_file`, `ui::reload_config` with the `config: ` notice; unit tests
- [ ] Docs: docs/configuration.md (When a change applies, Use your own workflow, Errors), examples/config.toml header, ROADMAP.md drops "First prompt read at saaio start"; any other doc naming `saaio ui --stop` for config
- [ ] End to end with scratch SAAIO_HOME and SAAIO_CONFIG: saaio ui running, change the prefix, the worker prompt, break and fix the file, start a flow after a prompt change
