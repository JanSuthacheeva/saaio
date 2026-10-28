# Mismatched saaio binaries

Review pages: approaches https://claude.ai/artifact/TDDLKWMsHuSeKrzrWebmG1 (`docs/design/mismatched-binaries-approaches.html`),
implementation https://claude.ai/artifact/EUzhze5ULtwx2e6k3Uift9 (`docs/design/mismatched-binaries-implementation.html`).

## Reproduced (2026-10-28, scratch store, real release binaries)

- 0.1.0 first on PATH, 0.1.1 behind it: bare `saaio` starts a 0.1.0 server; the master pane has
  `SAAIO_MASTER=1` and no `SAAIO_PROOF`; 0.1.1's `saaio send` from it says `only the master sends`.
- A 0.1.1 server runs, a 0.1.2 UI attaches: no word (both protocol 8).
- A 0.1.2 UI against a 0.1.0 server is refused (earlier socket), naming neither binary.

## Decision

- Approach A: the server's `Welcome` carries its build and the binary it started from
  (`LaunchEnv::flow_exe`, recorded at start, so right after an update replaced the file).
  `PROTOCOL_VERSION` 8 -> 9 in its own small commit (the kinds-of-flows flow may bump it too:
  whichever merges second takes the next number).
- The UI and `saaio update` compare that build with their own and with the first `saaio` on PATH,
  and name both binaries and `saaio --stop`.
- A refusal from a 0.1.x server tells the user exactly to run `saaio --stop`.
- The UI warns only: no restart from the dialog (a restart ends every session; the user decides).
- Panes running the server's own saaio first on PATH: not in this PR; a ROADMAP Planned soon row.

## Units

### src/binaries.rs (new)

```rust
//! The saaio binaries that can disagree: this one, the first on PATH, a server's; `Mismatch`

/// Moved here from proto.rs (build): proto now imports binaries for `Welcome`, so binaries
/// cannot import proto.
pub const BUILD: &str = concat!(env!("CARGO_PKG_VERSION"), " (", env!("SAAIO_COMMIT"), ")");

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Binary {
    pub exe: PathBuf,
    /// proto::BUILD's form: `0.1.2 (45630ee)`; `unknown` when --version said nothing usable.
    pub build: String,
}

impl Binary {
    /// This process: current_exe and BUILD.
    pub fn this() -> io::Result<Binary>;
    /// The version part of the build: `0.1.2`.
    pub fn version(&self) -> &str;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mismatch {
    /// Another saaio of another build comes first on PATH.
    Shadowed(Binary),
    /// The running server is another build.
    Server(Binary),
}

/// The first executable file named saaio in `path`'s folders; empty and relative entries skipped.
pub fn first_on_path(path: &OsStr) -> Option<PathBuf>;

/// The first saaio on PATH when it is another file than `this` (canonicalised) and its
/// `--version` (1 s) names another build or none.
pub fn shadowing(this: &Binary, path: &OsStr) -> Option<Binary>;

/// `shadowing` with its own limit for --version (tests: a fresh script's first exec on macOS can
/// take past 1 s). A build without its commit (`0.2.0`, from `saaio update`) matches on the version.
fn shadowing_within(this: &Binary, path: &OsStr, limit: Duration) -> Option<Binary>;

/// Shadowed first, then Server when `server`'s build is not `this`'s.
pub fn mismatches(this: &Binary, server: Option<&Binary>, path: Option<&OsStr>) -> Vec<Mismatch>;
```

### src/system.rs (modified)

```rust
/// `<program> --version` from the first word that starts with a digit; was private and took &str.
pub(crate) fn version(program: impl AsRef<OsStr>, deadline: Instant) -> Probe;
```

### src/proto.rs (modified)

```rust
pub const PROTOCOL_VERSION: u32 = 9;

ServerMsg::Welcome {
    version: u32,
    /// The server's build and the binary it started from.
    server: Binary,
},
```

### src/server/conn.rs (modified)

- `hello` sends `Welcome { version, server: Binary { exe: flow_exe, build: BUILD } }`.
- Another protocol: `Refused` reason names the server's build and exe:
  `saaio server 0.2.0 (abc1234) from ~/.local/bin/saaio speaks protocol 9, this saaio speaks 8`.

### src/client.rs (modified)

```rust
pub struct Client {
    stream: UnixStream,
    server: Binary,
}

impl Client {
    /// The server's build and binary, from its Welcome.
    pub fn server(&self) -> &Binary;
}

/// A server that listens on a socket: its pid, and its binary unless it refused (an earlier protocol).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Running {
    pub pid: u32,
    pub server: Option<Binary>,
}

/// Connects as Control to read who listens; None when no server does.
pub fn running(socket: &Path) -> Result<Option<Running>>;
```

- `connect` on `Refused` fails with: `{reason}. The saaio server (pid 4321) is another saaio than
  this one (saaio 0.2.0 (abc1234), ~/.local/bin/saaio): run `saaio --stop`, then start saaio
  again; unfinished flows resume`. (Build: the paths here are written in full, as the client has
  no home; the pid is read before the hello, since macOS refuses socket options once a refusing
  server hung up. `hello(stream, role) -> Result<Result<Binary, String>>` is what connect and
  running share.)

### src/cli/env.rs, src/main.rs (modified)

```rust
pub struct Env { /* ... */ pub path: Option<OsString> }   // main.rs: env::var_os("PATH")
```

### src/cli/commands/update.rs, src/cli/output/update.rs (modified)

```rust
/// What update finds beside its own install: the saaio first on PATH and the servers on another
/// version than `version`.
pub(crate) struct Others {
    pub shadowed: Option<Binary>,
    pub servers: Vec<(PathBuf, Running)>,   // store root, who listens
}

fn others(env: &Env, version: &str) -> Others;   // commands/update.rs

pub(crate) fn updated(from: &Build, to: &Tag, binary: &str, others: &Others) -> Document;
pub(crate) fn latest(running: &Build, latest: &Tag, others: &Others) -> Document;
```

- `servers` from `Registry::stores()` and `client::running(&env.socket(&store))`; kept when it
  refused (`server: None`) or its version is not `version` (the target after an install, the
  running version on Latest). The general "a running server keeps..." help line is replaced.
- TOON after an install:

```
updated: saaio 0.1.2 -> 0.2.0
binary: ~/.local/bin/saaio
shadowed: ~/.cargo/bin/saaio (saaio 0.1.0 (9b77d8c))
servers[1]{store,pid,build,binary}:
  ~/.local/state/saaio/projects/shop,4321,0.1.2 (45630ee),~/.local/bin/saaio
help[3]:
  `saaio` runs ~/.cargo/bin/saaio, not ~/.local/bin/saaio: remove it (cargo uninstall saaio) or put ~/.local/bin first on PATH
  Run `SAAIO_HOME=<store> saaio --stop` for each server, then start saaio again; unfinished flows resume
  Changes: https://github.com/JanSuthacheeva/saaio/releases/tag/v0.2.0
```

### src/cli/commands/send.rs, src/cli/commands/hook.rs (modified)

- send: `env.saaio_master && env.key().is_none()` -> Failure `this pane has no key: an earlier
  saaio server opened it`, help `Ask the user to run `saaio --stop` and start saaio again;
  unfinished flows resume`; before the `identity::proves` check.
- hook: same case logs `no SAAIO_PROOF: an earlier saaio server opened this pane; saaio --stop`.

### src/cli/commands/ui.rs, src/ui.rs (modified)

- `UiEnv` gains `path: Option<OsString>`.
- After `Client::connect`: `binaries::mismatches(&this, Some(client.server()), env.path.as_deref())`
  -> `App::warn(this, mismatches)`.

### src/ui/app.rs, keys.rs, draw.rs, chrome.rs (modified)

```rust
pub enum View { /* ... */
    /// The binaries disagree: any key closes it.
    Mismatch,
}
struct App { /* ... */
    this: Option<Binary>,
    mismatches: Vec<Mismatch>,
}

/// Opens View::Mismatch when `mismatches` is not empty; the title row keeps a line after.
pub fn warn(&mut self, this: Binary, mismatches: Vec<Mismatch>);
```

- keys.rs: any key in `View::Mismatch` -> `View::Panes`.
- draw.rs `dialog()`: `View::Mismatch` -> `dialog::mismatch(..)`; draw_view draws it like Help.
- chrome.rs right side: notice, else the mismatch line (`server is saaio 0.1.1: saaio --stop` /
  `another saaio first on PATH`, waiting style), else release and hint.

### src/ui/dialog.rs (modified)

```rust
/// "mismatched saaio": each mismatch with both binaries (with ~), what it breaks, and what to run.
pub fn mismatch<'a>(this: &Binary, mismatches: &[Mismatch], home: Option<&str>, theme: &'a Theme) -> Dialog<'a>;
```

## Call chains

### Bare `saaio` attaches

1. `commands::ui::attach -> ui::run(socket, UiEnv { path, .. }, clock)`
2. `ui::run -> Client::connect(socket, Role::Attach{..}): Result<Option<Client>>`
3. `Conn::hello -> ServerMsg::Welcome { version: 9, server: Binary { exe: flow_exe, build: BUILD } }`
4. `ui::run -> Binary::this(): io::Result<Binary>`
5. `ui::run -> binaries::mismatches(&this, Some(client.server()), path): Vec<Mismatch>`
   (`shadowing -> first_on_path`, `system::version(exe, 1 s)` only when PATH's saaio is another file)
6. `ui::run -> App::warn(this, mismatches)`: `View::Mismatch` when not empty
7. Any key: `App::on_key -> View::Panes`; `draw_title` keeps the mismatch line.

Errors: another protocol -> `Refused` -> `Client::connect` Err naming the server's pid
(`peer_pid`), this binary and `saaio --stop`; the UI fails with it. `Binary::this` fails -> no
warnings, the UI runs. A failed or slow `--version` -> build `unknown`, shadowed.

### `saaio update`

1. `update::run -> install(&releases, &target, dir)` (unchanged)
2. `update::run -> others(env, &target.version().to_string()): Others`
3. `others -> binaries::shadowing(&Binary { exe, build: target version }, path)`
4. `others -> env.registry()?.stores() -> client::running(&env.socket(&store))`, kept when
   `server` is None or its version differs
5. `update::run -> output::updated(&running, &target, &bin_path, &others)`

Errors: a socket that errors is left out; no registry -> no servers. `Step::Latest` runs the same
`others` with the running version and prints `latest(.., &others)`.

### `saaio send` in a pane without a key

1. `send::run`: `env.saaio_master && env.key().is_none()` -> Failure (above)
2. else the existing `identity::proves` check.

## Test seams

- binaries: temp folders with `fixtures::executable` scripts printing a version: first on PATH,
  a symlink to the same file is not shadowing, same build is not, another build or none is.
- client: server tests over a real socket: Welcome carries the server's binary; a refusal names
  the pid and `saaio --stop`; `running` against a protocol-8 listener -> `server: None`.
- output::update: TOON with and without others.
- App::warn, keys, chrome: app tests and golden screens.
- send: tests/cli with SAAIO_MASTER=1 and no SAAIO_PROOF.

## Assumptions

- A different file on PATH with the same build is no mismatch.
- The UI compares the full build; `saaio update` compares versions only.
- The PATH probe runs once at UI start; up to 1 s only when another saaio comes first.
- The dialog shows once per UI start; the title line stays until the UI quits.
- `saaio update` lists the servers of every registered store.
- Paths in messages use ~.

## Tasks

- [x] Add `binaries.rs` (`Binary`, `Mismatch`, `first_on_path`, `shadowing`, `mismatches`) and make `system::version` take any program, with unit tests on scripts first on PATH (same file via symlink, same build, other build, no version)
- [x] Bump `PROTOCOL_VERSION` to 9 (own commit)
- [x] `ServerMsg::Welcome { version, server }` from `Conn::hello`, `Refused` naming the server's build and exe, `Client::server`, with server tests over a socket
- [x] `Client::connect` refusal naming the server's pid, this binary and `saaio --stop`; `client::running`; tests incl. a protocol-8 listener
- [x] `Env::path` from main.rs; `send` and `hook` say an earlier server opened a keyless pane, with a tests/cli case for send
- [ ] `saaio update`: `Others`, `others()`, `updated`/`latest` with `shadowed` and `servers[]`, with TOON tests
- [ ] UI: `UiEnv::path`, `App::warn`, `View::Mismatch`, `dialog::mismatch`, the title-row line, with app tests and golden screens
- [ ] Docs: README/docs (install.md, troubleshooting.md, cli.md, ui.md as they apply), ROADMAP (drop the row, add the "panes run the server's saaio" Planned soon row), CHANGELOG Unreleased, CLAUDE.md (binaries.rs, plan list)
- [ ] E2E: rerun the repro (0.1.1 first on PATH, this build behind it; a running 0.1.2 server; a 0.1.0 server) on a scratch store and check the dialog, title line, `saaio update` (local mirror) and `saaio send` messages; `saaio --stop` after
