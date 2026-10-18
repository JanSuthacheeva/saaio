# Plan: PR closure after an ended flow

Implementation review: [slot-reclaim-implementation.html](../../.lavish/slot-reclaim-implementation.html) (local review page, intentionally uncommitted).

## Decision

An `Ended` event remains decisive even when a later open PR temporarily shows the flow as `Done`. When that PR closes, the fold clears the PR and records `PR #n closed`, but restores `Ended`. A flow with no `Ended` event retains the existing `Done` to `Idle` transition. A finished flow cannot be manually reopened because its slot may have been claimed by another flow. Work that must continue starts a new flow. No new event, stored field, or JSON field is needed.

## Units

`src/fold.rs`: add private memory of the existing `Ended` event to `Folding`; modify `new` and `apply`.

```rust
struct Folding {
    // existing fields
    ended: bool,
}

impl Folding {
    fn new(
        id: &str,
        name: &str,
        request: &str,
        mode: Mode,
        phases: Option<&[Phase]>,
        at: u64,
    ) -> Folding;

    fn apply(&mut self, event: &Event);
}
```

`Folding::new` starts `ended` as false. `Folding::apply` sets it for `Ended`, including an `Ended` event that leaves public state `Done` while a PR is open. On a matching unmerged `MergeRequestClosed`, it clears `merge_request`, appends the existing closure note, and restores `Ended` when `ended` is true; otherwise it restores `Idle` and clears `stopped_at` as today. It keeps `stopped_at` for an ended flow. The existing state change tracking updates `changed_at` when `Done` becomes `Ended` or `Idle`.

`src/server/panes.rs`: guard both entry paths that can launch a worker outside automatic reconciliation.

```rust
impl Server {
    pub(super) fn reopen_worker(&mut self, id: &str);
    pub(super) fn open_fresh(&mut self, key: &PaneKey);
}
```

`reopen_worker` returns for an already running pane as today. For a finished flow whose pane is not running, it calls the existing `Host::fail` with `finished flow cannot reopen; start a new flow` before `pane::worker_spec` or `open_worker`. It appends no flow event. `open_fresh` returns without launching when its flow has become finished since the resume attempt. Unfinished flows retain their existing behavior.

Review decision: both paths must fold the current log before launching. The server's cached board may be up to 500 ms old. Add a private `Server::current_flow` to read `Store::events` and fold at the current clock; a read or fold failure must never launch a worker.

```rust
impl Server {
    fn current_flow(&self, id: &str) -> anyhow::Result<FlowView>;
}
```

Existing units used unchanged:

```rust
impl FlowView {
    pub fn is_finished(&self) -> bool;
}

impl Host {
    pub fn fail(&mut self, key: PaneKey, reason: String);
}

pub fn news(flow: &FlowView, found: Option<&Found>) -> Option<EventKind>;

pub fn reconcile(
    flows: &[FlowView],
    open: &[PaneKey],
    known: &HashSet<String>,
    config: &Config,
    env: &LaunchEnv,
) -> Wanted;
```

## Data

- Existing log events: `Ended` and `MergeRequestClosed { url: String }`. No migration.
- New private `Folding::ended: bool`. It does not cross the serialization boundary.
- Existing public `FlowView` fields: `state`, `merge_request`, `timeline`, `workspace`. After closure on an ended flow, state is `ended`, merge request is null, and the last timeline text is `PR #n closed`. `VIEW_VERSION` stays 3.
- A finished flow's Workspace slot is historical. `slots::taken_slots` excludes finished flows; no slot claim is made for this closed PR.

## Call chains

### PR closes after Ended

1. `ForgeWatch::due(flows, now): Option<Vec<Ask>>` includes the ended unmerged flow.
2. `forge::news(flow, found): Option<EventKind>` returns `MergeRequestClosed` for the PR shown on the flow.
3. `Server::forge_answer(answer): ()` appends the event; if append fails, it logs the error and the view remains unchanged.
4. `fold::fold(id, events, last_beat, now): Option<FlowView>` replays the events via `Folding::apply(event): ()`; `Ended` sets private memory. The closure clears the PR, restores `Ended`, retains the stop time, and adds the closure note.
5. `reconcile(flows, open, known, config, env): Wanted` skips the finished flow, so the old worker is not relaunched with a reclaimed slot.

### PR closes before any Ended event

1. The same forge and fold calls append and replay `MergeRequestClosed`.
2. `Folding::apply(event): ()` finds `ended == false`, clears the PR and sets `Idle`.
3. The existing pane remains or normal reconciliation may open the unfinished flow; its slot was never released.

### User presses Reopen on a finished flow

1. `Server::reopen_worker(id): ()` checks `FlowView::is_finished()` before building a worker spec.
2. `Host::fail(key, reason): ()` shows `finished flow cannot reopen; start a new flow` in a failed pane. No process starts, no event is appended, and the slot claim is untouched.
3. If a pending resumed process fails after its flow became finished, `Server::open_fresh(key): ()` returns before `open_pane`.
4. Both paths use `Server::current_flow(id): anyhow::Result<FlowView>` for the decision, not `self.board`. A read or fold error fails the pane or logs the error and does not launch a worker.

## Test seams

- `src/fold/tests.rs`: `Ended` before PR open then close, PR open then `Ended` then close, and the existing no-`Ended` closure behavior. Check state, PR field, timeline, changed time, and phase duration.
- `src/server/tests.rs`: socket-level Reopen test verifies an unfinished exited worker reopens, and an ended worker gets a failed pane with the reason. Include a done flow and verify the fresh fallback guard if the test fixture can trigger it deterministically.
- End-to-end: with scratch `SAAIO_HOME`, record an ended flow and PR closure; verify `saaio status --json` says `ended` with `PR closed` and that a second flow may claim the old slot without its old worker reopening.

## Assumptions and decisions

- Jan chose approach B: keep a previously ended flow ended and show `PR closed`.
- Jan approved the review recommendation: close the stale-board reopen window by reading the current flow log before either launch path.
- Manual Reopen of any finished flow is refused because its slot may already have been reclaimed. The failed pane gives the user a concrete next step.
- Closure after Ended changes the timeline and `changed_at`, but does not wake the master through an idle state.
- Unfinished flows whose PR closes retain the existing idle behavior.
- The Workspace slot remains visible as history, not an active reservation.
- The `.lavish` pages are never committed.

## Tasks

- [x] Extend `Folding::new` and `Folding::apply` with private `ended` memory; first add fold tests for Ended-before-PR, PR-before-Ended, and no-Ended closure, then make them pass.
- [x] Guard `Server::reopen_worker` and `Server::open_fresh` for finished flows; first update socket-level server tests for unfinished reopen and finished refusal, then make them pass.
- [x] Update `docs/flows.md` and any affected user help to explain closed PR behavior and finished Reopen refusal; verify docs against the changed tests.
- [x] Run a scratch-`SAAIO_HOME` end-to-end reproduction with two flows and a reclaimed slot, then run `cargo test` and `cargo clippy --all-targets -- -D warnings` before review.
- [x] Add `Server::current_flow` and use it in `Server::reopen_worker` and `Server::open_fresh`; first reproduce immediate Ended-before-Reopen in a socket test, then verify both paths refuse stale-board launches and read failures.

## Review

- **Fixed:** none.
- **Open:** `Server::reopen_worker` and `Server::open_fresh` decide from `self.board`, which refreshes every 500 ms. An `Ended` event written after the last refresh can be followed by Reopen before the next refresh, launching the old worker even though its slot is available to another flow. A socket test proved this by appending `Ended` to an exited worker immediately before Reopen: the expected failed pane timed out, while a test expecting a running pane passed. Make both launch guards read the latest flow log (and handle read failure without launching), then add a regression test for this sequence. The temporary test change was removed.
- **Where to look:** `src/server/panes.rs` `reopen_worker` and `open_fresh`, then `src/server.rs` `handle` and `serve`: the stale-board window around manual and fallback launches. `src/fold.rs` `MergeRequestClosed` and `Ended` is the state transition to check; the fold and CLI tests cover the normal path. Documentation changes are routine.
- **Checks:** `cargo test`, `cargo fmt --check`, and `cargo clippy --all-targets -- -D warnings` passed at `e71c798fd2b2171110dfaecfc369c13fb2d2e477`. The focused socket test with the temporary `Ended` event passed when expecting the incorrect reopen; all temporary code changes were removed before this review note.
