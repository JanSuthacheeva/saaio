# Test structure

Review page: https://claude.ai/artifact/PXArrTXoFsC71gqGqhFzhq

Three findings of the structure audit (https://claude.ai/artifact/Lpm6RTzVR5vCuAU1UibgjE), checked against main at 75929e7: (1) guess-sleeps in `src/server/tests.rs`, (8) `tests/cli.rs` split, (15) shared unit-test builders and oversized inline tests. Tests only: no production behaviour change, no CHANGELOG line.

## Decision

- Each guess-sleep becomes a wait on something observable, with the existing `wait_until` / `wait_for`. Sleeps after a positive wait that assert nothing more happens stay.
- `tests/cli/main.rs` + `support.rs` + one module per subject: one test binary, no Cargo.toml change, a guard test for a forgotten module file.
- The shared part of the builders (the `Started` / `Workspace` literals and the fold call) moves to `crate::fixtures`; a local builder stays only where it adds test-specific shape.
- The inline tests of `config.rs`, `adapter/codex.rs`, `adapter/claude_code.rs` move verbatim to sibling `tests.rs`.

## Units

### `src/server/tests.rs` (modified)

- `a_failed_resume_retries_fresh_before_blocking`: fresh command `sh -c 'touch fresh; exec cat'`; `wait_until` `retry/fresh` exists; then assert no `Blocked`.
- `an_exited_idle_worker_keeps_its_state`: attach a UI, `wait_for(&mut ui, &mut panes, &id, Some(&PaneStatus::Exited { code: Some(23) }))`, then fold and assert `State::Idle`.
- `starts_the_master_fresh_when_its_session_cannot_be_resumed`: fresh master command `sh -c 'touch <dir>/fresh; exec cat'`; `wait_until` it exists before typing.
- The hand-rolled poll for `printed` (about :1487) becomes `wait_until("the worker never copied", || printed.exists())`.

### `tests/cli/` (new, replaces `tests/cli.rs`)

```
main.rs        mod lines + the guard
support.rs     helpers more than one subject uses (saaio*, git, PATIENCE, wait_until,
               wait_for_socket, Running, new_flow, id_in, shared, stdout, stderr, seed,
               seed_blocked, Terminal, StopsServer, Lab, executable)
home.rs        the home view, bare saaio
status.rs      status, --json, usage errors, help and version, unknown commands
report.rs      new, report, workflow phases, heartbeat, clear of finished flows
start.rs       start: worktree, setup, plain, slots, branch names, crash resume
hooks.rs       saaio hook: Claude Code, Codex, master, services, merge requests
remove.rs      clear and remove
server.rs      --stop, server lifecycle, prefix check
send.rs        send and send --type, dialogs
wait.rs        saaio wait
store.rs       a store per repository, its permissions, the forge watch
bug_report.rs  saaio bug-report
completions.rs saaio completions
```

```rust
// tests/cli/main.rs
#[test]
fn every_file_in_tests_cli_is_a_module() { ... }
```

Reads `tests/cli/*.rs` under `CARGO_MANIFEST_DIR`, fails naming each stem without a `mod <stem>;` line in `main.rs`. A helper used by one subject moves with it. Tests move verbatim.

### `src/fixtures.rs` (modified, `#[cfg(test)]`)

```rust
/// `name`'s first event: started at 0 in auto mode with the built-in phases.
pub fn started(name: &str, request: &str) -> Event;
/// A worktree for `name` at /repo/.saaio/worktrees/<name> on flow/<name>, slot 1, run by `agent`.
pub fn workspace(name: &str, agent: &str) -> Workspace;
/// `id` started (named after it), then `events`, folded at `now`.
pub fn flow(id: &str, events: Vec<Event>, now: u64) -> FlowView;
```

`workspace` returns `view::Workspace` (decided in the build), so a caller overrides one field with struct update and turns it into an event with `.into()`.

Users:

- `server/watch.rs` `started()`: deleted, callers use `fixtures::started`.
- `store.rs` `started()`, `fold/tests.rs` `started(at)`: one line over `fixtures::started` (13 and 47 callers).
- `deliver.rs` `started(store)`, `server/tests.rs` `started_flow`: kept (they create the log), events from `fixtures`.
- `wake.rs` `flow`, `answered`; `server/startup.rs`, `pause.rs`, `server/reconcile.rs` `flow`, `unstarted`; `ui/theme.rs` `flow`: built on `fixtures`.
- `cli/output/fixtures.rs`: `Started` from `fixtures::started`.
- Left as they are: `forge.rs` `flow_of` (asserts on its `/work/shop` path, varies the mode), `ui/board/fixtures.rs` (varies the mode, its `/shop` path is in the goldens), `view.rs` `flow` (pins the `--json` contract).

### Inline tests moved

```
src/config.rs              -> src/config/tests.rs
src/adapter/codex.rs       -> src/adapter/codex/tests.rs
src/adapter/claude_code.rs -> src/adapter/claude_code/tests.rs
```

## Proof

- Test count unchanged plus the guard.
- Outside test files, only `mod tests` lines and `#[cfg(test)]` code change; no golden changes.
- fmt, clippy `--all-targets -D warnings`, test, deny pass.
- CLAUDE.md (test rule: a sleep only to assert that nothing happens; `tests/cli/`; `fixtures.rs`) and CONTRIBUTING.md updated.
- Disk: one `target/` in the worktree, scratch homes removed, no stress runs.

## Assumptions

1. Subjects as listed; an unclear test goes where its section in `cli.rs` sits today.
2. `fixtures::started` takes the request, since server tests assert on `"x"`.
3. Incidental request text, mode or `at` may change where no assertion reads it.
4. `view.rs`'s literal `FlowView` stays.
5. One commit per finding.

## Tasks

- [x] Replace the three guess-sleeps and the hand-rolled poll in `src/server/tests.rs`; amend CLAUDE.md's test rule
- [x] Add `fixtures::started`, `fixtures::workspace`, `fixtures::flow` and move every listed builder onto them
- [x] Move the inline tests of `config.rs`, `adapter/codex.rs`, `adapter/claude_code.rs` to sibling `tests.rs`
- [x] Split `tests/cli.rs` into `tests/cli/` with `support.rs`, the subject modules and `every_file_in_tests_cli_is_a_module`
- [x] Update CLAUDE.md and CONTRIBUTING.md for `tests/cli/`, `fixtures.rs` and the new `tests.rs` files
- [x] End to end: full fmt, clippy, test, deny; test count before vs after; diff outside tests limited to `mod tests` lines; disk left clean
- [x] Review item: in `presses_esc_once_into_each_dialog_the_master_answered` wait for the board to show the flow `Idle` instead of sleeping; in `a_running_worker_keeps_the_adapter_it_started_with` open a second worker under the edited config and wait for it (the server reloads config right before it opens panes). The sleeps across the unreadable log stay: the failed read they cover (30650e1) is internal to the dialog check and observable from nowhere, so they assert that no second Esc happens

## Review

Reviewed in an isolated context against this plan, CLAUDE.md and CONTRIBUTING.md, at 55ab877.

**Fixed**

- The plan's `tests/cli/` tree left out `completions.rs`, which the split created for the four completions tests (plan only).

**Verified, no finding**

- `tests/cli/`: the same 101 test functions as `tests/cli.rs` plus the guard; a line-by-line comparison shows only `use` lines, `pub` on support items and `../../` fixture paths changed. Every support item is used by more than one subject or by another support helper.
- The three moved `tests.rs` files hold every line of the old inline tests (only rustfmt re-wraps); production files change by `mod tests;` alone; `fixtures` stays `#[cfg(test)]` in `lib.rs`.
- The replaced waits are sound: `an_exited_idle_worker_keeps_its_state` waits for `Exited`, and the server appends any `Blocked` in `notice_exited_worker` before `send_panes`, so the fold after the wait sees it; the fresh-command marker files fail the test after 30 s if the retry never runs.
- Builders whose mode, `at`, id or request changed (`store.rs`, `wake.rs` `answered`, `reconcile.rs` `unstarted`, `startup.rs`, `pause.rs`, `ui/theme.rs`) have no code under test that reads the changed field.

**Open** (decided by the builder, under the request's own item (1); see the last task)

- CLAUDE.md and CONTRIBUTING.md now say a bounded sleep only asserts that nothing happens, but three pre-existing sleeps in `src/server/tests.rs` let something happen instead: the unreadable-log window and the sleep after `Idle` in the dialog test (around :1356 and :1369), and `a_running_worker_keeps_the_adapter_it_started_with` (around :1424), where the sleep stands in for the config reload. If the reload has not happened yet, the last one still passes, because `deliver_news` runs before `refresh` reloads the config, so under load it can pass without testing anything. None of them can fail without cause. Recommendation: a follow-up giving each an observable anchor (for the config, a second worker opened under the new file, as `opens_the_next_worker_with_the_config_saved_while_it_runs` does), or loosen the sentence until then.

**Where to look**

1. `src/server/tests.rs` diff: the four replaced waits, the only changes that alter what a test observes.
2. `src/fixtures.rs` and its users: the defaults (`at` 0, auto mode, request "x", `/repo/.saaio/worktrees/<name>`) that the builders now inherit.
3. `tests/cli/main.rs`: the guard.

Routine, skim: the `tests/cli/*.rs` subject files and the three moved `tests.rs` files (verbatim moves), and the CLAUDE.md and CONTRIBUTING.md wording.

**Checks**

fmt, clippy `--all-targets -D warnings`, test (672 unit, 102 cli, 7 install) and deny passed at 55ab877; the review changed only this plan, so they were not run again. The plan fix was not reviewed independently; it is documentation only.
