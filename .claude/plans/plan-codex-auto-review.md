# Codex auto-review is not waiting

Design review: https://claude.ai/artifact/BVSxfettQusX8tYzs9UxvZ (approaches: https://claude.ai/artifact/DacSUou6CiF38SomAT1XAb)

## Decision

A Codex worker under `--approve-for-me` showed `waiting permission: ...` whenever Codex's automatic
reviewer took longer than the 3 s grace, and woke the master, though nobody had to answer.

On a Codex `PermissionRequest`, `saaio hook` reads the rollout the payload names (`transcript_path`)
and the Codex adapter reports `PermissionPending` only when that turn's approvals go to the user.
A turn whose `turn_context.approvals_reviewer` is Codex's automatic reviewer reports nothing; the
flow stays `working`. Unreadable rollout, missing turn or unknown value: the user, as today.

Facts from Codex 0.163 (verified): hook order is PreToolUse, PermissionRequest, then PostToolUse
after the reviewer allows; the PermissionRequest payload is identical for user and reviewer
(`permission_mode: "default"`); the reviewer is an internal subagent firing no hooks, answering
`allow` or `deny` only; the rollout's `turn_context` (written at turn start, before the turn's
first hook) carries `turn_id` and `approvals_reviewer` (`"auto_review"` or `"user"`).

## Units

`src/adapter.rs` (modified)

```rust
/// Who answers a permission request a hook reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Approver {
    /// The user, in the pane.
    User,
    /// A reviewer the harness runs, which answers without the user.
    Reviewer,
}

impl Adapter {
    /// Codex: codex::approver; Claude Code: User without calling `read`.
    pub fn approver(self, payload: &Value, read: impl FnOnce(&Path) -> Option<String>) -> Approver;
    /// Passes `approver` to codex::translate; Claude Code ignores it.
    pub fn translate(self, payload: &Value, workflow: &Workflow, approver: Approver) -> Vec<EventKind>;
}
```

`src/adapter/codex.rs` (modified)

```rust
/// Reviewer when the rollout's turn_context for the payload's turn_id names the automatic
/// reviewer; User otherwise. Calls `read` only for a PermissionRequest with transcript_path and turn_id.
pub fn approver(payload: &Value, read: impl FnOnce(&Path) -> Option<String>) -> Approver;
/// The approvals_reviewer of the last turn_context line for `turn` in a rollout.
fn approvals_reviewer(rollout: &str, turn: &str) -> Option<String>;
/// The approvals_reviewer values that mean Codex's automatic reviewer.
const REVIEWERS: [&str; 2] = ["auto_review", "guardian_subagent"];
pub fn translate(payload: &Value, workflow: &Workflow, approver: Approver) -> Vec<EventKind>;
//   "PermissionRequest" if !tool.is_empty() && approver == Approver::User => PermissionPending
```

`src/cli/commands/hook.rs` (modified)

```rust
// in hook():
let approver = adapter.approver(payload, |file| read_rollout(file, log));
let kinds = adapter.translate(payload, &config.workflow, approver);

/// A rollout's text; a read that fails goes to server.log and counts as none.
fn read_rollout(file: &Path, log: &Log) -> Option<String>;
```

Fixtures (new): `src/adapter/fixtures/codex/permission_bash_reviewed.json` (the recorded
PermissionRequest under `--approve-for-me`, paths generic) and
`src/adapter/fixtures/codex/rollout_auto_review.jsonl` (trimmed recorded rollout: session_meta, an
earlier turn's turn_context with `"user"`, the fixture turn's turn_context with `"auto_review"`).

## Data

`adapter::Approver` crosses from `Adapter::approver` to `Adapter::translate` within one
`saaio hook` run. Nothing stored changes: flow log, `EventKind`, `FlowView`, `--json` stay as they are.

## Call chains

Permission request under auto-review:

1. `hook::run -> hook::hook(store, config, Adapter::Codex, hooked, payload, log, now)`
2. `hook -> Adapter::approver(payload, |file| read_rollout(file, log)): Approver`
3. `Adapter::approver -> codex::approver(payload, read): Approver`
4. `codex::approver -> read(Path::new(transcript_path)): Option<String>`
5. `codex::approver -> approvals_reviewer(&text, turn_id): Option<String>` = `"auto_review"` -> `Reviewer`
6. `hook -> Adapter::translate(payload, &workflow, Reviewer): Vec<EventKind>` = `[Session]`
7. `hook -> report(...)`: nothing changes, nothing appended; flow stays working, master not woken.

Error paths, each -> `Approver::User` -> `PermissionPending` as today:
- no `transcript_path` or `turn_id`: `read` not called
- rollout unreadable: `read_rollout` logs `could not read the rollout <path>: <error>` to server.log
- no turn_context for the turn, non-JSON lines, unknown reviewer value: no log

Manual approval: step 5 finds `"user"` -> `User` -> `[Session, PermissionPending]` -> waiting after 3 s.
Any other event: `User` without reading; translate as today.

## Test seams

- `codex::approver` unit with a fake `read`: auto_review -> Reviewer; user -> User; earlier turn
  auto_review but this turn missing -> User; read fails -> User; non-permission event never calls read.
- `codex::translate` unit: `permission_bash` with Reviewer -> `[Session]`, with User -> pending.
- `tests/cli.rs`: `saaio hook codex` with a PermissionRequest pointing at a temp rollout: auto_review
  appends no `permission-pending`, user does.

## Assumptions

- Under auto-review Codex never waits for the user on a permission request (allow or deny; a denial
  goes back to the model).
- Nothing is shown while the reviewer works; denials are not reported.
- `"auto_review"` and `"guardian_subagent"` mean the reviewer; anything else the user.
- The whole rollout is read once per Codex permission request.
- Unreadable rollout is logged; a rollout without the turn is not.
- No CHANGELOG entry (0.1.0 not released yet).

## Tasks

- [ ] Add `Approver`, `Adapter::approver` and the `approver` argument of `Adapter::translate` and `codex::translate` (PermissionPending only for `User`); existing tests pass `Approver::User`; unit test that `permission_bash` with `Reviewer` gives only the session
- [ ] Add `codex::approver`, `approvals_reviewer`, `REVIEWERS` and the fixtures `permission_bash_reviewed.json` and `rollout_auto_review.jsonl`, with unit tests for reviewer, user, missing turn, failed read and a non-permission event
- [ ] Wire `hook()` to `Adapter::approver` with `read_rollout` logging failed reads; `tests/cli.rs` test that an auto-review PermissionRequest appends no `permission-pending` and a user one does
- [ ] Update `docs/flows.md` (Codex table row and the automatic review bullet) and `docs/troubleshooting.md` (the auto-review row)
- [ ] End to end: one small Codex run with `--approve-for-me` on a scratch `SAAIO_HOME` and scratch `CODEX_HOME`; an escalated `git commit` leaves no `permission-pending` in the log and the flow never shows waiting
