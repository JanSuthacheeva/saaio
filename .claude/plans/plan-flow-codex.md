# Codex: a second harness for workers and the master

Implementation page (review surface): https://claude.ai/artifact/GyDfWffvXbf51Ff14As68u, copy in
`docs/design/codex-implementation.html`; approaches: https://claude.ai/artifact/3y32yGaMR3cZwCBcYsVGm5,
`docs/design/codex-approaches.html`.

## Decision

- Approach A: `Adapter::Codex` passes flow's hooks to every Codex launch, fresh and resumed, as `-c hooks.<Event>=[...]`
  arguments. You trust them once in Codex's own startup review (`t`), and Codex stores that trust in
  `~/.codex/config.toml` until the hook definition changes. flow writes nothing into `~/.codex`.
- Codex cannot take a session id up front, so flow learns it from the first hook: a `session` event in the
  flow's log for workers, `waits/master.session` for the master. `{session}` fills resume templates.
- A Codex session cannot wake itself (Codex has no asyncRewake), so news reaches it two ways:
  - Busy: its synchronous Stop hook hands the news over (exit 2 with the news on stderr) and Codex goes on.
  - Idle: `flow server` types the news in with a bracketed paste and Enter, but only when the session is not
    busy and no key has gone into that pane for `QUIET` (10 s).
- Codex can be the master: `[master] agent = "codex"` in config.toml. The default stays Claude.
- In scope: a Codex peek (reads the rollout JSONL) and an agent row in the new-flow dialog.
- The skills stay agent-agnostic. A Codex session shows review pages with lavish, styled from
  `~/projects/dotfiles-v2/agents/design-system`.

## Verified Codex facts

Sources: Codex source at `openai/codex@6b991972c5b5` (0.163.0) and a live probe of 0.144.1.

- **Hooks.**
  - Events: `PreToolUse`, `PermissionRequest`, `PostToolUse`, `PreCompact`, `PostCompact`, `SessionStart`,
    `SessionEnd`, `UserPromptSubmit`, `SubagentStart`, `SubagentStop`, `Stop`, `Interrupt`.
  - A command runs as `$SHELL -lc "<command>"` in the session's cwd, with the payload JSON on stdin.
  - Exit 0 with empty stdout means nothing. Plain stdout on SessionStart is injected into the model, so
    `flow hook codex` never prints to stdout. Exit 2 with stderr on Stop makes Codex go on with it.
  - Async hooks' exit 2 and feedback are dropped (`hooks/src/engine/command_runner.rs:157`).
- **Payload.**
  - Every event: `session_id`, `transcript_path`, `cwd`, `hook_event_name`, `model`.
  - Turn events: `turn_id`, `permission_mode`; a subagent's events also carry `agent_id` and `agent_type`.
  - UserPromptSubmit: `prompt`. Pre/PostToolUse: `tool_name`, `tool_input`, `tool_use_id`; Post also
    `tool_response`. PermissionRequest: `tool_name`, `tool_input`. Stop: `stop_hook_active`,
    `last_assistant_message`. SessionEnd: `reason`, always "other".
  - Tool names: `Bash`, `apply_patch`, `mcp__server__tool`, `spawn_agent`, `request_permissions`.
  - SessionStart is deferred to the first turn.
- **Probed on 0.144.1.**
  - `-c 'hooks.UserPromptSubmit=[{hooks=[{type="command",command="..."}]}]'` fires with `session_id` and
    `transcript_path`.
  - Untrusted hooks show the review modal. `t` writes
    `[hooks.state."/<session-flags>/config.toml:<event>:0:0"] trusted_hash = "sha256:..."`.
  - `-c 'projects={"<path>"={trust_level="trusted"}}'` skips the folder prompt. Worktrees inherit the main
    repo's trust.
  - An update prompt appears unless `-c check_for_update_on_startup=false` is passed.
- **Resume.** `codex resume <SESSION_ID>` works from any folder. There is no `--session-id`.
- **Skills.** They live in `~/.agents/skills` or `.agents/skills`, and a `$name` mention in the prompt calls
  one. Implicitly, the model reads `<dir>/<name>/SKILL.md` through Bash.
- **Rollout.** `$CODEX_HOME/sessions/YYYY/MM/DD/rollout-<ts>-<uuid>.jsonl` holds lines of the form
  `{"timestamp","type","payload"}` with types `session_meta`, `response_item`, `event_msg` and others.
  Rollouts older than 7 days may become `.jsonl.zst`.
- **Instructions.** `-c developer_instructions="<toml string>"`.

## Units

```rust
// src/adapter/mod.rs (modified)
pub enum Adapter { ClaudeCode, Codex }
impl Adapter {
    pub fn named(name: &str) -> Option<Adapter>;            // + "codex"
    pub fn name(self) -> &'static str;
    /// Hooks and WORKER_PROMPT (through instruct), inserted after the program.
    pub fn wiring(self, flow_exe: &Path) -> Vec<String>;
    /// Claude: its waiter when `wake` (as today). Codex: its hooks with --master always (they record
    /// its session); hook() hands news over at Stop only when `wake`.
    pub fn master_wiring(self, flow_exe: &Path, wake: bool) -> Vec<String>;
    /// Claude ["--append-system-prompt", text]; Codex ["-c", "developer_instructions=<toml string>"].
    pub fn instruct(self, text: &str) -> Vec<String>;
    /// Claude true (asyncRewake flow wait); Codex false (Stop hand-over and server typing).
    pub fn wakes_itself(self) -> bool;
    pub fn translate(self, payload: &Value, phases: &PhaseMap) -> Vec<EventKind>;
}
/// Moved from claude_code.rs, harness-neutral: "the flow master is the agent session that coordinates
/// the workers ... Its messages reach you as hook feedback or as a prompt that starts with
/// \"message from the flow master:\" ..."
pub const WORKER_PROMPT: &str;
// moved from claude_code.rs unchanged, shared by both adapters:
pub fn turn_end(answer: &str) -> EventKind;        // with plain() and sentences()
pub fn created_merge_request(command: &str, stdout: &str) -> Option<String>;
pub fn tool_call(name: &str, input: &Value, cwd: &str) -> String;

// src/adapter/codex.rs (new, pure)
const EVENTS: [&str; 6] = ["UserPromptSubmit", "PreToolUse", "PostToolUse", "PermissionRequest", "Stop", "Interrupt"];
const HOOK_TIMEOUT_SECS: u64 = 10;
/// ["-c", "check_for_update_on_startup=false"] then per event
/// ["-c", "hooks.<Event>=[{hooks=[{type=\"command\",command=\"'<flow_exe>' hook codex[ --master]\",timeout=10}]}]"].
pub fn hooks(flow_exe: &Path, master: bool) -> Vec<String>;
pub fn translate(payload: &Value, phases: &PhaseMap) -> Vec<EventKind>;
fn session(payload: &Value) -> Option<EventKind>;           // Session { id: session_id, transcript: transcript_path }
fn mentioned_skill(prompt: &str) -> Option<&str>;           // "$design-code fix it" -> "design-code"
fn read_skill(command: &str) -> Option<&str>;               // ".../skills/<name>/SKILL.md" -> name
pub fn toml_string(text: &str) -> String;                   // TOML basic string, escapes " \ and newlines

// src/adapter/claude_code.rs (modified): helpers and WORKER_PROMPT moved out; wiring builds the prompt
// argument through Adapter::instruct. Behaviour unchanged.

// src/deliver.rs (new)
/// Moved from main.rs: Some(None) while there is nothing; None when the flow is finished or gone.
pub fn worker_news(store: &Store, id: &str, now: u64) -> Option<Option<String>>;
/// Flows that stopped since the master last saw them, marked seen; None when none.
pub fn master_news(store: &Store, flows: &[FlowView]) -> Option<String>;
// main::master_news keeps its SETTLE pause and is_mine check around deliver::master_news.

// src/event.rs
EventKind::Session { id: String, transcript: Option<String> }

// src/view.rs
pub struct FlowView { /* ... */ pub session: Option<Session> }
pub struct Session { pub id: String, pub transcript: Option<String> }

// src/fold.rs
// Folding::apply: Session -> view.session (no timeline entry, no state change).
// changes(): Session counts only when id or transcript differs from the last Session.

// src/store.rs
pub fn set_busy(&self, waiter: &Waiter, busy: bool) -> Result<()>;   // waits/<key>.busy
pub fn busy(&self, waiter: &Waiter) -> bool;
pub fn set_master_session(&self, id: &str) -> Result<()>;            // waits/master.session
pub fn master_session(&self) -> Option<String>;

// src/config.rs
pub struct MasterConfig { pub wake: bool, #[serde(default)] pub agent: Option<String> }
impl Config {
    /// [master] agent, else the "master" agent, else the default agent.
    pub fn master_agent(&self) -> Option<(&str, &Agent)>;
    /// Every agent but MASTER_AGENT, the default first.
    pub fn worker_agents(&self) -> Vec<&str>;
}
// builtin_agents(): + "codex": command ["codex", "{prompt}"], prompt "$ship-feature {request}",
//   resume ["codex", "resume", "{session}"], adapter "codex".
// builtin "master": drops "--append-system-prompt-file", "{instructions}" (master.rs instructs).

// src/launch.rs
/// {session} = `session`.
pub fn resume_argv(agent: &Agent, id: &str, name: &str, session: &str) -> Option<Vec<String>>;
/// session = flow.session's id, else the flow id. Must resume, the resume template names {session}
/// and flow.session is None: agent_argv (fresh) instead.
pub fn worker_spec(flow: &FlowView, agent: &Agent, env: &LaunchEnv) -> Option<PaneSpec>;

// src/main.rs
Command::Hook { adapter: String, #[arg(long)] master: bool }
enum Hooked { Flow(String), Master, Nobody }
struct HookOutcome { news: Option<String> }   // Some: news to stderr, exit 2
fn hook(store: &Store, config: &Config, adapter: &str, hooked: Hooked, payload: &str, now: u64) -> HookOutcome;
// ui(): UiEnv { agents: config.worker_agents() as Strings, .. }

// src/server/master.rs
pub struct Instructions { pub path: PathBuf, pub text: String }
impl MasterRecord {
    /// After the program: adapter.instruct(&instructions.text), then adapter.master_wiring(flow_exe, wake).
    /// {instructions} = path; {session} = `session` else self.session. Fresh when it must resume, the
    /// template names {session} and `session` is None.
    pub fn spec(&self, fresh: bool, agent: &Agent, env: &LaunchEnv,
                instructions: &Instructions, wake: bool, session: Option<&str>) -> Option<PaneSpec>;
}
// master.md: harness-neutral wording; the current text joins PREVIOUS_DEFAULTS (master-v3.md).

// src/server/mod.rs
const QUIET: Duration = Duration::from_secs(10);   // injectable in tests
struct Server { /* ... */ typed_at: HashMap<PaneKey, Instant> }   // set on ClientMsg::Input for focus
fn deliver_news(&mut self);                         // every tick, before refresh's early return
fn server_delivered(&self, key: &PaneKey) -> Option<Waiter>;   // adapter of the pane's agent !wakes_itself()
fn paste(text: &str) -> Vec<u8>;                    // "\x1b[200~" text "\x1b[201~" "\r"
// open_master(): config.master_agent(); Instructions from paths.instructions; store.master_session()

// src/ui/peek.rs
pub enum Format { Claude, Codex }
pub fn transcript_of(flow: &FlowView, claude_home: &Path) -> Option<(PathBuf, Format)>;
pub fn tail(file: &Path, max: usize, format: Format) -> Result<Vec<PeekLine>>;
fn parse_codex(text: &str, cut: bool) -> Vec<PeekLine>;
// response_item: message with assistant output_text -> Text; function_call -> Tool; its output -> Result;
// an unanswered last call -> Now. PeekCache::refresh uses transcript_of.

// src/ui/newflow.rs, app.rs, mod.rs
pub struct NewFlowForm { /* ... */ pub agents: Vec<String>, pub agent: usize }
enum Field { Request, Type, Agent, Mode }         // Agent row only with two or more agents
pub fn new(repo: PathBuf, agents: Vec<String>) -> NewFlowForm;
pub fn picked_agent(&self) -> Option<&str>;       // None for the default (first)
// app.rs Job::Start::args: + ["--agent", name] when picked_agent(); mod.rs UiEnv { agents, .. }
```

## Translation rules (codex::translate)

Every payload with a `session_id` also yields `Session { id, transcript }` first; fold::changes drops repeats.
A subagent's events (`agent_id` set) yield only a phase, from a skill it reads. Otherwise:

- **UserPromptSubmit:** `working`. Also `phase` when the prompt's first `$name` is in the phase map.
- **PreToolUse:**
  - `Bash` whose command reads `.../skills/<name>/SKILL.md`: the phase of `name`.
  - `spawn_agent`: `delegated` with its description; take the field name from the recorded fixture.
- **PostToolUse:** `working`. For `Bash`, also `merge-request` when `gh pr create` or `glab mr create`
  printed a URL. Take the output field from the fixture.
- **PermissionRequest:** `waiting` with `permission: Name(target)`, using `tool_call`.
- **Stop:** `answer` with `last_assistant_message`, then `turn_end(answer)`.
- **Interrupt:** `idle` with "interrupted".
- Unknown events and fields mean nothing. Texts are cut to 200 chars.

## Call chains

1. **Start a Codex worker.**
   - `App -> Job::Start::args()` gives `[.., "--agent", "codex"]`, then `main::start` writes the Workspace event.
   - `Server::refresh -> reconcile -> launch::worker_spec(flow, agent, env)`.
   - `worker_spec -> Adapter::Codex.wiring(flow_exe)` is spliced in after "codex", then `Server::open_worker -> Host::open(spec)`.
   - Host::open fails: the log gets `Blocked "could not start the agent: .."`.
   - First run: Codex's hooks review shows in the pane, and nothing is reported until you press `t`.
2. **A Codex hook event.**
   - `codex -> sh -lc "'<flow>' hook codex"` with the payload on stdin and FLOW_ID set.
   - `main -> hook(store, config, "codex", Hooked::Flow(id), payload, now)`, then `Adapter::translate`, then
     per kind `fold::changes` and `Store::append`.
   - UserPromptSubmit: `Store::set_busy(Flow(id), true)`.
   - Stop, when `!wakes_itself()`: `deliver::worker_news(store, id, now)`.
     - Some: append `Working`, return `HookOutcome { news }`; main prints it to stderr and exits 2.
     - None: `set_busy(false)`, exit 0.
   - No FLOW_ID and no --master, bad JSON, or an unknown flow: one stderr line, nothing appended, exit 0.
3. **Resume.** `Server::reopen_worker` or `refresh -> worker_spec`.
   - The window is set: `resume_argv(agent, id, name, flow.session.id)` gives `codex <wiring> resume <sid>`.
   - No session known: `agent_argv`, a fresh start.
   - Resume exits non-zero within RESUME_GRACE: `open_fresh`, as today.
4. **A message for an idle Codex worker.** `flow send` appends a Message, then
   `Server::serve -> deliver_news()`.
   - `server_delivered(key)` gives `Some(Waiter::Flow(id))`.
   - `!Store::busy(waiter)` and `typed_at[key]` is older than QUIET (or absent).
   - `deliver::worker_news(store, id, now)` gives `Some(news)`.
   - `Host::write(key, paste(news))`, and Codex submits it, so UserPromptSubmit sets busy and the flow shows working.
   - Write fails: one stderr line. The news is already marked delivered and is lost, the same as with a dead `flow wait`.
5. **Codex as the master.**
   - `Server::open_master -> Config::master_agent()` gives `("codex", agent)`.
   - `MasterRecord::spec(fresh, agent, env, &instructions, wake, store.master_session())`, which adds
     `instruct(text)` and `master_wiring(flow_exe, wake)`.
   - `codex -> flow hook codex --master -> hook(.., Hooked::Master, ..)`: `Store::set_master_session(session_id)`,
     and `set_busy(Master, ..)` on UserPromptSubmit and Stop.
   - Stop with wake: `deliver::master_news(store, flows)`, exit 2 with it.
   - Idle: `Server::deliver_news` with `Waiter::Master`.
   - Unknown `[master] agent`: `Host::fail(Master, "no agent named X")`.
6. **Peek.** `PeekCache::refresh(flow) -> peek::transcript_of(flow, claude_home)` gives `(rollout, Format::Codex)`,
   then `tail(file, max, Format::Codex) -> parse_codex`. Without a session, a file or with a `.zst` file: an empty peek.

## Test seams

- **Adapter.**
  - `codex::translate`: table tests over payloads recorded from the real CLI, in `src/adapter/fixtures/codex/`.
  - `codex::hooks` and `toml_string`: every event runs the exe; each value parses as TOML; multiline text round-trips.
  - `Adapter::instruct`, `wakes_itself` and `wiring`: per variant. Claude's output is unchanged apart from where WORKER_PROMPT lives.
- **Log and launch.**
  - `fold`: `session` sets `view.session`, and a repeat is no change.
  - `launch`: `{session}` is filled; a missing session means a fresh start.
- **Delivery and config.**
  - `deliver`: tempdir Store; news is taken once.
  - `config`: the built-in codex agent, `[master] agent`, `worker_agents` without master; `[master]` stays `deny_unknown_fields`.
  - `master.rs`: a Codex master gets instruct and hooks; a missing session means fresh; the Claude master gets `--append-system-prompt` with master.md's text.
- **Server.** A `cat` agent with `adapter = "codex"`. A message is typed only when the pane is not busy and quiet (QUIET injected small), and never twice.
- **UI.**
  - `peek::parse_codex`: a recorded rollout fixture.
  - `newflow`: the agent row with two agents, `--agent` in the args; a golden screen of the dialog with the row.
- **CLI.** `tests/cli.rs`: `flow hook codex` on Stop with a pending message exits 2 with it on stderr; without one, exit 0 and silent; `--master` records `waits/master.session`.

## Assumptions

1. Codex's SessionEnd reason is always "other", so a Codex worker never logs `ended` from a hook. You end it with C-space Q or `flow remove`.
2. Plan-mode and async questions (`request_user_input`) are invisible to hooks. They read as working until Stop.
3. plan-progress is a Claude Code mod, so a Codex worker shows no task bar.
4. `spawn_agent` maps to delegated. The description field comes from a recorded fixture.
5. `-c` options before `resume` apply to the resumed session. This is checked end to end.
6. A `.jsonl.zst` rollout shows an empty peek.
7. `FlowView.session` is additive, so `VIEW_VERSION` stays.
8. The agent row lists every agent but `master` and shows only with two or more.

## Decided in review

- Codex can be the master, through `[master] agent`.
- News for an idle Codex session is typed in by the server, guarded by busy and a 10 s QUIET.
- The `session` event goes in the log.
- The Codex peek and the dialog's agent row are in scope.
- The agent-skills are linked into `~/.agents/skills` for Codex. Their content stays agent-agnostic, and
  review pages use lavish where there is no Artifact tool.
- Codex is signed in, so fixtures are recorded from the real CLI.

## Tasks

### Adapter
- [ ] Record Codex hook payloads with the real CLI (a probe hook that appends stdin, in a scratch repo under a private tmux, `~/.codex/config.toml` backed up and restored) into `src/adapter/fixtures/codex/`: user_prompt, user_prompt_skill, pre_bash_skill, pre_spawn_agent, post_bash, post_bash_merge_request, permission_bash, stop, stop_question, interrupt, subagent_bash; and one rollout into `src/ui/fixtures/codex-rollout.jsonl`
- [ ] Move WORKER_PROMPT (made harness-neutral), turn_end, created_merge_request and tool_call from claude_code.rs to adapter/mod.rs; add Adapter::instruct and wakes_itself; claude_code tests still pass unchanged
- [ ] Add EventKind::Session, FlowView.session / Session, fold apply and changes, with fold tests (set, repeat is no change, new id is a change)
- [ ] Add adapter/codex.rs: hooks, toml_string, translate, session, mentioned_skill, read_skill; Adapter::Codex in named/name/wiring/master_wiring/translate; table tests over the fixtures

### Launch, delivery, hook
- [ ] Config: built-in codex agent, MasterConfig.agent, Config::master_agent and worker_agents, built-in master without the file argument; config tests
- [ ] launch: {session} in resume_argv, worker_spec fills it from flow.session and falls back to a fresh start; tests
- [ ] Store set_busy/busy and set_master_session/master_session; deliver.rs with worker_news and master_news moved out of main.rs (wait() uses them); tests
- [ ] main::hook with Hooked and HookOutcome, `flow hook <adapter> --master`, busy marks, Stop hand-over for adapters that do not wake themselves; tests/cli.rs cases

### Server
- [ ] MasterRecord::spec with Instructions, instruct, master_wiring(wake) and session; open_master through Config::master_agent and store.master_session; master.md harness-neutral with master-v3.md kept; tests
- [ ] Server typed_at, deliver_news, server_delivered, paste with an injectable QUIET; server tests with a codex-adapter `cat` agent

### UI
- [ ] peek: Format, transcript_of, tail with format, parse_codex over the recorded rollout; PeekCache uses transcript_of
- [ ] New-flow dialog agent row (NewFlowForm agents/agent, Field::Agent, picked_agent), UiEnv.agents, Job::Start --agent; tests and a golden screen

### Docs and setup
- [ ] Docs: README (Codex agent, [master] agent, trusting the hooks once), CLAUDE.md module map (codex.rs, deliver.rs, plan list), HANDOVER (Codex facts and decisions), plan-flow-adapters.md pointer to this plan
- [ ] Link design-code, implement-plan, review-changes, write-merge-request and ship-feature from ~/projects/agent-skills into ~/.agents/skills (as commit already is), recorded in dotfiles-v2 if it manages that folder
- [ ] End-to-end: in flow ui, a Codex worker (after one hooks review) shows phase, a permission wait, waiting on a question; the master's `flow send` reaches it while idle and while busy; it resumes after `flow ui --stop`; peek shows its transcript; then `[master] agent = "codex"` runs a Codex master that is woken when the worker stops
