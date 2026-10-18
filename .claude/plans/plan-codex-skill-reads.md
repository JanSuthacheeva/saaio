# Codex skill reads start phases

Review page: [Codex skill reads plan](../../.lavish/codex-skill-reads-plan.html). The page is a local review artifact and is not committed.

## Decision

Codex worker `developer_instructions` tell the worker to read a workflow skill's `SKILL.md` only when beginning that skill, never ahead. A `PreToolUse` shell command that reads one configured phase skill emits its phase. A command reading skills from several phases emits no phase. The worker does not use `saaio report phase` for this.

## Units

`src/adapter.rs` modifies `CODEX_PHASE_PROMPT` and keeps `Adapter::wiring` unchanged:

```rust
const CODEX_PHASE_PROMPT: &str;
pub fn Adapter::wiring(self, flow_exe: &Path, folder: &Path) -> Vec<String>;
```

`src/adapter/codex.rs` modifies hook translation and adds a private read detector:

```rust
pub fn translate(payload: &Value, workflow: &Workflow) -> Vec<EventKind>;
fn read_skill_phase(command: &str, workflow: &Workflow) -> Option<Phase>;
```

`src/adapter/calls.rs` may expose its existing shell tokenization to the sibling adapter:

```rust
pub(super) fn simple_commands(line: &str) -> Vec<Vec<String>>;
```

No new event or persistent data structure is needed.

## Call chains

1. `Adapter::wiring(Codex, exe, folder): Vec<String>` -> `Adapter::instruct(CODEX_PHASE_PROMPT): Vec<String>` -> Codex receives `developer_instructions`.
2. Codex reads one `SKILL.md` at skill start -> `codex::translate(PreToolUse, workflow): Vec<EventKind>` -> `read_skill_phase(command, workflow): Option<Phase>` -> existing `EventKind::Phase` and flow fold.
3. Codex reads skills from multiple configured phases in one command -> `read_skill_phase(...): None` -> no phase event.
4. Commands without a matching shell read, subagent hooks, and malformed payloads -> no phase event; existing prompt, work, permission, and turn events remain.

## Test seams

- `adapter.rs` unit test parses Codex wiring and checks the actual worker instruction.
- `codex.rs` tests use recorded `PreToolUse` fixtures for one skill, multiple phase skills in one command, separate commands, and unrelated commands.
- The end-to-end check replays a recorded hook through the CLI using a scratch store, if practical; otherwise a focused integration test of `translate` and fold.

## Assumptions

- The read detector supports normal shell file readers (`cat`, `sed`, `head`, `tail`, `less`, `bat`) and path arguments ending in `SKILL.md`; incidental mentions in `echo` or search patterns do not count.
- The path's parent directory supplies the skill name, matching the existing `Workflow::phase_of` convention.
- A command that reads one configured phase skill and unrelated files starts that phase.

## Tasks

- [x] Replace `CODEX_PHASE_PROMPT` and update `Adapter::Codex::wiring` instruction assertions.
- [ ] Add `read_skill_phase` to `codex.rs`, adapt recorded hook fixtures and tests, and reuse shell tokenization where needed.
- [ ] Update `docs/flows.md` and `docs/agents.md` for experimental instruction-dependent Codex phases.
- [ ] Run an end-to-end recorded hook check, full tests, formatting, and `cargo clippy --all-targets -- -D warnings`.
