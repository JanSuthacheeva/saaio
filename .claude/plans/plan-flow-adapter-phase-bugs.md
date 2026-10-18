# Adapter permission and phase corrections

Approved approach: defer Codex permission waiting for three seconds. A completed tool cancels the pending request; a real prompt still becomes waiting. A slow automatic review can cross the threshold because Codex's hook payload does not identify who handles approval. Keep an opened or merged merge request in the workflow's last phase. Codex skill file reads are preparation, even when each skill is read in a separate command; workers report the phase when they begin a skill.

## Units

```rust
// src/event.rs: private log event, backward compatible with existing logs
EventKind::PermissionPending { question: String }

// src/fold.rs
Folding::apply(&mut self, event: &Event);
Folding::finish(self, now: u64) -> FlowView;
changes(id: &str, events: &[Event], kind: &EventKind, now: u64) -> bool;

// src/adapter/codex.rs
translate(payload: &Value, workflow: &Workflow) -> Vec<EventKind>;
```

## Call chains

1. Codex `PermissionRequest` -> `translate` -> `PermissionPending` -> hook `report` -> `fold::changes` -> log. `Folding::finish` shows waiting once pending for three seconds.
2. Codex `PostToolUse` -> `Working` -> `fold::changes` records the resolution even if the waiting delay has not expired -> `Folding::apply` clears pending.
3. Worker command or forge check -> `MergeRequest` or `Merged` -> `Folding::open_merge_request` enters the last phase; later phase events cannot move it back while the MR remains open or merged.
4. Codex explicit `$skill` prompt or worker `saaio report phase` -> phase event. Reading `SKILL.md` never produces a phase event.

## Tests and assumptions

Recorded hook fixtures cover Codex permission pending and completion. A regression fixture reconstructed from the master's observed timeline covers separate read-ahead commands. Fold tests cover the delay, cancellation, MR state and custom workflows. Keep the five-second server refold, so a prompt appears as waiting between three and eight seconds after the request. No new dependency, CLI flag or environment variable.

## Tasks

- [x] Add Codex pending permission translation and fold delay/cancellation with fixture and fold tests.
- [x] Keep opened and merged MRs in the workflow's final phase with fold tests for both adapters' common event path and custom phases.
- [x] Ignore Codex multi-phase skill reads and tell workers to report the active phase, with adapter and prompt tests.
- [x] Update plain reference docs and run the end-to-end check using scratch `SAAIO_HOME`; run the full suite and `cargo clippy --all-targets -- -D warnings`.
- [x] Ignore all Codex skill file reads as phase evidence, including separate commands; add the observed-sequence fixture, update reference docs, and rerun the targeted and full checks.

## Review

- Fixed: none. The isolated review found no reproducible defect in the branch.
- Open: none. The documented three-second grace period can briefly show `waiting` during a slow automatic Codex review; a subsequent tool completion clears it.
- Where to look: `src/adapter/codex.rs` `translate()` for read-ahead and permission events; `src/fold.rs` `changes()` and `Folding::finish()` for permission resolution; `Folding::apply()` and `open_merge_request()` for late events after an MR opens. The reference docs are routine to skim.
- Checks: full `cargo test` (514 unit and 71 integration), `cargo fmt --check`, and `cargo clippy --all-targets -- -D warnings` passed at `b47fcde`. The focused separate-command regression test also passed with scratch `SAAIO_HOME` during this review. Only this plan changed afterward.
