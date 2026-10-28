# Kinds of flows

Review pages: approaches https://claude.ai/artifact/EnC8agfFM24ud8qXiGQDyi (`docs/design/kinds-of-flows-approaches.html`),
implementation https://claude.ai/artifact/J6pbSP8y5hrtjA56yQsu2V (`docs/design/kinds-of-flows-implementation.html`).

## Decision

- Approach B: kinds live in config.toml as `[kinds.<name>]`, with feature, fix and research built
  in; a built-in name replaces the built-in whole. `saaio start` records the kind's name
  (`flow_kind`), its whole workflow (phases with skills), its first prompt template and whether it
  merges in the flow's `started` line. After start, the server and `saaio hook` read the flow's
  own log, never config. Old logs fall back to config's default workflow, as today.
- First prompt: a kind has a `skill` and a `prompt` (default `{skill} {request}`, or `{request}`
  without a skill). `{skill}` is written as the agent's harness calls a skill: `/ship-fix` in
  Claude Code, `$ship-fix` in Codex, bare without an adapter. An agent's own `prompt` in
  config.toml wins for every kind; the built-in claude and codex agents have none any more.
  `[worker] prompt` becomes the feature kind's prompt.
- Branch type: a kind lists the types it means; the first one the repo's `[branch] types` lists
  wins, else the repo's first. `--type` overrides. No convention: `flow/<name>` as today.
- Research: `branch = false`. Its worktree is detached at the base (`git worktree add --detach`),
  with a slot and the setup hook. One phase, `research`, entered on its first turn. It is recorded
  as `"merges":false`; in such a flow a turn that ends without a question folds to `done`
  ("answered") instead of `idle`, so it gets what a done flow gets (DONE group, services paused,
  the master told it is ready). `saaio clear` also takes a done flow that merges nothing
  (`FlowView::is_clearable`): pane closed, teardown, worktree removed (kept with uncommitted
  work), forgotten, slot released.
- Old configs: top-level `[[phases]]` is the feature kind's; with `[[kinds.feature.phases]]` too
  it is an error on the `phases` line. `[worker] kind` picks the default kind, `feature`.
- Shown: `kind` in `saaio status` (list column and detail), `--json` (new key, `VIEW_VERSION`
  stays 3), the board's detail facts, the sidebar's faint marker for a kind other than feature,
  and a kind row in the new-flow dialog.
- master.md: the master picks the kind from the request and always passes `--kind`: a bug report
  or a ticket of type bug is `fix`, a question to answer is `research`, anything else `feature`.

## Units

### src/config/kind.rs (new)

(Under config, not top-level: `prompt_for` takes `config::Agent` and config holds `Kind`, so a
top-level `kind` module would make a cycle `layering.rs` refuses.)

What a kind of flow is: its workflow, how it is started, and whether it gets a branch.

```rust
pub const DEFAULT_KIND: &str = "feature";

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Kind {
    pub phases: Workflow,
    #[serde(default)]
    pub skill: Option<String>,
    #[serde(default)]
    pub prompt: Option<String>,
    #[serde(default)]
    pub branch_types: Vec<String>,
    #[serde(default = "with_branch")]
    pub branch: bool,
}

impl Kind {
    pub fn builtins() -> BTreeMap<String, Kind>;
    pub fn feature() -> Kind;
    /// agent.prompt, else self.prompt, else "{skill} {request}" ("{request}" without a skill),
    /// {skill} as the agent's adapter calls it; {request}, {id}, {name} left for the pane.
    pub fn prompt_for(&self, agent: &Agent) -> String;
}
```

Built-ins:

```text
feature  skill ship-feature  branch_types [feature, feat]
         plan design-code, implement implement-plan, review review-changes,
         merge-request write-merge-request
fix      skill ship-fix      branch_types [fix, bugfix, hotfix]
         diagnose diagnose-bug, implement implement-plan, review review-changes,
         merge-request write-merge-request
research branch = false
         prompt "Research this and answer here. Change no code and commit nothing: {request}"
         research (no skills)
```

### src/workflow.rs (modified)

`Phase` moves here from event.rs: `Started` now holds a `Workflow`, and workflow.rs naming
`event::Phase` would make a cycle.

`Workflow` and `PhaseSpec` derive `Serialize` always (a list of phases).

```rust
/// Whether the phase at `index` has no skill to start it.
pub fn starts_unasked(&self, index: usize) -> bool;
```

### src/config.rs (modified)

```rust
pub struct Config {
    pub agents: BTreeMap<String, Agent>,
    pub kinds: BTreeMap<String, Kind>,
    #[serde(rename = "phases")]
    legacy_phases: Option<Workflow>,
    pub worker: WorkerConfig, // + kind: String, DEFAULT_KIND unless set
    pub master: MasterConfig,
    pub ui: UiConfig,
    pub services: ServicesConfig,
}

impl Config {
    pub fn kind(&self, name: Option<&str>) -> Option<(&str, &Kind)>;
    pub fn worker_kinds(&self) -> Vec<(&str, &Kind)>; // the UI needs each kind's types and branch
    /// The default kind's workflow: the master's, and an old flow's that recorded none; the
    /// built-in one for a Config that never loaded its built-ins.
    pub fn workflow(&self) -> Workflow;
}
```

- `Config.workflow` field removed; `Agent.prompt` stays `Option` after load (None: the kind's);
  `resolve_prompts` removed; built-in claude and codex have `prompt: None`, master `Some("")`.
- `add_builtins`: built-in kinds under the file's; `legacy_phases` into `kinds["feature"].phases`;
  `[worker] prompt` into `kinds["feature"].prompt` when the file's feature kind sets none.
- `parse` refuses top-level `[[phases]]` beside `[[kinds.feature.phases]]` on the `phases` line:
  "[[phases]] is the feature kind's; set it in one place, [[kinds.feature.phases]]". A `[worker]
  kind` naming no kind is an error on its line.
- `examples/config.toml`: `[worker] kind`, and every built-in kind commented out.

### src/adapter.rs (modified)

```rust
/// "/ship-fix" in Claude Code, "$ship-fix" in Codex.
pub fn skill_call(self, skill: &str) -> String;
```

### src/event.rs (modified)

```rust
Started {
    name: String,
    request: String,
    mode: Mode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    phases: Option<Vec<Phase>>,           // read from old lines; new lines write workflow
    #[serde(rename = "flow_kind", default, skip_serializing_if = "Option::is_none")]
    kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    workflow: Option<Workflow>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    prompt: Option<String>,
    #[serde(default = "merges", skip_serializing_if = "merges_by_default")]
    merges: bool,
}
```

### src/fold.rs (modified)

- Phases from `workflow`, else `phases`, else `Workflow::builtin()`.
- The view carries `kind`, `workflow`, `prompt`, `merges` from `Started`.
- First `Working` with no phase entered: a first phase that `starts_unasked` is entered.
- `merges: false`: `Idle` folds to `State::Done`, timeline "answered: <note>", its steps done and its
  phase's clock stopped; a later `Working` starts the clock again.

### src/view.rs (modified)

```rust
pub struct FlowView {
    // ...
    pub kind: Option<String>,          // --json key "kind"; None for plain and older flows
    #[serde(skip)]
    pub workflow: Option<Workflow>,
    #[serde(skip)]
    pub prompt: Option<String>,
    #[serde(skip)]
    pub merges: bool,
}

impl FlowView {
    /// An ended flow, or a done one that merges nothing.
    pub fn is_clearable(&self) -> bool;
}
```

### src/branching.rs (modified)

```rust
/// `asked` when listed, else the first of `preferred` listed, else the first; None without types.
pub fn pick_type(&self, asked: Option<&str>, preferred: &[String]) -> Result<Option<String>>;
```

### src/launch/worktree.rs, src/launch/pane.rs (modified)

```rust
pub fn workspace_for(repo: &Path, name: &str, branch: Option<String>, slot: u8, agent: &str) -> Workspace;
/// add [--no-track] -b <branch> <path> [from], or add --detach <path> [from] without a branch.
pub fn add_argv(repo: &Path, workspace: &Workspace, from: Option<&str>) -> Vec<String>;
pub fn agent_argv(agent: &Agent, prompt: &str, id: &str, name: &str, request: &str) -> Vec<String>;
```

`repo_of` knows a plain session by neither branch nor slot (was: no branch), so a detached
worktree has its repository: the pane's hook variables, teardown and removal on clear. The peek's
`PeekLine::Code` gains `plain`, so a detached worktree reads "worktree", not "folder".

`worker_spec` takes the template from `flow.prompt`, else `Kind::feature().prompt_for(agent)`.
`server/master.rs` passes `""`.

### src/cli/commands/start.rs (modified)

```rust
pub struct Args {
    // ...
    /// Kind of flow from the config: feature, fix, research [default: the config's, feature]
    #[arg(long)]
    kind: Option<String>,
    /// Branch type from the repo's .saaio/config.toml [default: the kind's, else its first]
    #[arg(long = "type")]
    branch_type: Option<String>,
}

// in commands.rs, shared with saaio new
fn pick_kind<'a>(config: &'a Config, asked: Option<&str>) -> Result<(&'a str, &'a Kind), Failure>;
// the kind's name for the refusal: "a research flow has no branch, so no branch type"
fn pick_branch_type(rules: Option<&Branching>, (name, kind): (&str, &Kind), asked: Option<&str>) -> Result<Option<String>, Failure>;
```

`--plain` conflicts with `kind` and `branch_type`.

### src/cli/commands/new.rs (modified)

`--kind`; records `kind`, `workflow`, `merges`, no prompt.

```rust
pub fn run(args: Args, store: &Store, config: &Config, now: u64) -> Result<Document, Failure>;
```

### src/cli/commands/hook.rs (modified)

```rust
/// The flow's recorded workflow; the feature kind's for a flow from before kinds; config.workflow()
/// for the master and nobody.
fn workflow_for(store: &Store, hooked: &Hooked, config: &Config, now: u64) -> Workflow;
```

### src/cli/commands/clear.rs (modified)

Filters with `flow.is_clearable()` instead of `is_finished()`.

### src/cli/output/status.rs, start.rs, flow.rs (modified)

- List rows `flows[n]{name,kind,state,phase,tasks,age}`, kind empty for plain and older flows.
- Detail: `kind: fix` after `mode`.
- `saaio start` and `saaio new` documents name the kind; a research start says it has no branch.

### src/ui (modified)

```rust
// newflow.rs
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KindChoice {
    pub name: String,
    pub branch_types: Vec<String>,
    pub branch: bool,
}

pub struct NewFlowForm {
    // ...
    pub kinds: Vec<KindChoice>,
    pub kind: usize,
    pub branch_type: usize, // was `kind`
}

pub fn new(repo: PathBuf, place: String, agents: Vec<String>, kinds: Vec<KindChoice>) -> NewFlowForm;
pub fn picked_kind(&self) -> Option<&str>;   // when not the default
fn has_type_row(&self) -> bool;               // repo has types and the kind a branch
fn pick_branch_type(&mut self);               // kind changed or convention arrived
fn extra_fields(&self) -> Vec<Field>;         // kind, type, agent, base rows that show, in order
fn row_of(&self, field: Field) -> u16;        // one place for every row's position and tab order

// app.rs
pub fn set_kinds(&mut self, kinds: Vec<KindChoice>);
```

- Rows: request, kind, type, agent, base, mode. Kind row cycles with left and right, like agent.
  Plain mode hides kind, type and base.
- `Job::args` adds `--kind <name>` when not the default; `--type` from `branch_type`.
- `ui.rs` sets kinds from `config.worker_kinds()` at start and on config change.
- `sidebar/rows.rs`: the first row's faint marker reads `fix claude` for a kind other than
  feature, clipped like the agent.
- `board/detail.rs`: the kind before the mode in the facts, for every flow that has one.

### src/server/master.md, master-v13.md, master.rs (modified)

master-v13.md is today's master.md, added to `INSTRUCTIONS.earlier`. New bullet after the one on
`saaio start`:

```text
- Pick the flow's kind from the request and always pass it: `--kind fix` for a bug report or a
  ticket of type bug, `--kind research` for a question to answer or something to find out (it
  changes no code and has no branch or merge request), `--kind feature` for anything else. The
  config may add kinds; an unknown one is refused with the list.
```

The idle and done bullet: a research flow is done once it answers; say its findings are ready
(`saaio status <name> --full`) and that `saaio clear` forgets it once the user has read them. The
`--type` bullet: the type defaults to the kind's.

## Data

- config.toml: `[worker] kind`, `[kinds.<name>]` with `skill`, `prompt`, `branch_types`,
  `branch`, `[[kinds.<name>.phases]]`; top-level `[[phases]]` still accepted.
- started line:

```json
{"v":1,"at":1793146307000,"kind":"started","name":"shop-230","request":"SHOP-230 fix totals on refunds",
 "mode":"guided","flow_kind":"fix",
 "workflow":[{"name":"diagnose","skills":["diagnose-bug"]},{"name":"implement","skills":["implement-plan"]},
             {"name":"review","skills":["review-changes"]},{"name":"merge-request","skills":["write-merge-request"]}],
 "prompt":"/ship-fix {request}"}
```

  research adds `"merges":false`.
- `--json`: `"kind": "fix"`, null for plain and older flows; `"merges": false` for research.
- The board's `x` clears by `is_clearable` too.

## Call chains

### saaio start --kind fix "SHOP-230 fix totals on refunds"

1. start::run -> pick_kind(config, Some("fix")): ("fix", &Kind)
2. start::run -> Branching::load(repo): Option<Branching>
3. start::run -> pick_branch_type(branching, kind, args.branch_type) -> Branching::pick_type(None, &kind.branch_types): Some("bugfix")
4. start::run -> Kind::prompt_for(agent) -> Adapter::skill_call("ship-fix"): "/ship-fix {request}"
5. start::run -> Store::create(Started { kind, workflow, prompt, merges: kind.branch, phases: None, .. }): id
6. start::run -> branching::branch_name, free_branch: "bugfix/SHOP-230-fix-totals-on-refunds"
7. start::run -> worktree::workspace_for(repo, name, Some(branch), slot, agent) -> workspace::create_worktree
8. setup hook, Workspace event, fold, output::start::started: unchanged

Errors, before anything is created, exit 1:
- unknown kind: "no kind named fixx", help "Kinds: feature, fix, research"
- `--kind research --type x`: "a research flow has no branch, so no branch type", help "Leave out --type"
- a `--type` the repo does not list: as today

Research: step 3 is None without reading types, step 6 skipped, `workspace_for(.., None, ..)`,
`add_argv` writes `worktree add --detach <path> [base]`.

### saaio server opens a fix flow's pane

1. panes -> pane::worker_spec(flow, agent, env, trusted): PaneSpec
2. worker_spec -> flow.prompt, else Kind::feature().prompt_for(agent)
3. worker_spec -> agent_argv(agent, prompt, id, name, request): [.., "/ship-fix SHOP-230 fix totals on refunds"]

Resume never uses the prompt.

### saaio hook claude-code: the worker runs /diagnose-bug

1. hook -> workflow_for(store, hooked, config, now): Workflow
2. workflow_for (Flow) -> Store::events(flow) -> fold::fold: FlowView -> view.workflow
3. workflow_for (Master, Nobody, no workflow recorded, unreadable) -> config.workflow().clone()
4. hook -> adapter.translate(payload, &workflow, approver): [Phase { diagnose }]
5. hook -> report(..): as today

### A research flow answers and is cleared

1. hook -> translate: [Idle { note }] -> report appends it
2. fold (merges false) -> State::Done, timeline "answered"
3. server: master woken with the flow done; pause_watch pauses services after pause_after_minutes
4. saaio clear -> flows.filter(is_clearable) -> close_pane, retire_worktree, drop_branch (no branch: nothing), restack, Store::remove

### Config::read with an old [[phases]]

1. Config::read -> parse: refuses [[phases]] beside [[kinds.feature.phases]], "<file>:<line>: ..."
2. Config::read -> add_builtins: built-in agents (no prompts), built-in kinds under the file's
3. add_builtins -> legacy_phases into kinds["feature"].phases; worker.prompt into kinds["feature"].prompt
4. unknown [worker] kind -> error on its line

### New-flow dialog: pick fix and start

1. ui.rs -> App::set_kinds(KindChoice per config.worker_kinds())
2. App opens View::NewFlow(NewFlowForm::new(repo, place, agents, kinds))
3. right on the kind row -> kind += 1 -> pick_branch_type(): type row to "bugfix"
4. LoadBranching answer -> set_branching -> pick_branch_type()
5. enter -> Job::Start(form).args(): ["start", "--mode", "guided", "--kind", "fix", "--type", "bugfix", "--", request]
6. a refused start shows its message under the request, as today

## Test seams

- kind.rs: built-ins, prompt_for per adapter, with an agent prompt, with a kind prompt.
- config/tests.rs: `[kinds.*]` read, built-in replaced whole, legacy `[[phases]]` and `[worker]
  prompt` moved to feature, both phases refused on their line, unknown `[worker] kind`, example file.
- branching.rs: preferred types, asked wins, repo's first as fallback.
- event.rs: new started round trip with `flow_kind`, old line without the fields.
- fold/tests.rs: phases from workflow, kind on the view, research step entered on first turn,
  idle -> done for merges false, working again after.
- launch/worktree.rs, workspace.rs: `--detach` argv; a real detached worktree on a temp repo.
- tests/cli/: start --kind fix (branch, recorded prompt), --kind research (detached), unknown kind,
  research with --type, status list, detail and --json kind, new --kind, hook moves a fix flow to
  diagnose on /diagnose-bug, clear forgets a done research flow and keeps a done feature flow.
- ui: dialog kind row, type row following the kind, research hiding it, Job args (unit);
  sidebar marker and board detail (golden).

## Assumptions

1. Fix's phases are diagnose, implement, review, merge-request; colors follow position.
2. `saaio new` takes `--kind`.
3. A user kind must name its `phases`; a built-in name replaces it whole.
4. Old flows show no kind (null in --json, nothing in the sidebar).
5. An old flow never opened before the upgrade gets the built-in feature prompt for its agent.
6. The sidebar shows the kind only when it is not feature.
7. Research keeps a slot and runs the setup hook.
8. The dialog's kind row is always shown, right below the request.

## Decided in review

- `flow_kind` is the started line's key.
- A research flow is done once a turn ends without a question; `saaio clear` forgets a done flow
  that merges nothing. A feature flow still needs its session ended first.

## Tasks

### Core

- [x] Add `Adapter::skill_call` and `src/config/kind.rs` (`Kind`, `builtins`, `feature`, `prompt_for`, `DEFAULT_KIND`) with unit tests per adapter and prompt precedence; `Workflow`/`PhaseSpec` Serialize and `Workflow::starts_unasked`
- [x] Config: `kinds`, `legacy_phases`, `[worker] kind`, `Config::kind`, `workflow` (`worker_kinds` with the UI task, its only user); `agent_argv` takes the prompt and `worker_spec` the feature kind's, since built-in agents lost theirs; agents without built-in prompts; legacy `[[phases]]` and `[worker] prompt` moved to feature; refusals; `examples/config.toml`; config tests
- [x] `Branching::pick_type(asked, preferred)` with unit tests
- [x] `Started` gains `flow_kind`, `workflow`, `prompt`, `merges`; fold reads them into `FlowView` (`kind`, `workflow`, `prompt`, `merges`), enters an unasked first phase on the first turn, folds idle to done for merges false; `FlowView::is_clearable`; event and fold tests
- [x] `agent_argv` takes the prompt; `worker_spec` uses `flow.prompt` with the feature fallback; master passes ""; pane tests

### Commands

- [x] `workspace_for` with an optional branch and `add_argv` with `--detach`; worktree tests including a real detached worktree
- [x] `saaio start --kind`, `--type` into `branch_type`, `pick_kind`, `pick_branch_type`, research without a branch; `saaio new --kind`; CLI tests for fix, research, unknown kind, research with --type
- [x] `saaio hook` translates with `workflow_for`; CLI test: a fix flow moves to diagnose on /diagnose-bug
- [x] `saaio clear` uses `is_clearable`; CLI test: a done research flow is cleared, a done feature flow kept
- [x] status list column and detail field, start and new documents name the kind; `--json` kind; output tests

### UI

- [x] `KindChoice`, the dialog's kind row, `branch_type` rename, type row following the kind, `Job::args --kind`, `App::set_kinds` from `ui.rs`; dialog unit tests
- [x] Sidebar marker for a kind other than feature and the board's detail fact; golden tests updated

### Master and docs

- [x] master.md bullets, master-v13.md as the earlier default
- [x] README, docs/configuration.md (kinds, old [[phases]]), cli.md (--kind, --type default, clear), flows.md (kinds, research lifecycle), ui.md (kind row), json.md (kind), ROADMAP.md (kinds of flows built), CHANGELOG.md Unreleased, CLAUDE.md (kind.rs, plan list)

### End to end

- [x] With a scratch SAAIO_HOME and XDG_STATE_HOME and a scratch repo with `[branch] types = ["feature", "bugfix"]`: `saaio start --kind fix` gives a bugfix branch and the pane's `/ship-fix` prompt, `--kind research` a detached worktree; `saaio status`, `--json` and the UI's dialog, sidebar and board show the kinds; a research flow answers, is done, and `saaio clear` forgets it; end with `saaio --stop`

### After review

Decided by the master: 1 fix, 2 keep (answers stay cut at 4000 characters), 3 fix, 4 fix.

- [x] `saaio hook` translates a flow that recorded no workflow with the feature kind's phases, the master and nobody with the default kind's; CLI test with `[worker] kind = "fix"` and a pre-kinds flow
- [x] A flow that merges nothing gets no base-merged note from `wake::undelivered` (its `base-merged` event still records the move); wake unit test
- [x] Kind names take the phase-name alphabet (lowercase letters, digits, `-`): config refuses another on the line of its key; config test

## Review

Isolated review of flow/kinds-of-flows against main and this plan. The build matches the plan's
units, call chains and test seams; no bug in behaviour was found.

### Fixed

- `report`'s doc comment in `cli/commands/hook.rs` had been left above the new `workflow_for`,
  which so carried two docs and `report` none: moved back (23ea0ba).
- docs/flows.md said `saaio status <name> --full` shows a research flow's whole answer; the
  answer is cut at 4000 characters (`LONG_TEXT_LIMIT`): says so, and that the pane holds all of
  it (64f58d9).

Both are comment and docs only, so no second review round was needed; fmt and clippy pass.

### Open

- An old flow (no `workflow` recorded) is translated by `saaio hook` with `Config::workflow()`,
  the default kind's. Every pre-kinds flow was a feature flow, so with `[worker] kind = "fix"` set
  while such a flow still runs, its feature skills (design-code, ...) move it nowhere.
  Recommendation: fall back to the `feature` kind's workflow for flows without a recorded one,
  keep the default kind's for the master; low priority, it only affects flows started before the
  upgrade.
- A research answer is capped at 4000 characters in the log. Recommendation: keep it; long
  findings belong in the pane or a file the worker writes, and the master's text already points
  at `--full`.
- A research flow started with `--base X` still gets a `base-merged` note when X merges, though
  its detached worktree has nothing to move. Harmless; recommendation: leave it.
- A user kind named with spaces or commas works but reads oddly in the TOON list; config does not
  restrict kind names as it does phase names. Recommendation: restrict to the phase-name alphabet
  in a follow-up if it matters.

### Where to look

1. `src/fold.rs` `Folding::apply`, the `Working` and `Idle if !merges` arms, then `finish`:
   the research lifecycle (done on answer, clock restarted on a follow-up, steps all done).
2. `src/config.rs` `parse` and `add_builtins`: old `[[phases]]` and `[worker] prompt` moved into
   the feature kind, the refusals and their lines. This is what existing users' files hit.
3. `src/cli/commands/start.rs` `run` and `pick_branch_type`, with `launch/worktree.rs`
   `add_argv`/`repo_of`: research's detached worktree, and `repo_of` now taking a slot as the
   sign of a worktree (teardown, hook variables, pause scripts and clear all follow it).
4. `src/cli/commands/hook.rs` `workflow_for`: the per-flow workflow every hook now folds the log
   for.

Routine, skimmable: the `Phase` move from event.rs to workflow.rs, the `kind: None, ...` fixture
lines, the golden files, the new-flow dialog row plumbing and the docs.

### Checks

fmt, test, clippy and deny passed at 6107914; after the two fix commits (comment and docs only)
fmt and clippy pass again.
