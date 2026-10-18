# Adapter permission and phase corrections

Approved approach: defer Codex permission waiting for three seconds. A completed tool cancels the pending request; a real prompt still becomes waiting. A slow automatic review can cross the threshold because Codex's hook payload does not identify who handles approval. Keep an opened or merged merge request in the workflow's last phase. A Codex command reading skills from multiple phases is preparation, not phase evidence; workers report the phase when they begin a skill.

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
read_phase(command: &str, workflow: &Workflow) -> Option<Phase>;
```

## Call chains

1. Codex `PermissionRequest` -> `translate` -> `PermissionPending` -> hook `report` -> `fold::changes` -> log. `Folding::finish` shows waiting once pending for three seconds.
2. Codex `PostToolUse` -> `Working` -> `fold::changes` records the resolution even if the waiting delay has not expired -> `Folding::apply` clears pending.
3. Worker command or forge check -> `MergeRequest` or `Merged` -> `Folding::open_merge_request` enters the last phase; later phase events cannot move it back while the MR remains open or merged.
4. Codex prompt, single phase skill read, or worker `saaio report phase` -> phase event. A command reading multiple phases produces no phase event.

## Tests and assumptions

Recorded hook fixtures cover Codex permission pending and completion and batched skill reads. Fold tests cover the delay, cancellation, MR state and custom workflows. Keep the five-second server refold, so a prompt appears as waiting between three and eight seconds after the request. No new dependency, CLI flag or environment variable.

## Tasks

- [x] Add Codex pending permission translation and fold delay/cancellation with fixture and fold tests.
- [ ] Keep opened and merged MRs in the workflow's final phase with fold tests for both adapters' common event path and custom phases.
- [ ] Ignore Codex multi-phase skill reads and tell workers to report the active phase, with adapter and prompt tests.
- [ ] Update plain reference docs and run the end-to-end check using scratch `SAAIO_HOME`; run the full suite and `cargo clippy --all-targets -- -D warnings`.
