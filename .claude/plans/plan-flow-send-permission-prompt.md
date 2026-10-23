# saaio send at a permission prompt

Review page: https://claude.ai/artifact/BEgqTL4L7P5K2iSqstzbHv (approaches:
https://claude.ai/artifact/4sX5jfQGZBTwaPPnHWN1kn)

## Decision

`saaio send` to a worker waiting on a permission prompt said "saaio wakes its worker with it now",
but the worker reads the message only once the prompt is answered in its pane (reproduced E2E:
delivered within seconds, read only after "1. Yes" in the pane). The adapter that sees the
permission request marks the `Waiting` event with `permission: true`; `fold` carries it into
`FlowView.permission`; `output::flow::sent` prints the open prompt and when the worker reads the
message. `--json` gains `permission` without a `VIEW_VERSION` bump (new keys need none).

## Units

`src/event.rs`

```rust
Waiting {
    question: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    questions: Vec<Question>,
    /// The harness asks permission for a tool call; question names the call.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    permission: bool,
},

impl EventKind {
    /// A question asked in words: no dialog, no permission prompt.
    pub fn waiting(question: impl Into<String>) -> EventKind;
}
```

Sites that build a worded waiting (`questions` empty, not a permission) use `EventKind::waiting`,
in production and tests; dialog sites add `permission: false`; patterns use `..` where they do not.

`src/adapter/claude_code.rs` - the `PermissionRequest` arm builds
`Waiting { question: cut("permission: …"), questions: Vec::new(), permission: true }`.

`src/view.rs` - `FlowView`, after `questions`:

```rust
/// The flow waits on a permission prompt in its pane; blocker names the tool call.
pub permission: bool,
```

`src/fold.rs` - `Folding::new` starts it false; `apply`'s `Waiting` arm sets
`self.view.permission = *permission` (before `stop_for`, which may return early); the cleanup that
clears `questions` outside `Waiting` clears it too; `finish` sets it true for a matured
`PermissionPending`.

`src/cli/output/flow.rs` - `sent(flow: &FlowView) -> Document` keeps its signature; `Sent` gains

```rust
#[serde(skip_serializing_if = "Option::is_none")]
prompt: Option<&'a str>, // flow.blocker when flow.permission
```

and its first help line for a flow on a permission prompt reads
`{name}'s permission prompt is open in its pane. Its worker reads this once the user answers the prompt there`.

## Data

Log line: `{"v":1,"at":…,"kind":"waiting","question":"permission: Bash(curl -sI https://example.org)","permission":true}`;
`permission` left out when false, read false when missing, ignored by older binaries.

`--json`: `"permission": true|false` on every flow, true only for a `waiting` flow on a permission
prompt. `version` stays 3.

TOON:

```
sent: ask
state: waiting
prompt: "permission: Bash(curl -sI https://example.org)"
help[2]: ask's permission prompt is open in its pane. Its worker reads this once the user answers the prompt there,saaio wakes you when ask changes again
```

## Call chains

Claude Code worker asks permission:
1. `Claude Code -> saaio hook claude-code` with the `PermissionRequest` payload.
2. `hook -> Adapter::translate(payload): Vec<EventKind>` -> `Waiting { permission: true, .. }`;
   AskUserQuestion still yields nothing.
3. `hook -> fold::changes`, `Store::append`; errors to server.log as today.

Master sends:
1. `main -> send::run(args, store, env, now): Result<Document, Failure>`; refusals unchanged.
2. `run -> views(store, now)` -> `Folding::apply` sets `permission`; `Folding::finish` sets it for a
   Codex `PermissionPending` past its 3 s grace.
3. `run -> Store::append(id, Message)`; I/O error -> `Failure` as today.
4. `run -> output::flow::sent(flow): Document` with `prompt` and the permission help line.

Codex request inside its grace: flow reads `working`, `sent` prints today's text. An answered
prompt: the next `Working` clears `permission`.

## Test seams

- `event.rs`: a waiting line without `permission` reads false; false is left out when written;
  true round-trips.
- `claude_code/tests.rs`: `permission_bash.json` -> `permission: true`; `permission_ask.json` -> nothing.
- `fold/tests.rs`: set by a permission waiting, not a worded one; cleared by `Working`; matured
  Codex `PermissionPending` sets it, one inside its grace does not.
- `output/flow.rs`: `sent` on a permission prompt, exact text.
- `tests/cli/send.rs`: recorded Claude Code and Codex `PermissionRequest` through `saaio hook`,
  then `saaio send` as master prints `prompt`; `status --json` has `"permission": true`.

## Assumptions (approved)

- The TOON key is `prompt`, holding the blocker as it reads in the timeline.
- `saaio status` TOON and `saaio send --type` output stay as they are.
- A permission logged before the update gets the old text.
- The UI does not use the field yet.
- ROADMAP narrows the item to "the master answers the prompt itself"; CHANGELOG under Unreleased:
  one Changed (send), one Added (`--json` `permission`).
- Docs: `docs/cli.md` send section, `docs/json.md` field table and sample, `docs/flows.md` where it
  says what send does.

## Decisions during review

- Approach A (marker on the event), not parsing the blocker text.
- No `VIEW_VERSION` bump.
- The new field's churn goes through `EventKind::waiting(question)` instead of `permission: false`
  at every worded-question site.

## Tasks

- [x] Add `EventKind::Waiting.permission` and `EventKind::waiting(question)`; move worded-waiting sites to the helper and dialog sites to `permission: false`; event.rs tests for missing, false and true
- [x] Mark Claude Code's `PermissionRequest` waiting with `permission: true`, with claude_code tests on `permission_bash.json` and `permission_ask.json`
- [x] Add `FlowView.permission` and fold it (Waiting, cleanup, matured `PermissionPending`), with fold tests
- [x] `output::flow::sent`: `prompt` and the permission help line, with exact-text tests
- [x] `tests/cli/send.rs`: Claude Code and Codex permission requests through `saaio hook`, then `saaio send` and `status --json`
- [ ] Docs: `docs/cli.md`, `docs/json.md`, `docs/flows.md`; ROADMAP narrowed; CHANGELOG Unreleased
- [ ] E2E: scratch store, real UI, guided Claude Code worker at a permission prompt, master's `saaio send` prints the prompt; answer it in the pane and see the worker read the message
