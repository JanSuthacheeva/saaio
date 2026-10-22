# Who talks to saaio (0.1.1)

No review page: the request was to go with my own recommendation at every decision.

## Decision

Three findings, each fixed where the identity or the connection enters.

| Finding | Reproduced (scratch store, before the fix) | Fix |
| --- | --- | --- |
| M1 identity is only `SAAIO_FLOW_ID` / `SAAIO_MASTER` | `saaio send` without `SAAIO_FLOW_ID` sent as the master; `SAAIO_FLOW_ID=<other>` `saaio hook` appended answer + idle to that flow; `SAAIO_MASTER=1` `saaio hook` wrote `waits/master.turn` | the server hands each pane it opens a fresh key in `SAAIO_KEY` and keeps only its SHA-256 in `waits/<id or master>.key` (0600); `saaio hook`, `saaio send` and `saaio wait` act only for the session the key proves; unproven = refused (hook and wait: silent, logged; send: error) |
| M4 long store path: socket at `/tmp/saaio-<uid>/<hash>.sock`, connect before any check, no peer uid check | a listener in an open (0777) `/tmp/saaio-501` got `saaio remove`'s hello | socket under `$XDG_RUNTIME_DIR`, else `$TMPDIR`, else `/tmp` (first one whose path fits); `private::verify_dir` before every connect and bind: a folder, not a link, owned by this uid, closed to group and others; client and server both check the peer's uid |
| L7 connection handling | StopServer as the first message stopped the server; StopServer on a Control connection stopped the server | role kept per connection; nothing before Hello, one Hello, Control may only CloseFlow, else the connection is dropped and logged; Hello within 5 s; at most 16 connections; EMFILE and kin in accept pause 100 ms instead of spinning; `spawn_named` returns `io::Result` and every caller copes |

`PROTOCOL_VERSION` 7 -> 8: a server of this version hands panes keys the new hooks need, and refuses
what an older UI might send. `saaio --stop` also stops a server at the 0.1.0 location, and the UI
refuses to start a second server while one listens there.

## Units

`src/identity.rs` (new; private module in `lib.rs`, a line in CLAUDE.md's Architecture)

```rust
//! Who a session is: the key the server hands each pane (SAAIO_KEY) and its proof

/// A fresh key: 32 random bytes (getrandom) as 64 hex digits.
pub fn new_key() -> io::Result<String>;

/// SHA-256 of `key` as hex: what the store keeps, so the key itself lives only in the pane.
pub fn digest(key: &str) -> String;

/// Whether `key` is the one the server handed `waiter`'s pane; false without a key or a record.
pub fn proves(store: &Store, waiter: &Waiter, key: Option<&str>) -> bool;
```

`src/store.rs` (modified)

```rust
/// Keeps the digest of the key `waiter`'s pane was handed (waits/<name>.key, 0600).
pub fn set_key(&self, waiter: &Waiter, digest: &str) -> Result<()>;
/// The digest kept for `waiter`, if any.
pub fn key(&self, waiter: &Waiter) -> Option<String>;
// remove(id) also deletes waits/<id>.key
```

`src/launch/pane.rs` (modified)

```rust
/// The pane's key, which saaio hook, send and wait prove the session with.
pub const KEY_ENV: &str = "SAAIO_KEY";
```

`src/server/panes.rs` (modified)

```rust
/// A fresh key for the pane `spec` opens: its digest in the store, the key in spec.env;
/// Err when the pane already runs (its key stays) or the digest cannot be written.
fn hand_key(&mut self, spec: &mut PaneSpec) -> anyhow::Result<()>;
// open_pane(spec) calls hand_key before host.open
```

`src/server/host.rs` (modified)

```rust
pub fn is_running(&self, key: &PaneKey) -> bool;
// spawn: a thread that cannot start kills the child and returns Err (the pane is Failed);
// close: when the close thread cannot start, SIGKILL right away
```

`src/cli/env.rs` (modified)

```rust
pub saaio_key: Option<String>,        // $SAAIO_KEY
pub xdg_runtime_dir: Option<PathBuf>, // $XDG_RUNTIME_DIR
pub tmpdir: Option<PathBuf>,          // $TMPDIR

/// $SAAIO_KEY, unless empty.
pub(crate) fn key(&self) -> Option<&str>;
/// The store's server socket (client::socket_path with this env's private temp folders).
pub(crate) fn socket(&self, store: &Store) -> PathBuf;
/// server.log beside it.
pub(crate) fn server_log(&self, store: &Store) -> PathBuf;
```

`src/client.rs` (modified)

```rust
/// <root>/server.sock; when too long, <dir>/saaio-<uid>/<hash>.sock in the first of `temp`
/// ($XDG_RUNTIME_DIR, $TMPDIR, /tmp; empty and relative ones skipped) where it fits.
pub fn socket_path(store: &Store, temp: &[PathBuf]) -> PathBuf;
/// Where 0.1.0 put a long store's socket (/tmp/saaio-<uid>/<hash>.sock); None when the root is short.
pub fn earlier_socket(store: &Store) -> Option<PathBuf>;
// log_path removed: Env::server_log
// dial: Ok(None) when the folder is missing, Err when private::verify_dir refuses it,
//       Err when the peer's uid is not ours
// ensure_server: create_dir, verify_dir, then connect
```

`src/private.rs` (modified)

```rust
/// Ok when `dir` is a folder, not a link, owned by this user and closed to group and others;
/// Err naming what is wrong.
pub fn verify_dir(dir: &Path) -> io::Result<()>;
```

`src/proto.rs` (modified)

```rust
pub const PROTOCOL_VERSION: u32 = 8;
/// The uid of the process at the other end of a Unix socket (LocalPeerCred / SO_PEERCRED).
pub fn peer_uid(stream: &UnixStream) -> io::Result<u32>;
/// Whether a connection in `role` (None before its hello) may send `message`.
pub fn allowed(role: Option<Role>, message: &ClientMsg) -> bool;
```

`src/server/conn.rs` (modified)

```rust
/// At most this many connections at once; one more is closed.
const MAX_CONNS: usize = 16;
/// How long a connection gets to say hello.
const HELLO_WAIT: Duration = Duration::from_secs(5);
/// How long accept rests when the system is out of files or memory.
const ACCEPT_PAUSE: Duration = Duration::from_millis(100);

struct Conn { stream: UnixStream, peer: Option<Peer>, role: Option<Role> }

/// How long accept rests after `error`: ACCEPT_PAUSE for EMFILE, ENFILE, ENOBUFS, ENOMEM; None else.
fn accept_pause(error: &io::Error) -> Option<Duration>;
// accept: peer of another uid, or MAX_CONNS open -> logged and closed
// message: !proto::allowed(role, &message) -> logged ("refused <what> from <peer or a connection
//          before its hello>") and dropped
// read_in_background: read timeout HELLO_WAIT until the first message
// bind: verify_dir after create_dir
```

`src/server.rs` (modified)

```rust
/// Starts a thread with a name, so a panic in it says where it happened; Err when the system
/// cannot start one.
pub(super) fn spawn_named<F: FnOnce() + Send + 'static>(name: String, body: F) -> io::Result<()>;
// run: signals, accept and host-events threads failing -> run returns Err (server.log names it)
// forge_watch, pause_watch: a thread that cannot start is logged and tried at the next tick
// read_in_background failing -> the connection is logged and dropped
```

Commands (modified)

- `hook.rs`: claim (`SAAIO_MASTER` -> Master, `SAAIO_FLOW_ID` -> Flow) then `identity::proves`;
  unproven -> logged `SAAIO_KEY does not prove <the master|flow id>`, Silent, nothing recorded.
  `Log::new` takes the log path from `env.server_log(store)`.
- `send.rs`: `identity::proves(store, &Waiter::Master, env.key())` else `Failure` "only the
  master sends" (replaces the `SAAIO_FLOW_ID` check).
- `wait.rs`: the waiter must be proven, else Silent.
- `stop.rs`: stops `env.socket(store)` and, when it differs, a server at `client::earlier_socket`
  (its errors count as no server).
- `ui.rs` (command): refuses to start a server while one listens at `earlier_socket`:
  "a saaio server of an earlier version still runs; run `saaio --stop`".
- `start.rs`, `retire.rs`, `server.rs`, `bug_report.rs`: `env.socket` / `env.server_log`.
- `client::server_command` and `ui::run_job` also remove `SAAIO_KEY`.
- `main.rs`: reads `SAAIO_KEY`, `XDG_RUNTIME_DIR`, `TMPDIR`.

## Call chains

Pane opens: `Server::open_master / open_worker -> open_pane(spec) -> hand_key(&mut spec)
-> host.is_running? Err -> identity::new_key() -> store.set_key(waiter, digest(key))
-> spec.env.push(KEY_ENV, key) -> host.open(spec)`. hand_key Err: the master logs it; a worker's
open goes the existing Err path (Blocked, pane Failed).

Hook: `hook::run -> claim -> identity::proves(store, waiter, env.key()) -> store.key(waiter) ==
digest(key)`; false -> `Log::error`, `Document::Silent`.

Send: `send::run -> identity::proves(Master)`; false -> `Failure::error("only the master sends")`.

Connect: `Client::connect -> dial(socket) -> verify_dir(parent)` (missing -> None, refused -> Err)
`-> UnixStream::connect -> proto::peer_uid == getuid` (else Err) `-> Hello`.

Accept: `accept thread: incoming -> Err: accept_pause -> sleep / continue; Ok -> Event::Accepted ->
Server::accept -> peer_uid != getuid or conns >= MAX_CONNS -> log, close -> else Conn{role: None}
-> read_in_background (HELLO_WAIT on the first read; spawn Err -> log, drop)`.

Message: `Server::message(conn, msg) -> proto::allowed(conn.role, &msg)`; false -> log, drop_conn;
Hello -> sets role and peer.

## Test seams

- `identity`: new_key length/hex/differs; digest known vector; proves true, wrong key, no key, no record.
- `store`: set_key/key round trip 0600; remove deletes the key.
- `private::verify_dir`: ok 0700; 0755 refused; symlink refused; missing -> NotFound.
- `client`: socket_path picks XDG_RUNTIME_DIR, then TMPDIR, skips one too long, falls back to /tmp;
  earlier_socket; dial refuses an open folder; ensure_server closes the folder.
- `proto`: allowed table; peer_uid is our uid.
- `server/conn`: accept_pause per errno.
- `server/tests.rs` (real socket): StopServer before hello leaves the server running and closes the
  connection; Control StopServer refused, CloseFlow still works; a silent connection closed after
  HELLO_WAIT (timing via a test value); the 17th connection is closed; a pane's env has SAAIO_KEY
  whose digest is in the store.
- `tests/cli`: hook with no key / wrong key / another flow's key records nothing and logs; hook with
  the flow's key records; master hook without key writes no master.turn; send without key refused;
  wait without key silent. Existing hook/send/wait tests pass the key (support helper `prove`).

## Assumptions

- `waits/<name>.key` holds the hex digest; a new key on every open (a resume too).
- Unproven hook and wait stay silent (a hook's output reaches the model), logged to server.log.
- MAX_CONNS 16, HELLO_WAIT 5 s, ACCEPT_PAUSE 100 ms.
- A second Hello on a connection is refused like any message not allowed.
- `saaio report` and `saaio --stop` keep their current env handling.

## Tasks

- [x] identity: `identity::new_key`, `digest`, `proves`; `Store::set_key`, `key`, remove deletes it; `KEY_ENV`; getrandom dependency; unit tests
- [x] Server hands keys: `Host::is_running`, `Server::hand_key` in `open_pane`; `server_command` and `run_job` drop SAAIO_KEY; server test that a pane gets a key whose digest is stored
- [x] Commands prove: `Env::saaio_key`/`key`, main reads SAAIO_KEY; hook, send, wait refuse the unproven; CLI tests (forged flow, forged master, send, wait) failing before; existing tests pass the key
- [ ] `private::verify_dir` with unit tests
- [ ] Socket location: `Env::xdg_runtime_dir`/`tmpdir`, `client::socket_path(store, temp)`, `earlier_socket`, `Env::socket`/`server_log`; call sites; unit tests
- [ ] Checked connect and bind: dial verifies the folder and the server's uid; ensure_server and bind verify after create; `proto::peer_uid`; server accept refuses another uid; tests
- [ ] Earlier location: `saaio --stop` stops a server at `earlier_socket`, the UI refuses to start beside one; tests
- [ ] Connection rules: `Conn::role`, `proto::allowed`, refuse before hello / second hello / Control beyond CloseFlow; server tests failing before
- [ ] Hello deadline and connection cap: HELLO_WAIT read timeout, MAX_CONNS; server tests
- [ ] Graceful accept and spawn: `accept_pause`, `spawn_named` -> io::Result and its callers; unit test of accept_pause
- [ ] PROTOCOL_VERSION 8; CHANGELOG Fixed lines; docs (files.md socket, troubleshooting.md log path, cli.md send, flows.md hooks, agents/configuration env if listed); CLAUDE.md Architecture (identity.rs)
- [ ] End to end on a scratch store (long path, dummy master): the three repros refused, a real hook with the pane's key recorded, socket under $TMPDIR in a 0700 folder; `saaio --stop`; scratch removed
