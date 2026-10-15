# Harness adapters: a worker reports through Claude Code's own hooks

Implementation page (review surface): `docs/design/adapters-implementation.html`; approaches: `docs/design/adapters-approaches.html`.

## Decision

- Approach A: flow ui launches a Claude Code worker with `--settings '<json>'` hooks that run
  `<flow exe> hook claude-code` on each event. `flow hook` reads the hook JSON from stdin and `FLOW_ID`,
  translates it with a pure adapter, and appends only events that change the flow. No model decides
  anything; the skills stop knowing about flow; the flows mod is removed.
- Claude Code only now. `Adapter` is an enum so Codex or pi become one more variant behind the same command.
- Verified on Claude Code 2.1.299 (recorded payloads): `FLOW_ID` reaches hooks; inline `--settings` JSON
  works and merges with the user's hooks; `--settings` must be passed again on `--resume`; Skill input is
  `{"skill": name}`; the subagent tool is `Agent`, subagent events carry `agent_id`; AskUserQuestion raises
  a PermissionRequest too; Stop carries `last_assistant_message`; `/exit` is SessionEnd
  `prompt_input_exit`, a hangup is `other`.
- Answers: the built-in phase map is design-code, implement-plan, review-changes, write-merge-request;
  retiring the mod's reporting and the skills' report blocks are the last tasks of this plan.

## Units

```rust
// src/adapter/mod.rs (new, pure)
pub const REPORT_TEXT_LIMIT: usize = 200;
pub enum Adapter { ClaudeCode }
impl Adapter {
    pub fn named(name: &str) -> Option<Adapter>;          // "claude-code"
    pub fn name(self) -> &'static str;
    /// Arguments inserted right after the program in the agent's argv, fresh and resumed.
    pub fn wiring(self, flow_exe: &Path) -> Vec<String>;
    /// The flow events one hook payload means; empty for one that means nothing.
    pub fn translate(self, payload: &Value, phases: &PhaseMap) -> Vec<EventKind>;
}
/// "/design-code" and "plugin:design-code" both look up "design-code".
pub fn phase_of(skill: &str, phases: &PhaseMap) -> Option<Phase>;
pub fn cut(text: &str) -> String;

// src/adapter/claude_code.rs (new, pure)
/// --settings JSON: UserPromptSubmit, PreToolUse, PostToolUse, PermissionRequest, Stop, SessionEnd each run
/// [flow_exe, "hook", "claude-code"] (exec form), synchronous, timeout 10 s.
pub fn settings(flow_exe: &Path) -> String;
pub fn translate(payload: &Value, phases: &PhaseMap) -> Vec<EventKind>;
fn waiting_question(answer: &str) -> Option<String>;
fn task_counts(result: &str) -> Option<(u32, u32)>;
fn created_merge_request(command: &str, stdout: &str) -> Option<String>;
fn typed_skill(prompt: &str) -> Option<&str>;
fn tool_call(name: &str, input: &Value) -> String;

// src/main.rs
enum Command { /* ... */ #[command(hide = true)] Hook { adapter: String } }
/// Never prints to stdout, always exits 0; problems go to stderr.
fn hook(store: &Store, config: &Config, adapter: &str, flow: Option<String>, payload: &str, now: u64) -> Document;
// server(): LaunchEnv { flow_home, flow_exe: env::current_exe() }

// src/config.rs
pub struct Agent { command, prompt, resume, #[serde(default)] pub adapter: Option<String> }
pub type PhaseMap = BTreeMap<String, Phase>;
pub struct Config { agent, agents, #[serde(default)] pub phases: PhaseMap }   // builtin_phases() under the file's
pub fn builtin_phases() -> PhaseMap;
// builtin claude and master: adapter = Some("claude-code")

// src/launch.rs
pub struct LaunchEnv { pub flow_home: Option<PathBuf>, pub flow_exe: PathBuf }
/// The agent's argv with the adapter's wiring after the program; None also for an unknown adapter.
pub fn worker_spec(flow: &FlowView, agent: &Agent, env: &LaunchEnv) -> Option<PaneSpec>;

// src/fold.rs
/// True when appending `kind` changes state, phase, blocker, delegate, tasks, review or merge request.
pub fn changes(id: &str, events: &[Event], kind: &EventKind, now: u64) -> bool;

// src/output.rs
pub enum Document { Toon(String), Json(String), Text(String), Silent }

// src/server/reconcile.rs: failed reason "agent X uses adapter Y, which flow does not know"
```

## Translation rules (claude_code::translate)

Every event with `agent_id` (a subagent's) means nothing. Otherwise:

- UserPromptSubmit -> working; plus phase when the prompt is a typed `/skill` in the map
- PreToolUse Skill -> phase of `tool_input.skill`
- PreToolUse Agent -> delegated `tool_input.description`, unless `run_in_background` is true
- PreToolUse AskUserQuestion -> waiting `questions[0].question`
- PostToolUse (any tool) -> working, then for plan-progress (`mcp__plan-progress__report`): tasks from
  "ok, N/M done" / "ok, all N done", or blocked "item: note" when status is blocked; for Bash: merge-request
  when `gh pr create` / `glab mr create` printed an http(s) URL
- PermissionRequest -> waiting `permission: Name(target)`; nothing for AskUserQuestion
- Stop with `stop_hook_active` false -> waiting `waiting_question(last_assistant_message)` or "turn ended"
- SessionEnd `prompt_input_exit` -> ended; other reasons nothing
- Texts cut to 200 chars.

## Call chains

1. Open a worker: `Server::open_worker -> launch::worker_spec(flow, agent, env) -> Adapter::named(agent.adapter)
   -> Adapter::wiring(env.flow_exe)` inserted after argv[0] -> `Host::open(spec)`.
   - Unknown adapter -> `reconcile` `Wanted.failed` -> `Host::fail(key, "agent X uses adapter Y, ...")`.
2. A hook event: Claude Code runs `flow hook claude-code` (stdin JSON, FLOW_ID inherited) ->
   `main::hook -> Adapter::translate(payload, &config.phases)` -> `fold::resolve(flows, id)` -> per kind
   `Store::events(id) -> fold::changes(id, events, kind, now)` -> `Store::append` when true -> `Document::Silent`.
   - No FLOW_ID -> nothing. Unknown adapter, bad JSON, no such flow, failed append -> one stderr line,
     nothing appended, exit 0.

## Test seams

- `claude_code::translate`: table tests over recorded payloads in `src/adapter/fixtures/claude-code/`.
- helpers: ported mod cases (question, task counts, merge request, typed skill, phase_of).
- `settings`/`wiring`/`worker_spec`: JSON parses, every event runs the exe; wiring after the program, fresh and resumed.
- `fold::changes`: repeats false, changes true.
- `tests/cli.rs`: `flow hook` writes the event, prints nothing, exits 0 without FLOW_ID and with garbage.

## Assumptions

1. Only workers are wired (the master has no FLOW_ID).
2. Hooks synchronous for order; timeout 10 s.
3. No heartbeat for hooked workers; the server sees liveness. `flow report heartbeat` stays.
4. A permission wait is `Waiting` with text `permission: Name(target)`; no new event kind.
5. Any main-agent PostToolUse means working; `fold::changes` keeps the log small.
6. Workers started earlier get the wiring on their next resume.
7. `flow report` stays.
8. Unknown events and fields are ignored, never an error.

## Tasks

- [x] Add adapter/mod.rs (Adapter, phase_of, cut) and adapter/claude_code.rs (settings, translate and helpers) with fixture-based table tests
- [x] Add config Agent.adapter, Config.phases with builtin_phases (merged under the file's), builtin claude and master on claude-code; tests
- [x] Add fold::changes and output::Document::Silent with tests
- [x] Add the hidden `flow hook <adapter>` command (main::hook) with tests/cli.rs cases: appends, silent, exit 0 without FLOW_ID, with garbage, for an unknown flow
- [x] LaunchEnv.flow_exe; worker_spec inserts the adapter's wiring, fresh and resumed; reconcile fails an unknown adapter; tests
- [x] Docs: README (adapters, [phases], adapter key), CLAUDE.md module map, HANDOVER (piece 4 replaced)
- [x] End-to-end check: a real Claude worker in flow ui with the flows mod's reporting off shows phase, delegated, the question, a permission wait and ended
- [x] Remove the "report to flow" blocks from the skills in ~/projects/agent-skills (design-code, implement-plan, review-changes, write-merge-request, ship-feature)
- [x] Remove the flows mod from ~/projects/claude-code-mods and uninstall it from Claude Code
