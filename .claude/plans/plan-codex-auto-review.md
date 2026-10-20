# Codex auto-review is not waiting

Design review: https://claude.ai/artifact/BVSxfettQusX8tYzs9UxvZ (approaches: https://claude.ai/artifact/DacSUou6CiF38SomAT1XAb)

## Decision

A Codex worker under `--approve-for-me` showed `waiting permission: ...` whenever Codex's automatic
reviewer took longer than the 3 s grace, and woke the master, though nobody had to answer.

On a Codex `PermissionRequest`, `saaio hook` reads the rollout the payload names (`transcript_path`)
back from its end to the newest `turn_context` line
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
    /// Codex: codex::approver; Claude Code: User without calling `last_line`.
    pub fn approver(self, payload: &Value, last_line: impl FnOnce(&Path, &LineMatch) -> Option<String>) -> Approver;
    /// Passes `approver` to codex::translate; Claude Code ignores it.
    pub fn translate(self, payload: &Value, workflow: &Workflow, approver: Approver) -> Vec<EventKind>;
}
```

`src/adapter/codex.rs` (modified)

```rust
/// Reviewer when the rollout's turn_context for the payload's turn_id names the automatic
/// reviewer; User otherwise. Calls `last_line` (newest turn_context line) only for a
/// PermissionRequest with transcript_path and turn_id. Turns do not interleave in a rollout.
pub fn approver(payload: &Value, last_line: impl FnOnce(&Path, &LineMatch) -> Option<String>) -> Approver;
/// The payload of a rollout line that is a turn_context.
fn turn_context(line: &str) -> Option<Value>;
/// The approvals_reviewer of a turn_context line, when it is `turn`'s.
fn approvals_reviewer(line: &str, turn: &str) -> Option<String>;
/// The approvals_reviewer values that mean Codex's automatic reviewer.
const REVIEWERS: [&str; 2] = ["auto_review", "guardian_subagent"];
pub fn translate(payload: &Value, workflow: &Workflow, approver: Approver) -> Vec<EventKind>;
//   "PermissionRequest" if !tool.is_empty() && approver == Approver::User => PermissionPending
```

`src/cli/commands/hook.rs` (modified)

```rust
// in hook():
let approver = adapter.approver(payload, |file, matches| rollout_line(file, matches, log));
let kinds = adapter.translate(payload, &config.workflow, approver);

/// The last line of a rollout that `matches`; a read that fails goes to server.log and counts as none.
fn rollout_line(file: &Path, matches: &LineMatch, log: &Log) -> Option<String>;
/// The last line of `file` that `matches`, read back from the end `chunk` bytes at a time.
fn last_line(file: &Path, matches: &LineMatch, chunk: u64) -> std::io::Result<Option<String>>;
const ROLLOUT_CHUNK: u64 = 64 * 1024;
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
2. `hook -> Adapter::approver(payload, |file, matches| rollout_line(file, matches, log)): Approver`
3. `Adapter::approver -> codex::approver(payload, last_line): Approver`
4. `codex::approver -> last_line(Path::new(transcript_path), turn_context matcher): Option<String>` (newest turn_context)
5. `codex::approver -> approvals_reviewer(&line, turn_id): Option<String>` = `"auto_review"` -> `Reviewer`
6. `hook -> Adapter::translate(payload, &workflow, Reviewer): Vec<EventKind>` = `[Session]`
7. `hook -> report(...)`: nothing changes, nothing appended; flow stays working, master not woken.

Error paths, each -> `Approver::User` -> `PermissionPending` as today:
- no `transcript_path` or `turn_id`: `last_line` not called
- rollout unreadable: `rollout_line` logs `could not read the rollout <path>: <error>` to server.log
- newest turn_context is another turn's, none at all, unknown reviewer value: no log

Manual approval: step 5 finds `"user"` -> `User` -> `[Session, PermissionPending]` -> waiting after 3 s.
Any other event: `User` without reading; translate as today.

## Test seams

- `codex::approver` unit with a fake `last_line` over fixture text: auto_review -> Reviewer; user -> User; earlier turn
  auto_review but this turn missing -> User; read fails -> User; non-permission event never calls it;
  a later line mentioning "turn_context" is not taken for one.
- `hook::last_line` unit: last match across chunk sizes 1, 7, 64 and 1 MiB; no final newline; missing file.
- `codex::translate` unit: `permission_bash` with Reviewer -> `[Session]`, with User -> pending.
- `tests/cli.rs`: `saaio hook codex` with a PermissionRequest pointing at a temp rollout: auto_review
  appends no `permission-pending`, user does.

## Assumptions

- Under auto-review Codex never waits for the user on a permission request (allow or deny; a denial
  goes back to the model).
- Nothing is shown while the reviewer works; denials are not reported.
- `"auto_review"` and `"guardian_subagent"` mean the reviewer; anything else the user.
- A rollout is read back from its end only to the newest turn_context: the running turn's bytes
  (measured on 63 real rollouts: escalations sit a median 163 lines / 380 KB, at most 816 lines /
  1.5 MB, after their turn_context, so a fixed tail of 50-100 lines would miss most).
- Unreadable rollout is logged; a rollout without the turn is not.
- No CHANGELOG entry (0.1.0 not released yet).

## Tasks

- [x] Add `Approver`, the `approver` argument of `Adapter::translate` and `codex::translate` (PermissionPending only for `User`), `codex::approver`, `approvals_reviewer`, `REVIEWERS`, `Adapter::approver`, and wire `hook()` to it with `read_rollout` logging failed reads; fixtures `permission_bash_reviewed.json` and `rollout_auto_review.jsonl`; unit tests for `permission_bash` with `Reviewer`, reviewer, user, missing turn, failed read and a non-permission event (one task: `Reviewer` is dead code, which clippy rejects, until `hook()` produces it)
- [x] `tests/cli.rs` test that an auto-review PermissionRequest appends no `permission-pending` and a user one does
- [x] Update `docs/flows.md` (Codex table row and the automatic review bullet) and `docs/troubleshooting.md` (the auto-review row)
- [x] End to end: one small Codex run with `--approve-for-me` on a scratch `SAAIO_HOME` and scratch `CODEX_HOME`; an escalated `git commit` leaves no `permission-pending` in the log and the flow never shows waiting
- [x] Read the rollout back from its end to the newest `turn_context` (`hook::last_line`, `rollout_line`; `codex::turn_context`), with unit tests across chunk sizes and a later line that mentions turn_context

End-to-end result (2026-10-20, Codex 0.163, `--approve-for-me`, medium effort): flow `verify`
committed `b.txt` after an escalation the automatic reviewer allowed in 2.5 s
(`approvals_reviewer: "auto_review"`); its log has no `permission-pending` (the probe before the
fix logged one per escalation), and `saaio status --json` polled every 0.3 s showed only
`working`, then `idle`.

## Review

Isolated review of `main..flow/codex-auto-approve-waiting`. The diff matches the plan: units,
signatures, call chains, error paths, fixtures and docs. Nothing stored changes.

Fixed:
- No test pinned `REVIEWERS`: dropping `"guardian_subagent"` (or accepting any value) passed the
  suite. Added `only_the_known_reviewer_values_mean_the_reviewer`; it fails with the value removed.
  (`adapter: codex: test which reviewer values mean the reviewer`)
- `fold::PERMISSION_GRACE_MS` still said it exists for slow automatic review; auto-review requests
  no longer reach it. Comment only. (`fold: permission grace no longer about automatic review`)
  Not reviewed independently; a one-line doc comment.

Open:
- The 3 s grace now only delays a Codex permission the user answers (and the unreadable-rollout
  fallback). Keeping it is harmless and still hides requests whose tool completes at once;
  dropping it would show user prompts sooner. Recommendation: keep for 0.1.0 (release-day, bug
  fixes only), revisit after.
- `hook()` asks for the approver before it returns for `Hooked::Nobody`, so a Codex hook with no
  flow or master would still read the rollout. Codex hooks exist only in saaio's launch
  arguments, so this does not happen in practice. Recommendation: leave.
- The whole rollout is read on each Codex permission request; long sessions mean a few MB.
  Acceptable; noted in the plan's assumptions.

Decided (user, 2026-10-20): keep the 3 s grace and the read before the no-flow check; read only
the end of the rollout. Done: read back from the end to the newest turn_context instead of a fixed
50-100 lines, which the measurement above shows would miss most escalations.

Where to look:
1. `src/adapter/codex.rs` `approver` -> `approvals_reviewer`: the turn match and the
   fall-back to the user on anything unknown.
2. `src/cli/commands/hook.rs` `hook`, `rollout_line` and `last_line`: the only I/O, and what server.log says.
3. `docs/flows.md` Codex table and bullet: the user-facing promise.
The test call-site churn of `translate(..., Approver::User)` is routine and can be skimmed.

Checks: `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check` pass at
the review commits.
