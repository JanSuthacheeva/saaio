# Codex: a second harness for workers and the master

Implementation page (review surface): https://claude.ai/artifact/GyDfWffvXbf51Ff14As68u, copy in
`docs/design/codex-implementation.html`; approaches: https://claude.ai/artifact/3y32yGaMR3cZwCBcYsVGm5,
`docs/design/codex-approaches.html`.

Revised 2026-10-16 after merging origin/main:
- the library and thin-binary structure;
- plain sessions as a third Mode;
- deliver.rs and `Document::Wake` already on main;
- protocol 3.

The Codex facts were checked again at `openai/codex@6ae8b8aa7c60` (latest release 0.163.0; local CLI 0.144.1).

## Decision

- Approach A: `Adapter::Codex` passes flow's hooks to every Codex launch, fresh and resumed, as
  `-c hooks.<Event>=[...]`. You trust them once in Codex's startup review (`t`), and Codex stores that
  trust in `~/.codex/config.toml` until the hook definition changes. flow writes nothing into `~/.codex`.
- Codex cannot take a session id up front, so flow learns it from the first hook:
  - a `session` event in the flow's log, for workers and plain sessions;
  - `waits/master.session`, for the master.

  `{session}` fills resume templates.
- A Codex session cannot wake itself (Codex has no asyncRewake), so news reaches it two ways:
  - Busy: its synchronous Stop hook hands the news over (`Document::Wake`: stderr, exit 2) and Codex goes on.
  - Idle: `flow server` types it in with a bracketed paste and Enter, only when the session is not busy and
    no key has gone into that pane for `QUIET` (10 s).
- Codex can be the master: `[master] agent = "codex"` in config.toml. The default stays Claude.
- A plain Codex session (`Mode::Plain`) gets the hooks only: no worker prompt, no news.
- In scope:
  - the Codex peek, which reads the rollout JSONL;
  - an agent row in the new-flow dialog, for every mode, plain included.
- The skills stay agent-agnostic. A session with the Artifact tool publishes review pages as Artifacts; any
  other agent (Codex) uses the lavish skill, styled from `~/projects/dotfiles-v2/agents/design-system`.

## Verified Codex facts (6ae8b8a, paths under codex-rs/)

- **Hooks.** Events: `PreToolUse`, `PermissionRequest`, `PostToolUse`, `PreCompact`, `PostCompact`,
  `SessionStart`, `SessionEnd`, `UserPromptSubmit`, `SubagentStart`, `SubagentStop`, `Stop`, `Interrupt`
  (config/src/hook_config.rs:37-59). The feature is stable and on, in 0.144.1 too.
  - A command runs as `$SHELL -lc` with the payload JSON on stdin (hooks/src/engine/command_runner.rs:254, 395).
  - Plain stdout on SessionStart becomes model context, so `flow hook codex` never prints to stdout.
  - Exit 2 with stderr on a synchronous Stop hook makes Codex go on with it (hooks/src/events/stop.rs:343).
  - An async hook's exit 2 is dropped, and its context is drained only at the next turn
    (command_runner.rs:157, core/src/session/turn.rs:185). Nothing a hook does wakes an idle session.
- **Trust.** The key is `<source>:<event_snake>:<group>:<handler>` (hooks/src/lib.rs:116); for `-c` hooks the
  source is `/<session-flags>/config.toml` (discovery.rs:421-435).
  - Untrusted hooks wait for the review (tui/src/startup_hooks_review.rs:291). The probe on 0.144.1 showed that
    `t` writes the `trusted_hash`.
  - `--dangerously-bypass-hook-trust` would also run every untrusted hook.
- **Payload.**
  - Common: `session_id`, `transcript_path` (nullable), `cwd`, `hook_event_name`, and `model` except on
    SessionEnd.
  - Turn events: `turn_id`, `permission_mode`; a subagent's events add `agent_id` and `agent_type`.
  - UserPromptSubmit: `prompt`, which keeps the typed `$skill` text (core/src/hook_runtime.rs:693).
  - Pre/PostToolUse: `tool_name`, `tool_input`, `tool_use_id`, and Post adds `tool_response`.
    PermissionRequest: `tool_name`, `tool_input`. Stop: `stop_hook_active`, `last_assistant_message`.
    SessionEnd: `reason`, always `"other"` (events/session_end.rs:24).
  - SessionStart is deferred to the first turn (core/src/session/turn.rs:333).
- **Tools.**
  - `Bash` input is `{"command": ..}`. Its `tool_response` is a JSON string, the output text with no exit
    code (core/src/tools/context.rs:438), and its PostToolUse comes when the process ends.
  - `apply_patch`, `mcp__server__tool`.
  - Recorded on 0.163.0: the subagent tool reaches hooks as `collaborationspawn_agent` (namespace + name),
    input `{task_name, fork_turns, message}`, and `message` is encrypted (`gAAAA...`). It returns at once;
    the main agent then waits with `collaborationwait_agent`. A subagent's own tool calls carry `agent_id`.
  - PermissionRequest's `tool_input` adds `description`, the reason Codex gives.
  - Concurrent hooks run in parallel, so their payloads can arrive at the same time.
- **CLI.**
  - There is no session-id flag. `codex resume <SESSION_ID|name>` works from any folder.
  - Root `-c` options apply to `resume`, with resume-level `-c` over them (cli/src/main.rs:2643, 1505). Probed on 0.163.0: `codex <-c hooks> resume <id>` keeps the
    session id, fires SessionStart with `source: "resume"`, and runs the trusted hooks without a new review.
  - Keys: `developer_instructions` (config_toml.rs:254), `check_for_update_on_startup` (:529),
    `projects.<path>.trust_level` (:611). A worktree resolves to the main repo's trust (git-utils/src/trust.rs:16).
- **Skills.**
  - Locations: `~/.agents/skills`, a repo's `.agents/skills`, `.codex/skills`, and the deprecated `$CODEX_HOME/skills`.
  - Called as `$name` in the prompt, or read through Bash. There is no skill hook event.
- **Rollout.** Recorded on 0.163.0, shell calls are code-mode `custom_tool_call` lines named `exec`, whose
  `input` is a JS script calling `tools.exec_command({cmd:"..."})`. Their `custom_tool_call_output.output` is
  text items, the first being "Script completed / Wall time / Output:". Subagent tools are `function_call`
  lines with a `namespace`.
  `$CODEX_HOME/sessions/YYYY/MM/DD/rollout-<ts>-<uuid>.jsonl` holds lines of the form
  `{timestamp, ordinal?, type, payload}` (history/src/lib.rs:369).
  - `response_item` payloads: `message{role, content[{type:"output_text",text}]}`;
    `function_call{name, arguments: string, call_id}`;
    `function_call_output{call_id?, output: string | [items]}`;
    `custom_tool_call` / `custom_tool_call_output`, `local_shell_call` (protocol/src/models.rs:1045-1200).
  - Files are compressed to `.jsonl.zst` after 7 days.
- **TUI paste.** Bracketed paste is on (tui/src/tui.rs:250). A bracketed paste is one insert, and a following
  `\r` submits. Unbracketed fast typing is buffered as a paste burst (paste_burst.rs:159).
- **Rejected for now: `codex queue --thread <id> --message`** (0.163.0, experimental). It starts a turn on an
  idle thread, but only through Codex's shared app-server daemon. A session with `-c` overrides runs its
  own embedded server, which queue refuses (tui/src/daemon_startup.rs:65-128). Revisit once the daemon
  takes per-session hooks; it would replace the server's typing.

## Units

```rust
// src/adapter.rs (modified)
pub mod claude_code;
pub mod codex;
mod answer;     // shared: how a turn's closing message reads
mod calls;      // shared: how a tool call reads

pub enum Adapter { ClaudeCode, Codex }   // serde kebab-case: "claude-code", "codex"
impl Adapter {
    pub fn named(name: &str) -> Option<Adapter>;              // + "codex"
    pub fn name(self) -> &'static str;
    /// Report hooks, master hand-over (Codex: at Stop) and WORKER_PROMPT through instruct.
    pub fn wiring(self, flow_exe: &Path) -> Vec<String>;
    /// Report hooks only: no prompt, no news.
    pub fn plain_wiring(self, flow_exe: &Path) -> Vec<String>;
    /// Claude: its waiter (only called when wake, as today). Codex: its hooks with --master; hook hands
    /// news over at Stop.
    pub fn master_wiring(self, flow_exe: &Path) -> Vec<String>;
    /// Claude ["--append-system-prompt", text]; Codex ["-c", "developer_instructions=<toml string>"].
    pub fn instruct(self, text: &str) -> Vec<String>;
    /// Claude true (asyncRewake flow wait); Codex false (Stop hand-over, server typing).
    pub fn wakes_itself(self) -> bool;
    pub fn translate(self, payload: &Value, phases: &PhaseMap) -> Vec<EventKind>;
}
/// Moved from claude_code.rs, harness-neutral: "The flow master is the agent session that coordinates
/// the workers for the user. Its messages reach you as hook feedback or as a prompt that starts with
/// \"message from the flow master:\" ..."
pub const WORKER_PROMPT: &str;

// src/adapter/answer.rs (new; moved from claude_code.rs, bodies unchanged)
pub fn turn_end(answer: &str) -> EventKind;
fn plain(line: &str) -> String;
fn sentences(text: &str) -> Vec<String>;

// src/adapter/calls.rs (new; moved from claude_code.rs, bodies unchanged)
pub fn tool_call(name: &str, input: &Value, cwd: &str) -> String;
pub fn created_merge_request(command: &str, stdout: &str) -> Option<String>;

// src/adapter/codex.rs (new, pure)
const EVENTS: [&str; 6] = ["UserPromptSubmit", "PreToolUse", "PostToolUse", "PermissionRequest", "Stop", "Interrupt"];
const HOOK_TIMEOUT_SECS: u64 = 10;
/// ["-c", "check_for_update_on_startup=false"] then per event
/// ["-c", "hooks.<Event>=[{hooks=[{type=\"command\",command=\"'<flow_exe>' hook codex<args>\",timeout=10}]}]"];
/// args "" for workers and plain sessions, " --master" for the master.
pub fn hooks(flow_exe: &Path, master: bool) -> Vec<String>;
pub fn translate(payload: &Value, phases: &PhaseMap) -> Vec<EventKind>;
fn session(payload: &Value) -> Option<EventKind>;           // Session { id: session_id, transcript: transcript_path }
fn mentioned_skill(prompt: &str) -> Option<&str>;           // first "$name" token
fn read_skill(command: &str) -> Option<&str>;               // ".../skills/<name>/SKILL.md" -> name
fn subagent(input: &Value) -> String;                       // task_name, else agent_type, else "a subagent"
pub fn toml_string(text: &str) -> String;                   // TOML basic string: escapes " \ and control chars

// src/adapter/claude_code.rs (modified): helpers and WORKER_PROMPT moved out; settings take the prompt
// argument through Adapter::instruct. Behaviour unchanged; its tests keep passing.

// src/event.rs
EventKind::Session { id: String, transcript: Option<String> }

// src/view.rs
pub struct FlowView { /* ... */ pub session: Option<Session> }   // VIEW_VERSION stays 2 (additive)
pub struct Session { pub id: String, pub transcript: Option<String> }

// src/fold.rs: Folding::apply: Session -> view.session (no timeline entry, no state change);
// changes(): Session counts only when id or transcript differs from the last one.

// src/proto.rs: PROTOCOL_VERSION 3 -> 4 (the Board's FlowView gains a field; postcard is positional).

// src/store.rs
pub fn set_busy(&self, waiter: &Waiter, busy: bool) -> Result<()>;   // waits/<key>.busy
pub fn busy(&self, waiter: &Waiter) -> bool;
pub fn set_master_session(&self, id: &str) -> Result<()>;            // waits/master.session
pub fn master_session(&self) -> Option<String>;

// src/deliver.rs (modified: it exists on main)
/// The flows that stopped since the master last saw them, marked seen, at once (no SETTLE): for the
/// Stop hand-over and the server; master_news keeps SETTLE for flow wait around it.
pub fn master_news_now(store: &Store, flows: &[FlowView]) -> Vec<FlowView>;

// src/config.rs
pub struct MasterConfig { pub wake: bool, pub agent: Option<String> }   // [master] agent = "codex"
impl Config {
    /// [master] agent, else the "master" agent, else the default agent.
    pub fn master_agent(&self) -> Option<(&str, &Agent)>;
    /// Every agent but MASTER_AGENT, the default first.
    pub fn worker_agents(&self) -> Vec<&str>;
}
// builtin_agents(): + "codex": command ["codex", "{prompt}"], prompt "$ship-feature {request}",
//   resume ["codex", "resume", "{session}"], adapter Codex.
// builtin "master": drops "--append-system-prompt-file", "{instructions}" (master.rs instructs).

// src/launch/pane.rs
/// + {session}.
pub fn resume_argv(agent: &Agent, id: &str, name: &str, session: &str) -> Option<Vec<String>>;
/// session = flow.session's id, else the flow id. Must resume, the resume template names {session}
/// and flow.session is None: the fresh command (agent_argv or plain_argv) instead.
pub fn worker_spec(flow: &FlowView, agent: &Agent, env: &LaunchEnv) -> Option<PaneSpec>;

// src/cli/commands/hook.rs
#[derive(clap::Args)] pub struct Args { adapter: String, #[arg(long)] master: bool }
/// Whose hook: a flow's session ($FLOW_ID), the master's (--master), or neither.
enum Hooked { Flow(String), Master, Nobody }
/// Document::Wake(news) when a Stop hands news over, else Silent.
pub fn run(args: &Args, env: &Env, clock: fn() -> u64) -> Document;
fn hook(store: &Store, config: &Config, adapter: Adapter, hooked: &Hooked, payload: &Value, now: u64) -> Option<String>;
/// Stop of a session that does not wake itself: its news, after appending Working; else marks it idle.
fn hand_over(store: &Store, hooked: &Hooked, now: u64) -> Option<String>;

// src/cli/commands/ui.rs: UiEnv { agents: config.worker_agents() as Strings, .. }

// src/server/master.rs
pub struct Instructions { pub path: PathBuf, pub text: String }
impl MasterRecord {
    /// After the program: adapter.instruct(&instructions.text), then master_wiring when the adapter
    /// does not wake itself or `wake`. {instructions} = path; {session} = `session` else self.session.
    /// Fresh when it must resume a {session} template and `session` is None.
    pub fn spec(&self, start: Start, agent: &Agent, env: &LaunchEnv,
                instructions: &Instructions, wake: bool, session: Option<&str>) -> Option<PaneSpec>;
}
// master.md: harness-neutral ("one agent session per feature flow"); the current text joins
// PREVIOUS_DEFAULTS as master-v5.md.

// src/server.rs: Server { /* ... */ typed_at: HashMap<PaneKey, Instant>, quiet: Duration }
// src/server/conn.rs: ClientMsg::Input -> typed_at.insert(focus, now)
// src/server/panes.rs: open_master() uses config.master_agent(), Instructions read from master.md,
//   store.master_session()

// src/server/news.rs (new)
/// No key into a pane for this long before the server types news into it.
pub(super) const QUIET: Duration = Duration::from_secs(10);
impl Server {
    /// Every tick, before refresh's early return: for each open pane whose adapter does not wake
    /// itself (no plain flows; the master only when [master] wake), when not store.busy and quiet,
    /// takes its news (deliver::) and writes paste(news) into it.
    pub(super) fn deliver_news(&mut self);
    fn server_delivered(&self, key: &PaneKey) -> Option<Waiter>;
}
/// ESC[200~ text ESC[201~ then "\r": one prompt, however many lines.
fn paste(text: &str) -> Vec<u8>;

// src/ui/peek.rs (modified) + src/ui/peek/claude.rs, src/ui/peek/codex.rs (split: one parser each)
pub enum Format { Claude, Codex }
pub fn transcript_of(flow: &FlowView, claude_home: &Path) -> Option<(PathBuf, Format)>;
pub fn tail(file: &Path, max: usize, format: Format) -> Result<Vec<PeekLine>>;
// peek/claude.rs: today's parse; peek/codex.rs:
fn parse(text: &str, cut: bool) -> Vec<PeekLine>;
// message (assistant output_text) -> Text; function_call / custom_tool_call / local_shell_call -> Tool;
// custom_tool_call "exec" -> Bash(cmd from its tools.exec_command({cmd:"..."})); its output -> Result:
// the first text item after the "Script completed" header; an unanswered last call -> Now.

// src/ui/newflow.rs (+ newflow/view.rs draws the row)
pub struct NewFlowForm { /* ... */ pub agents: Vec<String>, pub agent: usize }
enum Field { /* ... */ Agent }    // the row shows only with two or more agents, for every mode
pub fn new(repo: PathBuf, place: String, agents: Vec<String>) -> NewFlowForm;
pub fn picked_agent(&self) -> Option<&str>;       // None for the default (first)
// ui/app: Job::Start args + ["--agent", name] when picked_agent(); ui.rs UiEnv { agents, .. }
```

## Translation rules (codex::translate)

Every payload with a `session_id` yields `Session { id, transcript }` first; fold::changes drops repeats.
A subagent's events (`agent_id` set) yield only a phase, from a skill it reads. Otherwise:

- **UserPromptSubmit:** `working`. Also `phase` when the prompt's first `$name` is in the phase map.
- **PreToolUse:**
  - `Bash` whose command reads `.../skills/<name>/SKILL.md`: the phase of `name`.
  - a tool named `*spawn_agent`: `delegated` with `subagent(input)`.
- **PostToolUse:** `working`, except for `*spawn_agent`: the main agent waits next, and `*wait_agent`'s
  PostToolUse means it goes on. For `Bash`, also `merge-request` when `gh pr create` or `glab mr create`
  printed a URL in the string `tool_response`.
- **PermissionRequest:** `waiting` with `permission: Name(target)`.
- **Stop:** `answer` with `last_assistant_message`, then `answer::turn_end`.
- **Interrupt:** `idle` with "interrupted".
- Anything else means nothing. Texts are cut to 200 chars.

## Call chains

1. **Start a Codex worker.**
   - `App` -> `Job::Start::args()` gives `[.., "--agent", "codex"]`.
   - `commands::start` writes the Workspace event.
   - `Server::refresh` (panes.rs) -> `reconcile` -> `pane::worker_spec`.
   - `worker_spec` -> `Adapter::Codex.wiring(flow_exe)`, or `plain_wiring` for a plain session, spliced in
     after "codex" -> `Host::open`.
   - Host::open fails: `Blocked "could not start the agent: .."`.
   - First run: Codex's hooks review shows in the pane, and nothing is reported until you press `t`.
2. **A Codex hook event.**
   - `codex` -> `sh -lc "'<flow>' hook codex"` (stdin JSON, FLOW_ID) -> `cli::run` -> `commands::hook::run`.
   - `hook()` -> `Adapter::translate` -> per kind `fold::changes` -> `Store::append`.
   - UserPromptSubmit: `Store::set_busy(Flow(id), true)`.
   - Stop: `hand_over()` -> `deliver::worker_news(store, id, now)`.
     - `News`: append `Working`, return the text; run returns `Document::Wake(news::for_worker(..))`, and
       main exits 2 with it on stderr.
     - `Nothing` or `Gone`: `set_busy(false)`, `Silent`.
   - No FLOW_ID and no --master, bad JSON, or an unknown flow: one stderr line, nothing appended, `Silent`.
3. **Resume.** `Server::reopen_worker` or `refresh` -> `worker_spec`.
   - The window is set: `resume_argv(agent, id, name, session.id)` gives `codex <wiring> resume <sid>`.
   - No session yet: the fresh command.
   - Resume exits non-zero within RESUME_GRACE: `open_fresh`, as today.
4. **A message for an idle Codex worker.** `flow send` appends a Message, then `Server::serve` ->
   `deliver_news()` (news.rs).
   - `server_delivered(key)` gives `Some(Waiter::Flow(id))`.
   - `!Store::busy`, and `typed_at[key]` is older than QUIET (or absent).
   - `deliver::worker_news` gives `News(msgs)`.
   - `Host::write(key, paste(news::for_worker(msgs)))`: Codex submits it, UserPromptSubmit sets busy, and
     the flow shows working.
   - Write fails: a server.log line. The news is already marked delivered and is lost, the same as with a
     dead `flow wait`.
5. **Codex as the master.**
   - `Server::open_master` -> `Config::master_agent()` gives `("codex", agent)`.
   - `MasterRecord::spec(start, agent, env, &instructions, wake, store.master_session())` -> `instruct(text)`
     plus `master_wiring(flow_exe)`.
   - `codex` -> `flow hook codex --master` -> `Hooked::Master`: `Store::set_master_session(session_id)`, and
     `set_busy(Master, ..)` on UserPromptSubmit and Stop.
   - Stop with `[master] wake`: `deliver::master_news_now` -> `Wake(news::for_master(..))`.
   - Idle: `Server::deliver_news` with `Waiter::Master`.
   - Unknown `[master] agent`: `Host::fail(Master, "no agent named X")`.
6. **Peek.** `PeekCache::refresh(flow)` -> `peek::transcript_of(flow, claude_home)` gives
   `(rollout, Format::Codex)` -> `tail(file, max, Codex)` -> `peek::codex::parse`. With no session, no file,
   or a `.zst` file: an empty peek.

## Test seams

- **Adapter.**
  - `codex::translate`: table tests over payloads recorded from the real CLI in `src/adapter/fixtures/codex/`.
  - `codex::hooks` and `toml_string`: every event runs the exe; values parse as TOML; multiline text round-trips.
  - `Adapter::instruct`, `wakes_itself`, `wiring`, `plain_wiring`: per variant. The claude_code tests stay
    green after the helpers move.
- **Log, launch, delivery.**
  - `fold/tests.rs`: `session` sets `view.session`, and a repeat is no change.
  - `launch/pane.rs`: `{session}` is filled; a missing session means a fresh or plain start.
  - `deliver`: `master_news_now` takes once.
  - `store`: busy marks and `master_session`.
- **Config and master.**
  - `config`: the built-in codex agent, `[master] agent` (still `deny_unknown_fields`), `worker_agents`.
  - `server/master.rs`: a Codex master gets instruct and hooks, and a missing session means fresh; the Claude
    master gets `--append-system-prompt` with master.md's text.
- **Server.** `server/tests.rs` with a `cat` agent on `adapter = "codex"` and a small `quiet`: a message is
  typed only when not busy and quiet, never twice, and never into a plain session.
- **UI.**
  - `peek/codex.rs`: a recorded rollout fixture.
  - `newflow`: the agent row with two agents, `--agent` in the args; a golden screen
    (`FLOW_UPDATE_GOLDEN=1`) of the dialog with the row.
- **CLI.** `tests/cli.rs`: `flow hook codex` on Stop with a pending message exits 2 with it on stderr;
  without one, exit 0 and silent; `--master` writes `waits/master.session`.

## Assumptions

1. Codex's SessionEnd reason is always "other", so a Codex worker never logs `ended` from a hook. You end it with C-space Q or `flow remove`.
2. Plan-mode and async questions (`request_user_input`) are invisible to hooks. They read as working until Stop.
3. plan-progress is a Claude Code mod, so a Codex worker shows no task bar.
4. A `.jsonl.zst` rollout shows an empty peek.
5. `FlowView.session` is additive, so `VIEW_VERSION` stays 2. `PROTOCOL_VERSION` goes to 4, because postcard is positional.
6. The agent row lists every agent but `master`, and shows only with two or more.
7. A paste longer than 1000 characters shows as a placeholder in Codex's composer and is still sent whole.

## Decided in review

- Codex can be the master, through `[master] agent`.
- News for an idle Codex session is typed in by the server, guarded by busy and a 10 s QUIET.
- The `session` event goes in the log.
- The Codex peek and the dialog's agent row are in scope.
- The agent-skills are linked into `~/.agents/skills`. Their content stays agent-agnostic: Artifact where
  the tool exists, else lavish.
- Codex is signed in. Fixtures are recorded from the real CLI, after updating it to 0.163.0 if possible.
- E2E runs of `flow ui` and `flow server` use a scratch `FLOW_HOME=$(mktemp -d)`, never the real store.

## Tasks

### Adapter
- [x] Record Codex hook payloads with the real CLI into `src/adapter/fixtures/codex/`: a probe hook that appends stdin, in a scratch repo under a private tmux (`tmux -L`), `~/.codex/config.toml` backed up and restored. Record user_prompt, user_prompt_skill, pre_bash_skill, pre_spawn_agent, post_bash, post_bash_merge_request, permission_bash, stop, stop_question, interrupt and subagent_bash, plus one rollout into `src/ui/peek/fixtures/codex-rollout.jsonl`
- [x] Move turn_end/plain/sentences to adapter/answer.rs and tool_call/created_merge_request to adapter/calls.rs; WORKER_PROMPT harness-neutral in adapter.rs; add Adapter::instruct; claude_code tests unchanged and green
- [x] Add EventKind::Session, view::Session and FlowView.session, fold apply and changes, PROTOCOL_VERSION 4; tests in fold/tests.rs and proto
- [x] Add adapter/codex.rs (hooks, toml_string, translate, session, mentioned_skill, read_skill, subagent); Adapter::Codex in named/name/wiring/plain_wiring/master_wiring/translate; table tests over the fixtures

### Launch, delivery, hook
- [ ] config: built-in codex agent, MasterConfig.agent, Config::master_agent and worker_agents, built-in master without the file argument; tests
- [ ] launch/pane.rs: {session} in resume_argv; worker_spec fills it from flow.session and falls back to the fresh or plain command; tests
- [ ] store busy marks and master_session; deliver::master_news_now (master_news uses it); tests
- [ ] commands/hook.rs: Adapter::wakes_itself, Args.master, Hooked, hand_over, busy marks, Document::Wake on a Stop with news; tests/cli.rs cases

### Server
- [ ] server/master.rs: Instructions, spec with instruct, master_wiring and session; panes.rs open_master through Config::master_agent and store.master_session; master.md harness-neutral with master-v5.md kept; tests
- [ ] server/news.rs: deliver_news, server_delivered, paste; Server typed_at and quiet; conn.rs records input; server/tests.rs cases with a codex-adapter cat agent

### UI
- [ ] peek split into peek/claude.rs and peek/codex.rs; Format, transcript_of, tail with format; PeekCache uses transcript_of; codex parser tests over the recorded rollout
- [ ] New-flow dialog agent row (NewFlowForm agents/agent, Field::Agent, picked_agent, view.rs), UiEnv.agents, Job::Start --agent; tests and a golden screen

### Docs and setup
- [ ] Docs: README (Codex agent, [master] agent, trusting the hooks once), CLAUDE.md (adapter/codex.rs, answer.rs, calls.rs, server/news.rs, peek/, plan list), HANDOVER (Codex facts, decisions, codex queue for later), plan-flow-adapters.md pointer to this plan
- [ ] Link design-code, implement-plan, review-changes, write-merge-request and ship-feature from ~/projects/agent-skills into ~/.agents/skills (as commit already is), recorded in dotfiles-v2 if it manages that folder
- [ ] End-to-end on a scratch FLOW_HOME: in flow ui a Codex worker (after one hooks review) shows phase, a permission wait and waiting on a question; the master's `flow send` reaches it idle and busy; it resumes after `flow ui --stop`; peek shows its transcript; a plain Codex session reports; then `[master] agent = "codex"` runs a Codex master that is woken when the worker stops
