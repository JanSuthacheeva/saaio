# Plan: one store per repository

Approaches page: https://claude.ai/artifact/5DpPiTwMi7jHQeSVgF4ihb

## Decision

saaio picks its store from the git repository it runs in: `<shared>/projects/<repo name>`, where
`<shared>` is `$XDG_STATE_HOME/saaio`, else `~/.local/state/saaio` (the registry's folder, the
default store today). `SAAIO_HOME` still overrides everything. Outside a repository, or without
git, the shared folder itself is the store, as today.

Decided (approach A, relayed by the master 2026-10-18):

- **Resolve once, pin it.** `cli::run`, `saaio hook` and `saaio wait` resolve the store from the
  current folder. `saaio ui` hands the resolved store to the server as `SAAIO_HOME`, and the server
  always hands its own store to every pane, so hooks and waits in a pane never look at their
  folder. A worker whose agent `cd`s elsewhere, or a `--plain` session in another repository,
  keeps reporting into the store its flow lives in.
- **The repository comes from git**: `git rev-parse --git-common-dir` in the folder. A worktree
  (`.saaio/worktrees/<name>`) and any subfolder give the main repository's git dir.
- **The name comes from the git dir**: `<repo>/.git` gives the repo folder's name, any other git
  dir (a bare `shop.git`, a submodule's `.git/modules/lib`) its own name without `.git`.
- **Same name, number suffix.** Each project store holds a `repo` file with the canonical git dir
  of the repository that owns it. A name owned by another repository that still exists moves on to
  `<name>-2`, `<name>-3`, ... A folder without the file (today's `SAAIO_HOME` per tmux session) is
  adopted by the first repository that resolves to it, so existing stores keep their flows.

Registry, slots and `saaio status --all` are unchanged: they already live in the shared folder,
every resolved store registers there in `cli::run`, and the project name is the store folder's.

## Units

`src/workspace.rs` (modified):

```rust
/// The git dir shared by every worktree of the repository `dir` is in (git rev-parse
/// --git-common-dir), absolute and canonical; None outside a repository or without git.
pub fn git_common_dir(dir: &Path) -> Option<PathBuf>;
```

`exclude_worktrees` uses it instead of its own `rev-parse --git-common-dir` call.

`src/registry.rs` (modified):

```rust
/// The file in a project store that names the git dir of the repository owning it.
const OWNER_FILE: &str = "repo";
/// The folder under the registry's root holding one store per repository.
const PROJECTS_DIR: &str = "projects";

impl Registry {
    /// The store of the repository whose git dir is `git_dir`: projects/<name>, or <name>-<n> when
    /// another repository that still exists owns that name. Writes the repo file when the store
    /// has none or names a repository that is gone.
    pub fn project_store(&self, git_dir: &Path) -> Result<Store>;
}

/// The project name of a git dir: the repository's folder for <repo>/.git, else the dir's own
/// name without .git; None for a root path.
fn repo_name(git_dir: &Path) -> Option<String>;

/// Who owns a candidate project store, read from its repo file: Free without one, else the git
/// dir it names (empty when it names nothing or cannot be read: taken over like a gone owner).
enum Owner { Free, Repo(PathBuf) }
fn owner(store: &Path) -> Owner;

/// Writes the repo file: written aside and hard-linked into place for a free store, so it is never
/// seen empty (Err AlreadyExists when another process won the race, and project_store looks at
/// that name again), write_atomic over a gone owner.
fn claim(store: &Path, git_dir: &Path, free: bool) -> io::Result<()>;
```

`src/cli/env.rs` (modified):

```rust
impl Env {
    /// The store: $SAAIO_HOME, else the project store of the repository `cwd` is in, else the
    /// shared default store; Ok(None) when neither $SAAIO_HOME, $XDG_STATE_HOME nor $HOME is set.
    pub(crate) fn store(&self, cwd: Option<&Path>) -> anyhow::Result<Option<Store>>;
}
```

`src/store.rs` (modified): `Store::locate` and its two tests go; `Env::store` takes their place
and their cases move to `env.rs` tests.

`src/cli.rs` (modified): `run` reads `std::env::current_dir().ok()` and calls
`env.store(cwd.as_deref())`; an Err becomes `Failure::error("could not open the store: ...")`,
Ok(None) keeps today's "cannot find a state directory".

`src/cli/commands/hook.rs`, `wait.rs` (modified): the same call; an Err is "cannot find a state
directory" (hook: on stderr), as today.

`src/cli/commands/ui.rs` (modified): `client::ensure_server(&socket, &exe, &[("SAAIO_HOME",
store root)])`.

`src/launch/pane.rs` (modified):

```rust
pub struct LaunchEnv {
    /// The server's store, passed to every pane as SAAIO_HOME.
    pub store: PathBuf,
    pub flow_exe: PathBuf,
}

/// SAAIO_HOME (the server's store), TERM=xterm-256color, COLORTERM=truecolor.
pub fn pane_env(env: &LaunchEnv) -> Vec<(String, String)>;
```

`src/cli/commands/server.rs` (modified): `LaunchEnv { store: store.root().to_path_buf(), .. }`.

## Data

On disk, new: `<shared>/projects/<name>/repo`, one line, the canonical git dir of the owning
repository (bytes of the path plus `\n`, like `stores/<hash>`). No change to the event log, the
registry entries, slots or `--json`.

Environment: every pane always gets `SAAIO_HOME` (before: only when set for the server).

## Call chains

`saaio status` in `~/projects/shop/.saaio/worktrees/shop-217/src`, no SAAIO_HOME:

1. `main -> cli::run(cli, env, clock)`
2. `cli::run -> std::env::current_dir(): .../shop-217/src`
3. `cli::run -> Env::store(Some(cwd)): Ok(Some(Store))`
4. `Env::store -> Env::registry(): Some(Registry ~/.local/state/saaio)`
5. `Env::store -> workspace::git_common_dir(cwd): Some(~/projects/shop/.git)`
6. `Env::store -> Registry::project_store(git_dir)`
   1. `repo_name(git_dir): "shop"`
   2. `owner(projects/shop)`: `Repo(~/projects/shop/.git)` -> that store; `Free` -> `claim(.., free: true)`, AlreadyExists -> look again; `Repo(other)` with `other` existing -> next name `shop-2`; `Repo(gone)` -> `claim(.., free: false)`
   3. `Ok(Store::at(projects/shop))`
7. `cli::run -> Registry::register(&store)` (as today) -> dispatch.

Errors: `git_common_dir` None (no repository, no git, git fails) -> the shared store. `claim`
fails (permissions, full disk) -> `Env::store` Err -> `cli::run` fails with "could not open the
store" and the path; hook and wait stay silent (hook writes to stderr).

`saaio ui` in a repository:

1. `cli::run -> Env::store(cwd)`: the project store (above)
2. `ui::run -> client::ensure_server(socket, exe, [("SAAIO_HOME", store root)])`
3. `saaio server` (spawned): `cli::run -> Env::store`: SAAIO_HOME -> the same store
4. `server::run(.., LaunchEnv { store, .. })` -> `pane_env`: every pane gets `SAAIO_HOME=<store>`
5. A worker's hook: `saaio hook claude-code -> Env::store`: SAAIO_HOME -> the flow's store,
   whatever its folder.

A `saaio ui` started inside a worker sees the pinned SAAIO_HOME and its own flow there, so
`refuse_inside_own_flow` refuses as today.

## Test seams

- `workspace::git_common_dir`: real `git init` and `git worktree add` in a tempdir: the main
  folder, a subfolder and a worktree give the same dir; a plain tempdir gives None.
- `registry::repo_name`: pure; `<repo>/.git`, `shop.git`, `.git/modules/lib`.
- `Registry::project_store`: tempdirs as git dirs and registry: claims, reuses its own store,
  suffixes past an existing owner, adopts a store without a repo file, reclaims from a gone owner.
- `Env::store`: SAAIO_HOME first (also inside a repository), a repository's project store, the
  shared store outside one, empty values count as unset, None without any home.
- `pane_env`: SAAIO_HOME always present; `server_command`'s existing test keeps SAAIO_HOME.
- `tests/cli.rs`: without SAAIO_HOME (XDG_STATE_HOME at the test's folder): `saaio new` in a repo
  and `saaio status` in its worktree see the same flow; outside the repo another store;
  `saaio status --all` shows it under the repo's name.

## Assumptions

- The first saaio command in a repository creates `projects/<name>/` with its repo file, even
  `saaio status`; `status --all` already skips stores without flows.
- A repository whose owner file names a git dir that no longer exists takes that store over,
  with its flows.
- Flows already in the shared store stay there: reachable with `SAAIO_HOME=~/.local/state/saaio`,
  and listed by `saaio status --all`. No migration (0.1.0 is not released).
- Git dirs are compared canonical (symlinks resolved), so a repository reached through a symlink
  gets the same store.
- The name is used as the folder name unchanged; git dirs are folders, so it holds no `/`.
- `saaio start --plain <folder>` keeps the store of the folder it is run in, not of `<folder>`.

## Build notes

- Tasks 2 and 3 are one commit: `project_store` is dead code, which clippy fails, until `Env::store` calls it.
- `Registry::default_root` became private: only `Registry::locate` uses it once `Store::locate` is gone.
- The E2E check (scratch XDG_STATE_HOME, two repos named `shop`, `saaio ui` in a pty from the
  worktree): the worker's pane got `SAAIO_HOME=<state>/saaio/projects/shop`, its `saaio report`
  run from `/` landed in that store, the twin repo got `projects/shop-2`, the shared store stayed
  empty, and `saaio status --all` listed `shop` and `shop-2`.

## Tasks

- [x] `workspace::git_common_dir` with tests for a repo, a subfolder, a worktree and no repo; `exclude_worktrees` uses it
- [x] `registry::repo_name`, `Registry::project_store` (`owner`, `claim`) with tests for claim, reuse, suffix, adopt and reclaim
- [x] `Env::store(cwd)` replaces `Store::locate`; `cli::run`, `hook` and `wait` pass the current folder; env tests for SAAIO_HOME, repo, no repo and empty values
- [x] `LaunchEnv.store` and `pane_env` always set SAAIO_HOME; `saaio ui` passes its store to the server; pane and client tests
- [x] `tests/cli.rs`: store per repository end to end without SAAIO_HOME (repo, worktree, outside, `--all`)
- [x] Docs: `docs/files.md` (store location, `projects/`, `repo` file), `docs/cli.md` (SAAIO_HOME default, every session gets SAAIO_HOME, "Several projects"), `docs/troubleshooting.md` (where server.log is, flows of another repository not shown), README, ROADMAP and CHANGELOG where they describe the store; CLAUDE.md architecture lines
- [x] E2E with a scratch XDG_STATE_HOME: two repos named alike, `saaio start` in one, its worktree's hook lands in the right store, `saaio ui` in a pty opens the worker with the pinned SAAIO_HOME, `saaio status --all`

## Review

Reviewed in an isolated context against this plan and CLAUDE.md. The one fix changed production
code. Nobody else reviewed it: the reviewer re-read it as a separate pass.

**Fixed**

- A `repo` file that was empty or had no trailing newline made every saaio command in that
  repository loop forever at 100% CPU. `owner` read the file as Free, and `claim`'s `create_new`
  then failed with AlreadyExists, again and again. A crash between `create_new` and the write left
  exactly such a file, and a racing reader could briefly see one. Shown by `saaio status` in a
  scratch repo with `: > projects/shop/repo`, which hung. Now a free claim is written aside and
  hard-linked into place, so the file is never seen empty. A file with no newline is read as it
  is, and one that names nothing, or cannot be read, is taken over like a gone owner. Tests
  `takes_over_a_store_whose_repo_file_names_nothing` and `reads_a_repo_file_without_its_newline`.
  Commit 9e9296e.

**Open**

- Existing flows stay in the shared store (as the Assumptions say). After installing, `saaio ui` in
  a repository opens an empty project store, and the old server, master and flows sit in the
  shared folder, reachable only with `SAAIO_HOME=~/.local/state/saaio`. Recommendation: finish or
  clear the running flows before installing, or say so in the release notes.
- If two different repositories with the same name take over the same gone-owner store at the same
  moment, `write_atomic` lets the last one win, and both use the store. This is very unlikely and
  left as designed.
- A repository whose git dir is on an unmounted volume counts as gone, so a same-named repository
  takes over its store and flows. This is by design (Assumptions). It is worth knowing for
  external drives.

**Where to look**

1. `src/registry.rs` `project_store` -> `owner` -> `claim`: the only new on-disk state and the
   naming loop.
2. `src/cli/env.rs` `Env::store`, then its three callers (`cli::run`, `hook.rs`, `wait.rs`): which
   store each command picks, and that hook and wait stay silent on errors.
3. `src/cli/commands/ui.rs` and `src/launch/pane.rs` `pane_env`: pinning SAAIO_HOME so panes never
   resolve the store from their folder.

`workspace::git_common_dir`, the docs, and the removal of `Store::locate` are routine.

**Checks**

`cargo test`, `cargo clippy --all-targets -- -D warnings` and `cargo fmt --check` pass at 9e9296e.
