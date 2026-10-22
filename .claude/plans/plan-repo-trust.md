# Plan: repository trust, hook timeouts, hook process groups

No review page: the request asked to go with the recommendation at every decision, so the design was not stopped for review.

## Decision

- saaio runs a repository's `.saaio/setup`, `teardown`, `pause` and `resume`, and passes Codex `trust_level="trusted"` for its worktrees, only once the repository is trusted. Trust is recorded per repository in the shared folder (`<registry>/trusted/<fnv1a of the path>`, the path as its one line), so every store and project sees the same answer.
- `saaio start` refuses an untrusted repository that needs trust (it has an executable hook, or the agent's adapter is Codex) before anything is created. The refusal names the repository and what would run, with the phrase `is not trusted` (`Refusal::Untrusted`). saaio ui reads it back and asks the person in its confirm dialog; yes records the trust in-process and starts again.
- `saaio trust [FOLDER]` grants trust only in a person's terminal (`Env::is_person`), so an agent following a help line cannot grant it. `saaio trust --revoke [FOLDER]` and `saaio trust --list` work anywhere.
- The check sits at the cause: `hooks::run` takes the trust and refuses an untrusted repository. Teardown of an untrusted repository is skipped and remove says so; the pause watch leaves untrusted repositories alone; plain sessions and workers get Codex folder trust only when trusted (else Codex asks in its own pane).
- Every hook runs with a limit: setup 10 min, teardown 2 min, pause and resume 5 min. Every command run with a deadline (`process::finish_within`) runs in its own process group, and the whole group is killed past the limit.
- `create_worktree` refuses a repository whose `.saaio` or `.saaio/worktrees` is a symbolic link.

## Units

`src/trust.rs` (new): the repositories the person trusts.

```rust
/// The repositories whose hooks saaio runs and whose worktrees Codex trusts: <registry>/trusted/<hash>.
#[derive(Debug, Clone)]
pub struct Trust { root: PathBuf }

impl Trust {
    pub fn at(registry: &Registry) -> Trust;
    pub fn covers(&self, repo: &Path) -> bool;
    pub fn grant(&self, repo: &Path) -> Result<()>;
    /// Ok(false) when it was not trusted.
    pub fn revoke(&self, repo: &Path) -> Result<bool>;
    /// Sorted.
    pub fn repos(&self) -> Vec<PathBuf>;
}

/// The folder trust is kept for: a worktree's repository, a plain session's own folder.
pub fn root_of(space: &Workspace) -> PathBuf;
```

`src/workspace/hooks.rs` (modified):

```rust
impl Hook {
    /// setup 10m, teardown 2m, pause and resume 5m.
    pub fn limit(self) -> Duration;
    pub fn name(self) -> &'static str;
}

/// Runs <repo>/.saaio/<hook> within hook.limit(); Ok(false) when there is none; an Err naming the
/// repository when trust does not cover it.
pub fn run(trust: &Trust, repo: &Path, hook: Hook, workspace: &Workspace, env: &[(String, String)]) -> Result<bool>;
/// `run` with a limit of its own (tests, short limits).
pub fn run_within(trust: &Trust, repo: &Path, hook: Hook, workspace: &Workspace, env: &[(String, String)], limit: Duration) -> Result<bool>;
/// The executable hooks a repository has, in Hook order.
pub fn present(repo: &Path) -> Vec<Hook>;
```

`src/workspace/process.rs` (modified): `finish_within` spawns with `process_group(0)` and on timeout `rustix::process::kill_process_group(pid, Signal::KILL)` before `wait`.

`src/workspace.rs` (modified): `create_worktree` calls `refuse_links(repo)?` first.

```rust
/// Fails when <repo>/.saaio or <repo>/.saaio/worktrees is a symbolic link.
fn refuse_links(repo: &Path) -> Result<()>;
```

`src/refusal.rs`: `Refusal::Untrusted`, phrase `"is not trusted"`.

`src/cli/commands/trust.rs` (new): `Args { folder: Option<PathBuf>, revoke: bool, list: bool }`, `run(args, env) -> Result<Document, Failure>`, run without a store (like update), the trust from `Env::registry`, the folder from `current_dir`. The root is `git::repo_root(folder)` else the folder made absolute and canonical.

`src/cli/output/trust.rs` (new): `trusted(repo, hooks)`, `revoked(repo, was)`, `list(repos)`, `untrusted(repo, hooks, codex) -> Failure`, `not_a_person() -> Failure`.

`src/cli/commands/start.rs`: after `repo` and agent are known, before the flow is created, `refuse_untrusted(&trust, &repo, adapter)?`; `hooks::run` gets the trust. `run` takes `&Trust` (from `Env::registry`; none -> nothing trusted).

`src/cli/commands/retire.rs`: `retire_worktree(trust, flow, uncommitted) -> (Retired, Teardown)`; `Teardown { None, Ran, Failed(String), Untrusted }` in `cli/output/remove.rs`, printed as `teardown:` in remove; clear lists `untrusted[n]` flows with a help line. `remove::run` and `clear::run` take `&Trust`; `start::run` builds it from its registry.

`ServerPaths` gains `trust: Option<Trust>`, kept on `Server`; `HOOK_LIMIT` is gone.

`src/launch/pane.rs`: `worker_spec(flow, agent, env, trusted: bool)`; `Adapter::wiring(flow_exe, folder: Option<&Path>)` and `plain_wiring` the same: `None` leaves out `codex::trusted_folder`.

`src/server`: `Server` keeps `trust: Trust` (from the server command's registry); panes and reconcile pass `trust.covers(&trust::root_of(space))`; `PauseWatch` skips repos trust does not cover (`scripted_repo(flow, trust)`), `run_in_background` passes the trust to `hooks::run`; `HOOK_LIMIT` goes (Hook::limit).

`src/ui`: `View::Confirm { question, then: Confirmed }` with `enum Confirmed { Remove(Removal), Trust(NewFlowForm) }`; `on_ran` turns `(Job::Start(form), Err(Untrusted))` into a Confirm `"{message}. Trust it?"`; `y` on Trust gives `[Effect::Trust(form.repo.clone()), Effect::Run(Job::Start(form))]`; `ui.rs` performs `Effect::Trust` with `UiEnv::trust: Option<Trust>`, trusting `git::root_or_folder(folder)` (shared with `saaio trust`); a failure goes to server.log and the start that follows asks again. `Confirmed` lives in `ui/app.rs`.

## Call chains

`saaio start` in an untrusted repo with `.saaio/setup`:
1. cli::run -> start::run(args, store, registry, trust, ...)
2. start::run -> git::repo_root(cwd): repo
3. start::run -> refuse_untrusted(trust, repo, adapter) -> hooks::present(repo): [Setup]; trust.covers(repo): false -> Err(output::trust::untrusted(repo, [Setup], false))
4. Nothing created; exit 1; message `<repo> is not trusted: saaio would run .saaio/setup`; help: ask the person to run `saaio trust` there, or start from saaio ui.

saaio ui, same repo:
1. Job::Start fails -> outcome: JobError { refusal: Some(Untrusted) }
2. on_ran -> View::Confirm { question: "... Trust it?", then: Confirmed::Trust(form) }
3. `y` -> [Effect::Trust(repo), Effect::Run(Job::Start(form))]; ui loop -> Trust::grant(repo); then the job runs and start passes.

Hook past its limit: hooks::run -> finish_within -> deadline -> kill_process_group(KILL) -> wait -> Err("no answer within 10m") -> start appends Blocked "setup failed: no answer within 10m".

Symlinked `.saaio/worktrees`: start -> workspace::create_worktree -> refuse_links -> Err(".saaio/worktrees is a symbolic link: ...") -> Rollback removes the flow and slot; exit 1.

## Test seams

- trust.rs: grant, covers, revoke, repos over a temp registry.
- hooks.rs: untrusted refused; limit; process group: a hook that starts `sleep` in the background and then hangs: after the timeout its child pid is gone.
- workspace.rs: symlinked `.saaio` and `.saaio/worktrees` refused, nothing created at the link target.
- adapter / pane: no `trusted_folder` arguments when not trusted.
- pause_watch: untrusted repo not scripted.
- UI app tests: Untrusted start -> Confirm -> y -> Trust then Run effects.
- tests/cli: start refuses untrusted (setup not run, no worktree, no flow); `saaio trust` refuses without a terminal, grants in a PTY (Terminal helper); `--list`, `--revoke`; remove of an untrusted repo says teardown skipped; a hanging teardown no longer blocks remove (short limit not settable from CLI -> covered at unit level); symlinked `.saaio/worktrees` refused.

## Assumptions

- Trust is keyed by the path git names (`--show-toplevel`), not by hook contents: a later change to `.saaio/setup` is not asked again.
- `saaio trust` grants without a y/N prompt: running it in a person's terminal is the answer. It prints the hooks that will run.
- A Claude Code worker in a repo with no hooks needs no trust; a Codex worker always does.
- Plain sessions are never refused; Codex in an untrusted plain folder shows its own trust prompt.
- Limits are fixed (10m / 2m / 5m), not configurable.
- Process groups apply to every deadline command (gh, glab, curl), not only hooks.

## Tasks

- [x] Process group: `finish_within` runs its command in its own process group and kills the group past the limit; unit test that a hung command's background child is gone after the timeout; remove the ROADMAP row
- [x] Symlinks: `refuse_links` in `create_worktree` with unit tests for `.saaio` and `.saaio/worktrees` links; CLI test that `saaio start` refuses and creates nothing at the target
- [x] `src/trust.rs`: `Trust` (at, covers, grant, revoke, repos) and `root_of`, with unit tests
- [x] Hooks: `Hook::limit`, `Hook::name`, `hooks::present`, trust-checked `run`/`run_within`; callers in start, retire and pause_watch pass the trust (start refuses untrusted via `refuse_untrusted` and `Refusal::Untrusted`; retire returns `Teardown`; pause watch skips untrusted); unit and CLI tests
- [x] `saaio trust` command (grant in a person's terminal, `--revoke`, `--list`) with output documents and CLI tests (PTY grant, refused without a terminal)
- [x] Codex folder trust only when trusted: `wiring`/`plain_wiring` take `Option<&Path>`, `worker_spec` takes `trusted`, the server computes it; tests
- [x] saaio ui asks: `Confirmed`, `Effect::Trust`, `UiEnv::trust`; app tests
- [ ] Docs (cli.md, repository.md, flows.md, troubleshooting.md, README where hooks are described), CHANGELOG Unreleased lines, CLAUDE.md architecture lines for trust.rs and the trust command
- [ ] End to end: scratch SAAIO_HOME repo with setup/teardown: start refused, `saaio trust` in a PTY, start runs setup, hanging hook killed with its children, symlinked `.saaio/worktrees` refused, remove, `--revoke`; server stopped, nothing left on disk
