# Reach workers after a PR opens

Implementation review: [Lavish page](../../.lavish/reach-after-pr-implementation.html)

## Decision

Approach A, approved by the user: a PR opens the final workflow milestone but does not end the worker. `Done` describes the instant a PR is opened and may transition to `Waiting`, `Working`, `Idle`, or `Blocked`. `Ended` alone means the session finished. Keep the PR URL and merge status separate from worker state. The JSON view has no new fields.

## Units

`src/view.rs` — `FlowView`:
```rust
pub fn is_finished(&self) -> bool;
```
Return true only for `State::Ended`. Existing consumers of this shared predicate therefore retain a live PR-open flow's slot, pane eligibility, worker delivery, typed lines, and master news.

`src/fold.rs` — `Folding`:
```rust
fn apply(&mut self, event: &Event);
fn open_merge_request(&mut self, url: &str, at: u64);
fn finish(self, now: u64) -> FlowView;
```
Let worker state events after PR update state; ignore phase regressions after a PR; ignore worker state events after Ended. `Ended` always sets `State::Ended`. A forge PR discovery or merge after Ended updates PR metadata without reviving the session. A merge of an already open PR preserves a current Waiting/Working/Idle/Blocked state. PR opening completes every workflow step, including while the worker later waits. The final phase duration stops when the PR opens; Ended remains terminal for the session. A closed PR removes its URL and leaves an ended session Ended.

`tests/cli.rs` — real binary integration:
```rust
fn sends_after_pr_until_worker_ends();
```
Use recorded adapter hook fixture payloads with a scratch `SAAIO_HOME`; prove both `saaio send` and `saaio send --type`, JSON waiting state, refusal after end and removal.

`src/fold/tests.rs` — lifecycle regression tests:
```rust
fn a_question_after_pr_is_waiting();
fn ended_after_pr_is_finished();
fn forge_update_does_not_revive_ended_worker();
```
Also cover slot, pane, and master news gates with focused existing-module tests where needed.

## Data

Existing `EventKind::MergeRequest`, `Waiting`, `Ended`, `Merged`, `MergeRequestClosed` feed the fold. Existing `FlowView.state`, `merge_request`, `merged`, and `steps` cross the JSON/UI boundary. No event or view schema migration.

## Call chains

1. Adapter hook -> `commands::hook::run` -> `Store::append`: record `Waiting` after a recorded PR creation hook.
2. Store -> `fold::fold(id, events, beat, now): Option<FlowView>` -> `Folding::apply`: retain URL, expose `Waiting` and question.
3. `wake::changed_flows` -> master news: report the waiting flow.
4. Master -> `send::run` -> `Store::append`: accept message or typed line while worker exists. Worker delivery or server typing consumes it.
5. Session end hook -> `Folding::apply(Ended)` -> `FlowView::is_finished`: reject later sends, free slot, stop delivery, prevent reopening.
6. Forge watcher -> `Folding::apply(Merged or MergeRequestClosed)`: update PR facts; keep Ended state after session end.
7. Missing/removed flow -> `fold::resolve` returns none -> `send::run` returns no-flow failure.

## Test seams

- Fold tests feed event sequences directly and check state, blocker, phase, steps, duration, merge data and changed time.
- CLI tests feed recorded Codex or Claude hook JSON through the real binary in a temporary store.
- Slots, pane reconciliation and master news tests use folded views or store fixtures and verify their existing predicates treat PR-open workers as live.

## Assumptions

- A live flow with a PR retains its slot and can reopen its pane.
- A merged PR does not end a worker session.
- Late phase hooks do not move a PR-open flow away from its final phase.
- The final workflow phase remains complete while the worker asks a post-PR question.
- No new public command, flag, field, or event is required.

## Tasks

- [x] Change `FlowView::is_finished` and `Folding::{apply,open_merge_request,finish}` with lifecycle tests in `src/fold/tests.rs`.
- [x] Add recorded-fixture CLI coverage for `send` and `send --type` after PR, and focused slot, pane and master-news regression coverage; update user-facing docs for the new lifecycle.
- [x] Run end-to-end checks using a scratch `SAAIO_HOME`, `cargo test`, `cargo fmt --check`, and `cargo clippy --all-targets -- -D warnings`.
