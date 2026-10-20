# Security fixes for 0.1.0

Implementation page: https://claude.ai/artifact/Ms8yEn42aA4oT6yb2Tbi1V
(approaches: https://claude.ai/artifact/FNDyPT2NkxCf7mnhZXYfi4, audit: https://claude.ai/artifact/Cz8VLZ1mr1dvMj43ApqATL)

## Decision

Eight findings of the pre-release audit, each fixed where the untrusted value enters or leaves
(approach A). Three shared pieces: `text::printable` for anything printed or typed into a
terminal, a new `private` module for folders 0700 and files 0600, and `workspace::worktree_git`
for git on a worker's worktree through the git dir its repository registered. Ids that become
paths or argv must be UUIDs: checked where they enter, session ids again before argv. Built-in
agent templates and `examples/config.toml` stay unchanged. No CHANGELOG entry (none before 0.1.0),
no doc change.

| Finding | Fix |
| --- | --- |
| H1 forged Codex session id becomes an option of `codex resume` | `codex::session` and the resume specs accept only UUIDs |
| H2 `saaio remove` runs git with the worktree's own `.git` | `worktree_git` via the registered git dir, fsmonitor and hooks off; forge watch runs in the main checkout |
| H3 `saaio bug-report` writes through a symlink | `private::write_new` (create_new), `-2`, `-3` on a clash |
| M2 control characters in text pasted into a pane | `news::paste` passes every line through `text::printable` |
| M3 store, logs, server.log created with umask modes | `private` everywhere saaio creates them; server start closes an existing store root |
| M5 `SAAIO_FLOW_ID` used as a path | hook and wait refuse a set id that is no UUID |
| L1 a client's claimed folder/build printed raw | `Peer::printable` on hello, `text::printable` on every UI quit text |
| L2 config type errors quote the value | `text::without_value` on toml messages |

## Units

`src/private.rs` (new; private module in `lib.rs`, a line in CLAUDE.md's Architecture)

```rust
/// Creates `dir` and its missing parents 0700; makes `dir` itself 0700 when it was already there.
pub fn create_dir(dir: &Path) -> io::Result<()>;

/// OpenOptions whose create makes the file 0600.
pub fn options() -> OpenOptions;

/// `contents` in a new file 0600 at `path`; Err(AlreadyExists) when anything, a symlink too, is
/// there, so it never writes through one.
pub fn write_new(path: &Path, contents: &[u8]) -> io::Result<()>;

/// Writes `contents` to a hidden 0600 file beside `path` and renames it over, so a reader never
/// sees half a file. (Moved from store.rs; callers switch to private::write_atomic.)
pub fn write_atomic(path: &Path, contents: &[u8]) -> io::Result<()>;
```

`src/event.rs` (modified)

```rust
/// Whether `text` is a UUID: what saaio's flow and master ids and Codex's session ids are, and
/// what alone may become a file name or a resume argument.
pub fn is_uuid(text: &str) -> bool;
```

`src/text.rs` (modified)

```rust
/// `text` without control characters (C0, DEL, C1, ESC above all), safe to print or type.
pub fn printable(text: &str) -> String;

/// A toml error message without the value it quotes ("invalid type: string \"sk-..\", expected a
/// sequence" -> "invalid type: a string, expected a sequence"; invalid value and unknown variant
/// alike); other messages unchanged.
pub fn without_value(message: &str) -> String;
```

`src/workspace.rs` (modified)

```rust
/// The git dir the repository registered for `worktree`: the <common>/worktrees/<n> whose
/// gitdir file names a .git in that folder (a relative one read from <n>, as
/// worktree.useRelativePaths writes it; folders compared canonicalised, so a replaced or
/// symlinked .git does not matter); Err when none does.
fn registered_git_dir(repo: &Path, worktree: &Path) -> Result<PathBuf>;

/// git --git-dir=<registered> --work-tree=<worktree> -c core.fsmonitor=false
/// -c core.hooksPath=/dev/null <args>: nothing the worker wrote into its worktree configures it.
fn worktree_git(repo: &Path, worktree: &Path, args: &[&str]) -> Result<String>;

// changed: takes the repository, runs status through worktree_git
pub fn has_uncommitted_work(repo: &Path, workspace: &Workspace) -> Result<bool>;
```

`src/cli/commands/retire.rs` (modified): `refuse_uncommitted_work` passes the repo from
`worktree::repo_of(space)`; `has_uncommitted_work(..).unwrap_or(true)` - fails closed, refusing
with today's "uncommitted work" text; `--force` still goes through.

`src/forge.rs` + `src/server/forge_watch.rs` (modified)

```rust
pub struct Ask {
    pub id: String,
    pub dir: PathBuf,   // the worktree: is_dir() still says whether to ask
    pub repo: PathBuf,  // new: the main checkout gh/glab and remote_urls run in
    pub branch: String,
    pub since: u64,
    pub shown: Option<String>,
}
```

`asks` sets `repo` from `worktree::repo_of(space)` and skips a flow without one.

`src/adapter/codex.rs` (modified): `session(payload)` returns None unless
`event::is_uuid(id)`.

`src/launch/pane.rs` (modified): `worker_spec` filters `flow.session` through `event::is_uuid`;
none left means a fresh start (agent_argv), as for a worker that never had a turn.

`src/server/master.rs` (modified): `Master::spec` filters `session` through `event::is_uuid`
before `Start::Resume` picks it.

`src/cli/commands/hook.rs` + `src/cli/commands/wait.rs` (modified): a `SAAIO_FLOW_ID` that is set
but no UUID is refused, never treated as unset (send's worker check relies on `is_some`):
hook logs `SAAIO_FLOW_ID "<id>" is no flow id` and returns `Document::Silent`; wait returns
`Document::Silent`.

`src/server/news.rs` (modified): `paste(text)` same signature; each line through
`text::printable`, joined with `\n`.

`src/proto.rs` (modified)

```rust
impl Peer {
    /// This peer with its folder and build stripped of control characters: what a client claims
    /// about itself, safe for server.log and another UI's terminal.
    pub fn printable(self) -> Peer;
}
```

`src/server/conn.rs` (modified): `message` passes `peer.printable()` to `hello`; `bind` creates
the folder with `private::create_dir`.

`src/ui/app/messages.rs` (modified): `Refused{reason}` -> `Effect::Quit(text::printable(&reason))`,
`Detached{by}` -> `Effect::Quit(text::printable(&format!(..)))`.

`src/bug_report.rs` (modified)

```rust
/// The n-th report of a day: 1 is saaio-report-2026-10-20.txt, 2 is saaio-report-2026-10-20-2.txt.
pub fn file_name(at: u64, offset_secs: i64, n: u32) -> String;
```

`src/cli/commands/bug_report.rs` (modified): tries n = 1..=100 with `private::write_new`, next on
AlreadyExists; past 100 or on another error a Failure "could not write <path>: <error>".

Store files (modified): every production `create_dir_all` -> `private::create_dir`, every
file-creating `OpenOptions::new()`/`fs::write` -> `private::options()`/`write_new`/`write_atomic`:
`store.rs:93,96,205,362,383`, `registry.rs:63,164,172`, `slots.rs:72,74`, `server_log.rs:48`,
`client.rs:293,300`, `server/master.rs:71,75`, `server/conn.rs:240`, `default_file.rs:30,33`.
`saaio server` start: `private::create_dir(store.root())`, closing an existing store.

`src/config.rs` + `src/ui/theme_file.rs` (modified): `config.rs:126` and `theme_file.rs:117` print
`text::without_value(error.message())`; file:line stays.

## Data

- `forge::Ask.repo: PathBuf`, server-internal, never serialised.
- No change to the event log, proto messages (`PROTOCOL_VERSION` stays), `--json` or config.
- On disk: new folders 0700, new files 0600; bug reports may be named
  `saaio-report-<date>-<n>.txt`.

## Call chains

a. Codex session (H1)
1. `hook::run -> Adapter::translate(payload, workflow): Vec<EventKind>`
2. `codex::translate -> codex::session(payload): Option<EventKind>` -> `event::is_uuid(id)`;
   false: None, nothing recorded
3. `hook::run -> Store::append(id, Session)`, or `record_master_session -> Store::set_master_session(id)`
4. later `panes -> pane::worker_spec(flow, agent, env): Option<PaneSpec>` filters the session
   through `event::is_uuid`
5. `worker_spec -> pane::resume_argv(agent, id, name, session)`, or `agent_argv` with no valid session
6. master: `Master::spec(start, agent, env, instructions, session)` filters the same way

b. Bad `SAAIO_FLOW_ID` (M5)
1. `hook::run -> Env::flow_id(): Some("../../x")`
2. `-> event::is_uuid: false -> Log::error(..) -> Document::Silent`; no path built
3. `wait::run` likewise -> `Document::Silent`
4. `send::run` unchanged: `flow_id().is_some()` still refuses

c. `saaio remove` (H2)
1. `remove::run -> retire::refuse_uncommitted_work(flow): Result<(), Failure>`
2. `-> worktree::repo_of(space)`; None (plain): Ok
3. `-> workspace::has_uncommitted_work(&repo, space): Result<bool>`
4. `-> registered_git_dir(repo, path)`: `git_common_dir(repo)`, each `worktrees/*/gitdir`
   whose folder, canonicalised, is `path`
5. `-> worktree_git(repo, path, ["status", "--porcelain"])`
6. error (none registered, git fails): `unwrap_or(true)` -> Failure "has uncommitted work";
   `--force` goes through
7. `remove_worktree` (git worktree remove, through the repo) refuses a worktree whose `.git`
   points elsewhere: remove says `worktree: "kept: <path>"`

d. Forge watch (H2)
1. `ForgeWatch::due -> forge::asks(flows)`, `repo` from `repo_of`
2. `forge_watch::ask`: `ask.dir.is_dir()` else Unavailable
3. `-> workspace::remote_urls(&ask.repo)`, `workspace::output_in(&ask.repo, argv, ASK_LIMIT)`

e. `saaio send --type` (M2)
1. `send::run` appends Typed, unchanged
2. `typing -> news::paste(text): Vec<u8>`, lines through `text::printable`
3. `-> Host::write(key, bytes)`

f. UI takeover (L1)
1. `conn::message(Hello{peer}) -> Server::hello(conn, version, role, peer.printable())`
2. `-> send_to(old, Detached{by: peer})`, server.log line with the clean peer
3. `App -> Effect::Quit(text::printable(..))` for Detached and Refused
4. `main::answer -> println!`

g. `saaio bug-report` (H3)
1. `bug_report::run -> bug_report::file_name(now, offset, n)` for n = 1..=100
2. `-> private::write_new(cwd.join(name), text)`; AlreadyExists: next n
3. prints `report: <path>`; other error: Failure as today

h. Store writes (M3)
1. `Store::create -> private::create_dir(flows) -> private::options().write(true).create_new(true).open(log)`
2. `saaio server` start -> `private::create_dir(store.root())`

i. Config type error (L2)
1. `Config::parse -> toml::from_str(text).map_err(at)`
2. `at -> text::without_value(error.message())`, `.context(place(path, line))`

## Test seams

- `private`: modes under umask 022; `write_new` refuses an existing file and a dangling symlink;
  `create_dir` tightens an existing 0755 folder.
- `event::is_uuid`: v4 and v7 pass; `-cnotify=..`, `../x`, `7f3c`, "" fail.
- `text::printable`: ESC, BEL, C1, `\r` removed, Unicode kept. `without_value`: type, value and
  variant messages redacted, an unknown-key message unchanged.
- `codex::session`: a forged id records no Session; short-id fixtures move to UUIDs.
- `pane::worker_spec`, `Master::spec`: a stored forged session starts fresh, never in the argv.
- `workspace`: real git in a temp repo; a worktree whose `.git` points at a planted gitdir with
  `core.fsmonitor` creates no marker; dirty and clean still read right.
- `news::paste`: `ok\e[201~\e[B\x03` -> one paste, no ESC inside.
- `Peer::printable`, `messages.rs`: Detached and Refused with OSC 52 quit with clean text.
- `tests/cli.rs`: bug-report beside a planted symlink leaves the target untouched and writes
  `-2`; hook with `SAAIO_FLOW_ID=../../x` creates nothing outside the store; remove of a worktree
  with a planted fsmonitor runs nothing; a new store's folders and logs are 0700/0600.

## Assumptions

1. Codex session ids are always UUIDs.
2. A session id failing the check means a fresh Codex session, never a pane that fails to open.
3. gh/glab in the main checkout find the same merge requests (by branch and shared remotes).
4. `saaio remove` fails closed when git cannot answer; `--force` is the way out.
5. Pasted text keeps line breaks and drops every other control character, Tab included.
6. Bug reports go up to `-100` a day.
7. Theme errors get the same redaction for consistency.
8. No CHANGELOG entry. Docs change only where they describe what changed: `docs/cli.md` on the
   bug report's name.

## Decisions during review

- Approach A; types for ids and terminal text come after the release.
- `saaio server` closes an existing store root to 0700 when it starts.

## Tasks

Each shared piece lands in the commit of its first user: alone it is dead code, which
`clippy -D warnings` refuses.

- [x] Add `event::is_uuid` with unit tests (v4, v7, option-like, traversal, short, empty)
- [x] H1: `codex::session` and the resume specs (`pane::worker_spec`, `Master::spec`) accept only UUID sessions; fixtures to UUIDs; tests for a forged session
- [x] M5: hook and wait refuse a set `SAAIO_FLOW_ID` that is no UUID; `tests/cli.rs` traversal test
- [x] H2: `workspace::registered_git_dir`, `worktree_git`, `has_uncommitted_work(repo, workspace)`, retire fails closed; fsmonitor test with real git; `tests/cli.rs` remove test
- [x] H2: `Ask.repo`, forge watch runs remote_urls and gh/glab in the main checkout; forge tests updated
- [x] Add `src/private.rs` (`write_new`, `write_atomic` moved from store.rs) with mode and symlink tests; CLAUDE.md Architecture line
- [x] H3: `bug_report::file_name(at, offset, n)` and `private::write_new` in the command; `tests/cli.rs` symlink test
- [x] M3: `private::create_dir` and `private::options` with mode tests; every store, registry, slots, server_log, client, master, conn and default_file create site through `private`; server start closes the store root; `tests/cli.rs` mode test
- [x] Add `text::printable` with unit tests
- [x] M2: `news::paste` through `text::printable`; paste test
- [x] L1: `Peer::printable` in `Server::hello`, `text::printable` on Refused and Detached quit texts; tests
- [x] L2: `text::without_value` with unit tests, used in `config.rs` and `theme_file.rs`; config error test

### End to end
- [x] Replay the audit's PoCs against a scratch store and scratch repos with the installed binary (forged session id via hook then server resume argv, fsmonitor worktree + `saaio remove`, bug-report beside a planted symlink, `SAAIO_FLOW_ID=../../x saaio hook`, `send --type` with ESC into a scratch Codex-like pane, fresh store modes, config type error in the bug report) and confirm each is closed
