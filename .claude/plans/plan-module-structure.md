# Module structure

Design page: https://claude.ai/artifact/VGAwXGWwY1hh678eKnmUFv
Audit: https://claude.ai/artifact/Lpm6RTzVR5vCuAU1UibgjE (items 2, 11, 12 splits, names, 16)

## Decision

Break the module cycle by moving the four upward imports, keep it broken with a test, split
`ui/input.rs` and `workspace.rs` along their seams, rename functions that repeat their module,
group `Server`'s per-pane and per-connection maps and `App`'s master fields, and move kill, killpg,
getuid and Linux peer creds to rustix. One commit per step. No behaviour changes.

Decided in review: news text into `wake.rs` (with `truncated`, `LIST_TEXT_LIMIT` into `text.rs`);
`Refusal` into a new `refusal.rs`; a test-only `layering.rs` cycle check; `workspace.rs` into four
files (worktrees, git, hooks, process).

## Units

### 1a. `src/config/prefix.rs` (moved from `src/ui/prefix.rs`, unchanged)

Callers import `crate::config::prefix::Prefix`.

### 1b. `src/default_file.rs`

```rust
/// What saaio writes as theme.toml on its first run.
pub const THEME_EXAMPLE: DefaultFile = DefaultFile { .. };
```

Replaces `ui::theme_file::EXAMPLE`; `server.rs` and the theme_file tests use it.

### 1c. `src/wake.rs`, `src/text.rs`

```rust
// wake.rs (moved from cli/output/news.rs, which goes)
pub fn for_worker(messages: &[&str]) -> String;
pub fn for_master(flows: &[&FlowView]) -> String; // serde_toon::to_string directly

// text.rs (moved from cli/output.rs)
pub const LIST_TEXT_LIMIT: usize = 80;
pub fn truncated(text: &str, limit: usize, full: bool) -> String;
```

Callers: `cli/commands/wait.rs`, `cli/commands/hook.rs`, `server/news.rs`, cli outputs.

### 1d. `src/refusal.rs` (new)

```rust
/// Work `saaio remove` refuses to lose unless told to; saaio ui asks the person instead.
pub enum Refusal { Uncommitted, Unmerged }
impl Refusal {
    pub fn of(message: &str) -> Option<Refusal>;
    /// The words remove's message carries: "has uncommitted work", "on no other branch".
    pub fn phrase(self) -> &'static str;
}
```

`cli/output/remove.rs` builds its messages from `phrase`; ui imports `crate::refusal::Refusal`.

### 1e. `src/layering.rs` (new, `#[cfg(test)] mod layering;` in lib.rs)

```rust
/// Each top-level module's production `crate::` uses: test modules, tests.rs and fixtures left out.
fn uses(src: &Path) -> BTreeMap<String, BTreeSet<String>>;
/// The first cycle in the graph, as the modules on it.
fn cycle(graph: &BTreeMap<String, BTreeSet<String>>) -> Option<Vec<String>>;
#[test] fn the_top_level_modules_form_no_cycle();
```

### 2. `ui/input.rs` split

- `ui/input.rs`: `Router`, `Routed`, `Pointer`, `PointerKind`, `Target`, `Step`, `Command`, `flush`; Router tests.
- `ui/input/keys.rs`: `Key`, `decode`, `csi_len`, `utf8_len`, decode tests; `pub use keys::{Key, decode}` in input.rs.
- `ui/input/mouse.rs`: `MouseEvent`, `mouse`, `encode`, `wanted` as `pub(super)`.

### 3. `workspace.rs` split

- `workspace.rs`: `create_worktree`, `remove_worktree`, `exclude_worktrees`.
- `workspace/git.rs`: `repo_root`, `common_dir` (was `git_common_dir`), `current_branch`,
  `has_uncommitted_work`, `commits_only_on`, `branch_exists`, `remote_urls`, `ssh_hostname`,
  `delete_branch`; private `git`, `worktree_git`, `registered_git_dir`.
- `workspace/hooks.rs`: `Hook`, `run`, `run_within`, `exists` (were `run_hook`, `run_hook_within`,
  `has_hook`), `file`, `is_executable`.
- `workspace/process.rs`: `output_in`, `finish_within`, `Pipe`, `read_in_background`, `run`,
  `stdout_of`, `last_line`, `WAIT_STEP`, `READ_GRACE`.
- Tests with what they test; shared `git`, `repo`, `hook` helpers in `workspace/fixtures.rs`.
- Callers use `workspace::git::..`, `workspace::hooks::..`; no re-exports.

### 4. Renames

`config::config_file` -> `config::path`, `slots::taken_slots` -> `slots::taken`,
`pane::pane_env` -> `pane::env`, `worktree::worktree_add_argv` -> `worktree::add_argv`,
`worktree::worktree_remove_argv` -> `worktree::remove_argv`. `Env::config_file` stays.

### 5. `src/server.rs`

```rust
/// What the server keeps about one pane, kept after it closes as the maps were.
#[derive(Debug, Default)]
struct PaneState {
    adapter: Option<Adapter>,
    resumed: Option<Instant>,
    closing: Vec<ConnId>,
    typed_at: Option<Instant>,
    opened_at: Option<Instant>,
    line_at: Option<Instant>,
    dismissed: Option<u64>,
}

/// One UI connection: its socket, and who it said it is once its hello came.
#[derive(Debug)]
struct Conn { stream: UnixStream, peer: Option<Peer> }

struct Server { panes: HashMap<PaneKey, PaneState>, conns: HashMap<ConnId, Conn>, .. }
impl Server { fn pane(&mut self, key: &PaneKey) -> &mut PaneState; }
```

Entries are never dropped at close (as today); `resumed` and `closing` are taken (`Option::take`,
`mem::take`) where the maps removed them.

### 6. `src/ui/app.rs`

```rust
#[derive(Debug, Default)]
struct Master { place: Option<String>, cwd: Option<String>, turn: Option<MasterTurn>, unseen: bool }
pub struct App { master: Master, .. }
```

### 7. rustix

`rustix = { version = "1.1.5", features = ["process", "net"] }`. `client.rs`: `kill_process`,
`getuid`, Linux `socket_peercred`. `server/host.rs`: `Pid`, `kill_process_group`. libc stays for
`setsid` in `pre_exec`, `localtime_r`, macOS `LOCAL_PEERPID`.

## Call chains

Unchanged: every step moves or regroups code. Error paths stay where they are.

## Test seams

- Moves and splits: the existing tests move with their code.
- `layering`: fails before 1a-1d, passes after.
- Server/App regrouping: `server/tests.rs` over a real socket and `ui/app/tests/`.
- rustix: `saaio --stop` in `tests/cli.rs`, pane close in server tests; Linux peercred on CI.

## Assumptions

- No user-visible change: no CHANGELOG or docs entry.
- Moves use `git mv`.
- CLAUDE.md's Architecture list is updated in the commit that moves the file.

## Tasks

- [x] 1a. Move `Prefix` to `config/prefix.rs`
- [x] 1b. Move the theme example to `default_file::THEME_EXAMPLE`
- [x] 1c. Move the news text to `wake::for_worker`/`for_master`, `truncated` to `text`
- [x] 1d. Move `Refusal` to `refusal.rs` with `Refusal::phrase`
- [x] 1e. Add `layering.rs` with `the_top_level_modules_form_no_cycle`
- [ ] 2. Split `ui/input.rs` into `input.rs`, `input/keys.rs`, `input/mouse.rs`
- [ ] 3. Split `workspace.rs` into `workspace.rs`, `git.rs`, `hooks.rs`, `process.rs`
- [ ] 4. Rename `config::path`, `slots::taken`, `pane::env`, `worktree::add_argv`/`remove_argv`
- [ ] 5. Group `Server`'s pane maps into `PaneState` and `conns`/`peers` into `Conn`
- [ ] 6. Group `App`'s master fields into `App.master`
- [ ] 7. Replace libc with rustix for kill, killpg, getuid and Linux peer creds
- [ ] End to end: bare `saaio` on a scratch store in a PTY, start a flow, close it, `saaio --stop`
