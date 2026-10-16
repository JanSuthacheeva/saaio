# Plan: server diagnostics

Review page: https://claude.ai/artifact/XrLPYAire9fazAsshwizJz

## Decision

server.log only said "flow server stopped", so on 2026-10-16 a take-over by a worker's debug
`flow ui` (13:34:22) followed by Jan's own `flow ui --stop; flow ui` (13:39:54) looked like a crash.
The server's stderr stays server.log (approach A); every line is stamped with time and pid.

- New `src/server_log.rs` owns server.log: path, rotation, stamped lines, the panic hook.
- Logged: start, the stop reason (`Stop`), stopped, attach, take-over, the attached UI leaving,
  refused hellos, errors, panics. `flow ui --stop` appends its own line before it signals.
- Protocol 3: `Hello` carries a `Peer` (pid, cwd, build); `Detached { by: Peer }` lets the dropped
  UI say who took over. A first message the server cannot decode gets `Refused`.
- `flow ui` and `flow ui --stop` refuse inside a worker on that worker's own store and point to a
  scratch `FLOW_HOME`.
- `ensure_server` moves a log over 1 MiB to `server.log.1` before it spawns.
- Decided in review: a panic in any server thread is logged and ends the server (exit 101); a build
  names itself by the git commit from `build.rs` (no dirty flag); the scratch-FLOW_HOME rule goes in
  CLAUDE.md and the guard's help, not in `WORKER_PROMPT`.

## Units

`src/server_log.rs` (new)

```rust
/// The size past which ensure_server moves server.log to server.log.1.
pub const CAP: u64 = 1 << 20;

/// server.log beside the socket.
pub fn path(socket: &Path) -> PathBuf;
/// Moves a log over `cap` bytes to `<log>.1`, replacing the one before; nothing when under it or missing.
pub fn rotate(log: &Path, cap: u64) -> io::Result<()>;
/// One line: `2026-10-16 13:39:54 +07:00 [2739] text`.
pub fn line(at_ms: u64, offset_secs: i64, pid: u32, text: &str) -> String;
/// Writes a line to stderr, which is server.log in a server ensure_server started.
pub fn write(now: u64, text: &str);
/// Appends a line to `log`, for a process that is not the server (flow ui --stop).
pub fn append(log: &Path, now: u64, text: &str) -> io::Result<()>;
/// From now on a panic in any thread writes its line and backtrace, then ends the process (exit 101).
pub fn exit_on_panic(clock: fn() -> u64);
/// `panic in thread 'accept' at src/server/conn.rs:212:9: message`.
fn panic_text(thread: Option<&str>, location: Option<&Location>, message: &str) -> String;
```

`src/proto.rs` (modified)

```rust
pub const PROTOCOL_VERSION: u32 = 3;
/// This binary: `0.1.0 2904734`; `unknown` for the commit outside a git checkout.
pub const BUILD: &str = concat!(env!("CARGO_PKG_VERSION"), " ", env!("FLOW_COMMIT"));

ClientMsg::Hello { version: u32, role: Role, peer: Peer }   // peer last
ServerMsg::Detached { by: Peer }

/// The process at the other end: what the log and a dropped UI say about it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Peer { pub pid: u32, pub cwd: PathBuf, pub build: String }
impl fmt::Display for Peer   // `pid 4321 in /Users/jan/projects/flow (flow 0.1.0 2904734)`
```

`src/server.rs` (modified)

```rust
/// Why the server stops.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Stop {
    /// A UI's StopServer; None when its hello never came (a unit `Asked` until the Peers task).
    Asked(Option<Peer>),
    /// SIGTERM, SIGINT or SIGHUP; `from` is the sender's pid when the system says.
    Signal { name: &'static str, from: Option<u32> },
}
impl fmt::Display for Stop   // `StopServer from pid 4321 in … (flow …)` / `SIGTERM from pid 2810`

enum Event { Accepted, Message, Gone, Host, Signal { name: &'static str, from: Option<u32> }, Unreadable(ConnId) }
struct Server { …, peers: HashMap<ConnId, Peer> }

pub fn run(…) -> Result<()>;                        // writes `started (flow {BUILD}, protocol {PROTOCOL_VERSION})` after bind, `stopped` after the socket is removed
fn serve(&mut self, inbox: &Receiver<Event>) -> Result<Stop>;   // writes `stopping: {stop}` before self.stop
fn handle(&mut self, event: Event) -> ControlFlow<Stop>;
fn stop_on_signals(events: Sender<Event>) -> Result<()>;        // was stop_on_terminate; SignalsInfo<WithOrigin> for SIGTERM, SIGINT, SIGHUP; thread `signals`
// WithOrigin needs signal-hook's `extended-siginfo` feature (its `cc` build dependency was already in Cargo.lock)
```

`src/server/conn.rs` (modified)

```rust
pub(super) fn message(&mut self, conn: ConnId, message: ClientMsg) -> ControlFlow<Stop>;
fn hello(&mut self, conn: ConnId, version: u32, role: Role, peer: Peer);
    // `refused {peer}: protocol {version}`; `ui attached: {peer}` or `ui attached: {peer}; dropped {old}`
pub(super) fn refuse_unreadable(&mut self, conn: ConnId);   // Refused + `refused an unreadable hello`
pub(super) fn drop_conn(&mut self, conn: ConnId);           // `ui left: {peer}` when the attached one goes (not on a take-over); forgets the peer
fn read_in_background(conn: ConnId, stream: UnixStream, events: Sender<Event>);  // thread `conn {n}`; an undecodable first message sends Event::Unreadable
fn accept_in_background(…);                                 // thread `accept`
```

`src/server/panes.rs`, `src/server/host.rs` (modified): every `eprintln!` becomes
`server_log::write((self.clock)(), …)`; host.rs names its reader threads `pane {key}`.

`src/client.rs` (modified)

```rust
/// This process as a Peer: its pid, current folder and BUILD.
fn this_process() -> Peer;
Client::connect(socket, role)        // Hello carries this_process()
pub fn stop_server(socket: &Path, clock: fn() -> u64) -> Result<bool>;
    // appends `flow ui --stop (flow {BUILD}) sends SIGTERM to pid {pid}`,
    // `pid {pid} still listens after 5 s; sends SIGKILL`, `pid {pid} did not stop`; append errors ignored
pub fn ensure_server(…)              // server_log::rotate(&log, server_log::CAP) before opening; its error ignored
```

`src/cli/commands/ui.rs` (modified)

```rust
/// Err when $FLOW_ID names a flow in `store`: a worker's flow ui would take over the user's.
fn refuse_inside_own_flow(store: &Store, env: &Env) -> Result<(), Failure>;
pub fn run(args, store, env, clock)  // calls it first, for --stop too; passes clock to stop_server
```

`src/cli/commands/server.rs` (modified)

```rust
pub fn run(store: Store, env: &Env, clock: fn() -> u64) -> Result<Document, Failure>;
    // server_log::exit_on_panic(clock) first; Err from server::run is written `failed: {error:#}`
    // and returned; Ok is Document::Silent
```

`src/text.rs` (modified)

```rust
/// `2026-10-16 13:39:54 +07:00` of a Unix ms time in a zone `offset_secs` east of UTC.
pub fn stamp(ms: u64, offset_secs: i64) -> String;
/// How many seconds the local time zone is east of UTC now; 0 when the system cannot say.
pub fn local_offset_secs() -> i64;   // moved from ui/outer.rs
```

`src/ui/app/messages.rs` (modified): `ServerMsg::Detached { by }` quits with
`another flow ui took over: pid 4321 in ~/projects/flow/… (flow 0.1.0 2904734)` (cwd through `self.tilde`).

`build.rs` (new): sets `FLOW_COMMIT` from `git rev-parse --short HEAD`, else `unknown`;
`rerun-if-changed` on the git dir's `HEAD` and the ref it points to (via `git rev-parse --git-dir`
and `--git-common-dir`, plus `packed-refs`).

`CLAUDE.md` (modified): `server_log.rs` in Architecture; `build.rs` in Structure; a rule that E2E
runs of `flow ui`, `flow server` and `flow ui --stop` use a scratch `FLOW_HOME`.

## Data

- `Peer { pid: u32, cwd: PathBuf, build: String }`: client -> server in Hello, server -> dropped UI in Detached, into the log.
- `ClientMsg::Hello` + `peer: Peer` (last, so an older server still decodes version and role and refuses).
- `ServerMsg::Detached { by: Peer }`.
- `Stop`: `Asked(Option<Peer>)` | `Signal { name, from }`; handle -> serve -> run, into the log.
- `server::Event`: `Terminate` -> `Signal { name, from }`; + `Unreadable(ConnId)`.
- server.log / server.log.1: stamped lines, each file about 1 MiB at most at a start.

## Call chains

flow ui --stop
1. `commands::ui::run -> refuse_inside_own_flow(store, env): Result<(), Failure>`
2. `commands::ui::run -> client::stop_server(socket, clock): Result<bool>`
3. `client::stop_server -> dial -> peer_pid -> server_log::append(log, now, "flow ui --stop (flow {BUILD}) sends SIGTERM to pid {pid}")`
4. `client::stop_server -> libc::kill(pid, SIGTERM) -> stops_listening(socket, STOP_WAIT)`
5. signal thread `-> events.send(Event::Signal { name: "SIGTERM", from })`
6. `Server::serve -> handle(event): Break(Stop::Signal { .. }) -> server_log::write(now, "stopping: {stop}") -> self.stop(inbox) -> Ok(stop)`
7. `server::run -> fs::remove_file(socket) -> server_log::write(now, "stopped")`
- Error: still listening after STOP_WAIT -> append `pid {pid} still listens after 5 s; sends SIGKILL` -> kill SIGKILL; still listening after KILL_WAIT -> append `pid {pid} did not stop` -> bail as today.
- Error: inside a worker on its own store -> Failure (exit 1), nothing signalled.

A second flow ui takes over
1. `commands::ui::run -> refuse_inside_own_flow -> client::ensure_server -> ui::run -> Client::connect(socket, Role::Attach { size })` with `this_process()`
2. `Server::message -> hello(conn, version, role, peer) -> peers.insert(conn, peer)`
3. `hello -> send_to(old, ServerMsg::Detached { by }) -> drop_conn(old)` (no `ui left` for it)
4. `hello -> server_log::write(now, "ui attached: {peer}; dropped {old_peer}")`
5. `App::on_server(Detached { by }) -> Effect::Quit("another flow ui took over: {by}")`
- Error: other version -> write `refused {peer}: protocol {version}` -> Refused.
- Error: undecodable first message -> `Event::Unreadable(conn) -> refuse_unreadable -> Refused { reason }` + write `refused an unreadable hello` -> drop.
- The attached UI's connection ends -> `Gone -> drop_conn -> write "ui left: {peer}"`.

The UI's stop key
1. `App::on_stop_key -> Effect::Send(ClientMsg::StopServer)`
2. `Server::message(conn, StopServer) -> Break(Stop::Asked(self.peers.get(&conn).cloned()))`
3. Then as flow ui --stop steps 6-7.

A panic
1. `commands::server::run -> server_log::exit_on_panic(clock)` before `server::run`
2. hook `-> panic_text(thread name, location, payload) -> server_log::write(now, text)`, then `Backtrace::force_capture()` written below
3. hook `-> process::exit(101)`

Start and errors
1. `client::ensure_server -> server_log::rotate(&log, CAP)` (error ignored) -> open append -> spawn
2. `server::run -> bind -> server_log::write(now, "started (flow {BUILD}, protocol {PROTOCOL_VERSION})")`
3. `server::run` Err -> `commands::server::run -> server_log::write(now, "failed: {error:#}")` -> Err (main prints the TOON)

## Test seams

- `text::stamp`: midnight with an offset, a negative offset, a leap day.
- `server_log::line`, `panic_text`: pure. `rotate`, `append`: TempDir (under cap kept, over moved, old .1 replaced, missing fine).
- `Stop`, `Peer` Display: unit.
- server/tests.rs: take-over asserts `Detached { by }` is the second peer; a garbage first message gets `Refused`.
- ui app tests (panes.rs): Detached names the peer with ~.
- tests/cli.rs: `ui --stop` leaves the stopper's line and `stopping: SIGTERM from pid M` matching the stopper's `[M]`, then `stopped`; `FLOW_ID` of a flow in the store is refused for `ui` and `ui --stop`; one from another store is not; ensure_server rotates a log over the cap.
- Manual E2E for a real thread panic, with a temporary debug trigger removed after.

## Assumptions

1. 1 MiB cap, one old `server.log.1`, rotated only when `ensure_server` starts a server.
2. Line format `YYYY-MM-DD HH:MM:SS +HH:MM [pid] text`, local time.
3. Not logged: control connections (`flow remove`, `flow clear`), focus, input.
4. SIGINT and SIGHUP stop like SIGTERM, each logged by name.
5. A worker's own store holds `flows/<FLOW_ID>.jsonl`; a scratch `FLOW_HOME` passes the guard.
6. Server threads are named `accept`, `signals`, `conn N`, `pane KEY`.
7. `flow server` no longer prints the bare "flow server stopped".
8. `local_offset_secs` moves from `ui/outer.rs` to `text.rs`.

## Tasks

Regrouped during the build: dead code fails `clippy -D warnings`, so each task lands a unit with
its first caller.

- [x] Stamped lines: `text::stamp` (and `local_offset_secs` moved), `server_log` (`path`, `line`, `write`, `exit_on_panic`, `panic_text`), `server::run` writes started/stopped, `eprintln!` in server/ to `server_log::write`, named threads, `commands::server` (`exit_on_panic`, `failed:`, `Document::Silent`); unit tests for stamp, line, panic_text
- [x] Rotation: `server_log::CAP`, `rotate`, `ensure_server` rotates; TempDir tests and a client test
- [x] Stop reasons: `Stop` with Display, `Event::Signal` from `stop_on_signals` (WithOrigin; SIGTERM, SIGINT, SIGHUP), `handle`/`message` return `ControlFlow<Stop>`, `serve` returns `Stop` and writes `stopping:`; unit test for Display
- [x] Peers: `build.rs` (FLOW_COMMIT), `proto::BUILD`, `Peer` with Display, `Hello { peer }`, `Detached { by }`, protocol 3, `client::this_process`, `server/conn.rs` `peers` with attach/take-over/left/refused lines, `Event::Unreadable` and `refuse_unreadable`, `Stop::Asked(peer)`, the UI's take-over message with ~; proto, server and app tests
- [ ] Stopper's lines: `server_log::append`, `stop_server(socket, clock)` appends its SIGTERM/SIGKILL/did-not-stop lines; tests/cli.rs checks the stopper's and the server's lines in server.log
- [ ] Guard: `commands::ui::refuse_inside_own_flow` for ui and --stop; tests/cli.rs
- [ ] CLAUDE.md: `server_log.rs`, `build.rs`, the scratch FLOW_HOME rule
- [ ] E2E with a scratch FLOW_HOME in tmux: start flow ui, take it over from a second one, stop with C-space Q, start again, `flow ui --stop`, `kill -TERM`, a temporary panic in a thread; read server.log for each case
