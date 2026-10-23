# Plan: hardening, files and paths (0.1.2)

Review page: https://claude.ai/artifact/6ujR4VzCBj6iNE4VXuisNm

## Decision

1. `saaio bug-report` writes into `reports/` in the shared folder, never into the current folder.
2. The peek reads a Codex transcript reported in a Session event only when its real path lies under the real `<codex home>/sessions` (`$CODEX_HOME`, else `~/.codex`).
3. `Registry::register` closes the shared folder's root (0700) on every call, before its early return.
4. A project store is never taken over: an owner other than this repository, gone or not, means the next free number.
5. `.saaio/teardown`, `.saaio/pause` and `.saaio/resume` keep the worktree as their folder, with `GIT_DIR` set to the worktree's registered git dir (resolved from the main repository, never from the worktree's `.git` file) and `GIT_WORK_TREE` to the worktree. Setup is unchanged.

## Units

### src/cli/commands/bug_report.rs

```rust
/// The folder under the shared folder that holds bug reports.
const REPORTS_DIR: &str = "reports";

pub fn run(env: &Env, clock: fn() -> u64) -> Result<Document, Failure>;
// dir = env.registry()?.root().join(REPORTS_DIR), made by private::create_dir.
// No registry: Failure "cannot find a state directory", help "Set HOME or XDG_STATE_HOME to a directory".
// The store is still looked up from the cwd for the facts; write(dir, ..) unchanged.
```

### src/cli/output/bug_report.rs

```rust
pub(crate) fn written(path: &Path) -> Document;
// help gains: "Attach it from there; it is never written into a repository"
```

### src/main.rs, src/cli/env.rs

```rust
// Env, read in main.rs as path("CODEX_HOME")
pub codex_home: Option<PathBuf>,

/// Codex's home, for the peek: $CODEX_HOME, else ~/.codex.
pub(crate) fn codex_home(&self) -> PathBuf;
```

### src/ui.rs, src/cli/commands/ui.rs

```rust
pub struct UiEnv { pub claude_home: PathBuf, pub codex_home: PathBuf, .. }
// ui::run: PeekCache::new(env.claude_home, env.codex_home)
```

### src/ui/peek.rs

```rust
/// The flow's transcript: the rollout its harness reported when it lies in Codex's sessions
/// folder, else Claude Code's, found by the flow id it runs as.
pub fn transcript_of(flow: &FlowView, claude_home: &Path, codex_home: &Path)
    -> Option<(PathBuf, Format)>;

pub struct PeekCache { claude_home: PathBuf, codex_home: PathBuf, .. }
pub fn new(claude_home: PathBuf, codex_home: PathBuf) -> PeekCache;
```

A reported path outside the sessions folder yields None: no peek, never Claude's transcript instead.

### src/ui/peek/codex.rs

```rust
/// The reported rollout's real path when it lies under the real <codex home>/sessions; None for
/// any other file, or when either cannot be resolved.
pub(super) fn rollout(codex_home: &Path, reported: &str) -> Option<PathBuf>;
```

### src/registry.rs

```rust
/// Closes the shared folder (0700), then writes stores/<hash> when it is missing.
pub fn register(&self, store: &Store) -> Result<()>;

/// projects/<name>, or <name>-<n> when another repository owns that name, whether it still
/// exists or not. Writes the repo file only into a store that has none.
pub fn project_store(&self, git_dir: &Path) -> Result<Store>;

/// Links the repo file into place whole; Err AlreadyExists when another process won.
fn claim(store: &Path, git_dir: &Path) -> io::Result<()>; // loses `free`

enum Owner { Free, Repo(PathBuf) } // an unreadable repo file is a foreign owner
```

### src/workspace/git.rs

```rust
// was private, now used by hooks.rs
pub fn registered_git_dir(repo: &Path, worktree: &Path) -> Result<PathBuf>;
```

### src/workspace/hooks.rs

```rust
impl Hook {
    /// Whether git in it is pinned to the worktree's registered git dir: every hook that runs
    /// after the worker had the worktree (teardown, pause, resume), not setup.
    pub fn pins_git(self) -> bool;
}

/// `run` with a limit of its own. Err when a pinned hook's worktree has no registered git dir.
pub fn run_within(trust: &Trust, repo: &Path, hook: Hook, workspace: &Workspace,
    env: &[(String, String)], limit: Duration) -> Result<bool>;

/// The hook in the worktree with `env`, without the key of the session that runs it, and with
/// GIT_DIR and GIT_WORK_TREE when `git_dir` is given.
fn command(file: &Path, workspace: &Workspace, env: &[(String, String)], git_dir: Option<&Path>)
    -> Command;
```

## Data

- `Env.codex_home: Option<PathBuf>` (process env -> cli), `UiEnv.codex_home: PathBuf` (cli -> ui).
- `<shared folder>/reports/saaio-report-<date>[-n].txt`: same name and 0600, new folder (0700).
- Hook environment of teardown, pause and resume: `GIT_DIR`, `GIT_WORK_TREE`.
- `EventKind::Session.transcript`: unchanged, still stored as reported, checked on read.

## Call chains

saaio remove / clear, a flow with .saaio/teardown (pause and resume the same from `pause_watch::run_in_background`):

1. `retire::retire_worktree -> hooks::run(trust, repo, Hook::Teardown, space, env): Result<bool>`
2. `hooks::run_within`: not executable -> Ok(false); untrusted -> Err (as now)
3. `Hook::pins_git()` true -> `git::registered_git_dir(repo, Path::new(&space.path)): Result<PathBuf>`
4. `hooks::command(file, space, env, Some(&git_dir)): Command` - folder the worktree, GIT_DIR, GIT_WORK_TREE
5. `process::finish_within(command, name, limit)`
6. Error: no registered git dir -> Err "<worktree> is no worktree of <repo>", the script does not run; remove prints `teardown: failed: ...` and removes the worktree as for any failed teardown; pause shows `pause failed: ...`.
7. Setup: `pins_git()` false -> `command(.., None)`, unchanged.

The peek on a Codex worker:

1. `PeekCache::refresh(flow) -> peek::transcript_of(flow, claude_home, codex_home)`
2. session has a transcript -> `codex::rollout(codex_home, reported): Option<PathBuf>` -> `(path, Format::Codex)`
3. Error: outside `<codex home>/sessions` (links resolved) or missing -> None: no peek; tried again on the next refresh.

saaio bug-report:

1. `cli::run -> bug_report::run(env, clock)`
2. `env.registry()` -> `root.join(REPORTS_DIR)` -> `private::create_dir(dir)`
3. `write(dir, now, offset, text)` -> `output::bug_report::written(path)`
4. Error: no registry -> Failure "cannot find a state directory"; create or write fails -> Failure "could not write <path>: <error>".

Store lookup and register:

1. `Env::store(cwd) -> Registry::project_store(git_dir)`: `Owner::Repo(r) if r == git_dir` -> that store; `Owner::Repo(_)` -> next number; `Owner::Free` -> `claim(root, git_dir)` (AlreadyExists -> look again)
2. `cli::run -> Registry::register(store)`: `private::create_dir(root)` first, then the early return when the entry exists
3. Error: register errors stay ignored by `cli::run`; a store that cannot be claimed is "could not open the store".

## Test seams

- `hooks.rs`: real repo and worktree; the worktree's `.git` rewritten to a decoy git dir whose config sets `core.fsmonitor` to a marker script; teardown runs `git status` and `git rev-parse --git-dir`: marker absent, git dir is the registered one. Setup unpinned. Unregistered worktree -> Err.
- `peek/codex.rs`: `rollout` with a file under sessions, outside it, a `..` path, a link from sessions to outside.
- `peek.rs`: `transcript_of` with a foreign path -> None.
- `registry.rs`: root 0755 with the entry present -> 0700 after register; gone owner -> `shop-2`, old repo file untouched; unreadable repo file -> next number.
- `cli/env.rs`: `codex_home` defaults.
- `tests/cli/` bug-report: run in a git repo, the file lands in `$XDG_STATE_HOME/saaio/reports`, nothing new in the repo.

## Assumptions

- Setup stays unpinned: it runs before the worker, and package managers there may clone with git.
- A pinned hook whose worktree git does not know fails without running, rather than running unpinned.
- Docs tell a script that wants git on the main checkout to use `env -u GIT_DIR -u GIT_WORK_TREE git -C "$SAAIO_REPO" ...`.
- An unreadable `repo` file counts as a foreign owner.
- Existing `saaio-report-*.txt` files in repositories are left alone.
- CHANGELOG: plain user-facing lines under Unreleased, Fixed; no audit ids.

## Tasks

- [x] Registry: `register` closes the root before its early return; `project_store` never takes over a store, `claim` loses `free`; tests for an open root with the entry present, a gone owner and an unreadable repo file; files.md on a moved repository's store
- [ ] Bug report: `REPORTS_DIR` in the shared folder, the failure without one, the new help line; tests/cli bug-report test that the file lands in reports/ and nothing in the repo; cli.md, troubleshooting.md, files.md
- [ ] Codex home: `Env.codex_home` from `CODEX_HOME`, `Env::codex_home()` with tests; `UiEnv.codex_home`
- [ ] Peek: `codex::rollout` and `transcript_of`/`PeekCache` taking the codex home, with tests for inside, outside, `..` and a link out; files.md row on the Codex transcript
- [ ] Hooks: `git::registered_git_dir` public, `Hook::pins_git`, `command` with GIT_DIR and GIT_WORK_TREE; tests with a tampered worktree `.git` for teardown, pause and resume, setup unpinned, an unregistered worktree failing; repository.md on the hooks' git variables
- [ ] CHANGELOG lines under Unreleased, Fixed
- [ ] E2E: built binary on a scratch store: `saaio bug-report` in a repo, a moved-repo store, and `saaio start` then a tampered worktree `.git` then `saaio remove` whose teardown never reads the decoy
