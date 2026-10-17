# Plan: a configurable workflow and first prompt

Implementation page: https://claude.ai/artifact/Viba3iC8BfoMW4Cdcwd1Ac (approaches: https://claude.ai/artifact/Wr7jpU3Fj4kNdYYcs5wToU, approach A). Pages in `docs/design/workflow-*.html`.

## Decision

Users are no longer tied to Jan's skills. `config.toml` sets the workflow as an ordered
`[[phases]]` list (name, skills that start it) and the worker's first prompt as
`[worker] prompt`. Each flow records its phase names in its `started` event, and `fold` builds the
steps from the log, never from config: a running flow keeps the workflow it started with, and fold
stays pure.

- 1 to 4 phases (`MAX_PHASES`). Default: plan (design-code), implement (implement-plan), review
  (review-changes), merge-request (write-merge-request).
- A log without a recorded list folds with the built-in four and reads `design` as `plan`.
- JSON `VIEW_VERSION` 3. theme.toml `[phase]` colors any phase by name; a phase without one takes
  the built-in color of its position.
- The first prompt: an agent's own `prompt` from the file wins; else `[worker] prompt`, when set,
  exactly as written; else the built-in default of its name (claude `/ship-feature {request}`,
  codex `$ship-feature {request}`). No rewriting between harnesses.

Decided in review: the old `[phases]` skill table is refused with an error naming `[[phases]]`;
phase names are lowercase letters, digits and `-`; a workflow has 1 to 4 phases. Later the user
dropped the Codex `/` -> `$` rewrite for the prompt order above.

## Units

`src/workflow.rs` (new, private module in `lib.rs`): replaces `config::PhaseMap` and
`config::builtin_phases`.

```rust
pub const MAX_PHASES: usize = 4;

/// One [[phases]] entry: its name and the skills that start it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PhaseSpec {
    pub name: Phase,
    #[serde(default)]
    pub skills: Vec<String>,
}

/// 1 to MAX_PHASES phases with distinct names, no skill in two.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "Vec<PhaseSpec>")]
pub struct Workflow { phases: Vec<PhaseSpec> }

impl Default for Workflow { fn default() -> Workflow; }   // builtin()
impl TryFrom<Vec<PhaseSpec>> for Workflow { type Error = String; }

impl Workflow {
    pub fn builtin() -> Workflow;
    /// The names in order: what flow start records.
    pub fn phases(&self) -> Vec<Phase>;
    /// "/design-code" and "plugin:design-code" both find design-code's phase.
    pub fn phase_of(&self, skill: &str) -> Option<Phase>;
}
```

try_from errors: "a workflow has 1 to 4 phases", "two phases are named review",
"design-code starts two phases".

`src/event.rs` (modified):

```rust
/// A phase's name: plan, implement, ... as the workflow names it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Phase(String);

impl TryFrom<String> for Phase { ... }   // lowercase letters, digits, '-'; not empty
impl From<Phase> for String { ... }
impl FromStr for Phase { ... }           // clap: flow report phase <name>
impl Phase { pub fn name(&self) -> &str; }

Started {
    name: String,
    request: String,
    mode: Mode,
    /// The flow's phases in order; None in a log from before they were configurable, and for a
    /// plain session.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    phases: Option<Vec<Phase>>,
},
```

Removed: `Phase::ALL`, `Phase::index`, the `ValueEnum` and `Copy` derives. `EVENT_VERSION` stays 1.

`src/fold.rs` (modified; public signatures unchanged):

```rust
struct Folding {
    view: FlowView,
    phases: Vec<Phase>,               // Started.phases, else Workflow::builtin().phases()
    legacy: bool,                     // no list recorded: "design" reads as "plan"
    durations: Vec<Option<u64>>,      // by position in phases
    entered: Vec<bool>,
    phase_since: u64,
    stopped_at: Option<u64>,
}

impl Folding {
    fn new(id: &str, name: &str, request: &str, mode: Mode, phases: Option<&[Phase]>, at: u64) -> Folding;
    /// Where `phase` stands in the flow's phases (after the legacy rename); None when it has none.
    fn position(&self, phase: &Phase) -> Option<usize>;
    fn enter(&mut self, position: usize, at: u64);
}

/// "design" -> "plan", for logs written before phases were configurable.
fn legacy_name(phase: &Phase) -> Phase;
```

A Phase event the flow has no phase for changes nothing. A MergeRequest event enters the flow's
last phase. `shown` compares `view.phase.as_ref()`.

`src/view.rs` (modified): `VIEW_VERSION = 3`; `FlowView.phase: Option<Phase>`, `Step.phase: Phase`.

`src/config.rs` (modified):

```rust
pub struct Config {
    pub agents: BTreeMap<String, Agent>,
    /// [[phases]]: the built-in workflow unless the file has one.
    #[serde(rename = "phases")]
    pub workflow: Workflow,
    pub worker: WorkerConfig,
    pub master: MasterConfig,
    pub ui: UiConfig,
}

pub struct Agent {
    pub command: Vec<String>,
    /// Its own first prompt; None takes [worker] prompt, else the built-in agent's of its name.
    /// Some after load for every agent.
    #[serde(default)]
    pub prompt: Option<String>,
    pub resume: Option<Vec<String>>,
    pub adapter: Option<Adapter>,
}

pub struct WorkerConfig {
    pub agent: String,
    /// The first prompt of every agent the file gives none, the built-in ones included; unset,
    /// each keeps its built-in default.
    pub prompt: Option<String>,
}

impl Config {
    /// Also refuses the old [phases] skill table, naming [[phases]].
    fn parse(text: &str) -> Result<Config>;
    /// Each agent's first prompt: its own from the file, else [worker] prompt, else the built-in
    /// agent's of its name, else none.
    fn resolve_prompts(&mut self, builtins: &BTreeMap<String, Agent>);
}
```

Removed: `PhaseMap`, `builtin_phases`. `builtin_agents` keep their prompts as `Some`: claude
`/ship-feature {request}`, codex `$ship-feature {request}`, master `""`. `add_builtins` calls
`resolve_prompts` with them before it merges the file's agents over them.

`src/adapter.rs`, `adapter/claude_code.rs`, `adapter/codex.rs` (modified):

```rust
impl Adapter {
    pub fn translate(self, payload: &Value, workflow: &Workflow) -> Vec<EventKind>;
}
pub fn claude_code::translate(payload: &Value, workflow: &Workflow) -> Vec<EventKind>;
pub fn codex::translate(payload: &Value, workflow: &Workflow) -> Vec<EventKind>;
```

Removed: `adapter::phase_of` (now `Workflow::phase_of`).

Commands (modified):

```rust
// start: Started { ..., phases: Some(config.workflow.phases()) }; start/plain: phases: None
pub fn new::run(args: Args, store: &Store, workflow: &Workflow, now: u64) -> Result<Document, Failure>;
// report: Phase { phase: Phase } via FromStr; refuses a phase the flow's steps lack:
fn no_phase(flow: &FlowView, phase: &Phase) -> Failure; // "shop-221 has no phase deploy", help "Phases: plan, ..."
// hook: adapter.translate(payload, &config.workflow)
// cli.rs: Command::New loads env.config()? like Start
```

`cli/output/flow.rs` (modified): `created`'s help names the flow's first phase ("Run `flow report
--flow shop-221 phase plan` when plan starts"). `reported` and the status rows take `&'a str` phase
names, since `Phase::name` borrows.

`src/ui/theme.rs` (modified):

```rust
/// The phases' colors: by position (the built-in themes' four), and by name from [phase].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhasePalette {
    pub positions: [Color; MAX_PHASES],
    pub names: BTreeMap<String, Color>,
}

pub struct Palette { /* ui and state colors */ pub phases: PhasePalette }  // no longer Copy
// removed: fields design, implement, review, merge_request; PaletteKey::Design..MergeRequest

/// A color a theme file sets: a fixed key of [ui] or [state], or a phase by name in [phase].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ColorKey { Fixed(PaletteKey), Phase(Phase) }

impl Palette {
    pub fn color(&self, key: &ColorKey) -> Color;
    pub fn set(&mut self, key: &ColorKey, color: Color);   // replaces color_mut
}

pub struct PhaseTokens {
    pub positions: [Style; MAX_PHASES],
    pub names: BTreeMap<String, Style>,
}

impl Theme {
    /// The phase's color by name, else by its position among the flow's steps.
    pub fn phase_style(&self, phase: &Phase, position: usize) -> Style;
}
```

`tinted(flow)` finds the position in `flow.steps`. `Section::Phase` stays and holds names.

`src/ui/theme_file.rs` (modified): `Entry { key: ColorKey, value, line }`; `colors` reads any phase
name in `[phase]` ("design" reads as "plan", a bad name errors "theme.toml:4: Build is no phase
name"); `apply` resolves references by `ColorKey`.

`ui/board/cards.rs` (modified): a delegated flow reads `"{phase} · agent"`; `gerund` goes.
`ui/board/detail.rs` (modified): `merge-request` keeps the short label "MR", matched by name.

`launch/pane.rs` (modified): `agent_argv` fills `agent.prompt.as_deref().unwrap_or_default()`.
`server/master.rs` (modified): the fresh master strips the prompt with `prompt: None`.

## Data

- config.toml `[[phases]]`: `name` (string), `skills` (list of strings).
- config.toml `[worker] prompt`: string with `{request}`, `{name}`, `{id}`.
- `started` line: `"phases":["plan","implement","review","merge-request"]`, left out when None.
- `--json`: `version: 3`; `phase` and `steps[].phase` are any configured name, never `design`.
- theme.toml `[phase]`: phase name -> color.

## Call chains

flow start:
1. cli::run -> Env::config(): Result<Config>
2. Config::load -> Config::parse(text) -> Workflow::try_from(Vec<PhaseSpec>)
3. Error: old [phases] table, more than 4 or none, a duplicate name or skill, a bad name -> anyhow
   with path:line -> Failure, exit 1, nothing created
4. start::run -> config.workflow.phases(): Vec<Phase>
5. start::run -> Event::new(now, Started { name, request, mode, phases: Some(..) })
6. start::run -> store.create(&first): id; the rest as today

A skill starts a phase:
1. hook::run -> adapter.translate(payload, &config.workflow): Vec<EventKind>
2. claude_code::translate -> workflow.phase_of("buzzwoo:design-code"): Some(Phase "plan")
3. hook::run -> fold::changes(id, events, Phase{plan}, now): bool
4. Folding::position(&plan): Some(0) -> enter(0, at)
5. hook::run -> store.append when it changed
6. The flow started under another workflow and lacks the phase -> position None -> changes false ->
   nothing appended

flow report phase:
1. clap -> Phase::from_str("review"); a bad name -> usage error, exit 2
2. report::run -> fold::resolve(flows, name): &FlowView
3. No step with that phase -> no_phase(flow, phase): Failure, exit 1
4. report::run -> store.append; fold::fold -> output::flow::reported

An old log:
1. fold::fold -> Started { phases: None } -> Folding::new(.., None, at): builtin phases, legacy
2. apply(Phase{design}) -> position(legacy_name(design) = plan): Some(0)
3. finish -> steps plan, implement, review, merge-request

The UI colors a working flow:
1. theme_file::parse: [phase] build = "#7aa7ff" -> Entry { ColorKey::Phase(build) }
2. apply -> palette.set(&ColorKey::Phase(build), color)
3. Theme::from_palette -> PhaseTokens { positions, names }
4. labels::steps -> theme.step_style(flow) -> tinted -> phase_style(&build, 1): names[build], else
   positions[1]

The first prompt:
1. Config::load -> add_builtins -> resolve_prompts(&builtin_agents())
2. [worker] prompt unset: claude keeps "/ship-feature {request}", codex "$ship-feature {request}"
3. [worker] prompt = "/my-flow {request}": claude and codex both get it as written; an
   [agents.aider] with prompt = "{request}" keeps its own
4. server panes -> pane::worker_spec -> agent_argv fills the request

## Test seams

- `workflow.rs`: builtin, phases, phase_of (slash and plugin prefix), each try_from error.
- `event.rs`: Phase name rules; Started round-trips with and without phases; an old line parses.
- `fold/tests.rs`: a custom 3-phase flow's steps; design -> plan only without a list; an unknown
  phase ignored; MR enters the last phase.
- `config.rs`: [[phases]] read, limits, old [phases] refused; the prompt order (own,
  [worker], built-in default, none).
- `adapter`: translate fixtures with a Workflow.
- `theme.rs`, `theme_file.rs`: color by name, by position, the "design" alias, a bad name.
- `tests/cli.rs`: start and new record phases; report refuses an unknown phase; `status --json`
  version 3. Goldens rewritten where they showed "designing" or "design".

## Assumptions

1. `[[phases]]` replaces the built-in workflow whole.
2. A phase event a flow has no phase for is ignored by fold.
3. A merge-request event enters the flow's last phase.
4. A phase without a theme color takes the built-in color of its position.
5. A `[phase]` color can name a `[ui]` or `[state]` key; nothing can name a phase's color.
6. A delegated card says "review · agent", no gerunds.
7. An `[agents.claude]` in the file without a prompt and no `[worker] prompt` keeps the built-in
   claude prompt.
8. `flow new` reads config.toml for the workflow (and fails on a broken one).

## Tasks

- [ ] Add `event::Phase` as a validated name (TryFrom, FromStr, name) and `Started.phases`, with tests for the name rules, round-trips and an old started line; adapt every `Phase::X` use to names so the crate compiles
- [ ] Add `workflow.rs` (`PhaseSpec`, `Workflow`, `builtin`, `phases`, `phase_of`, `MAX_PHASES`) with unit tests for each try_from error
- [ ] Config reads `[[phases]]` into `Config.workflow`, refuses the old `[phases]` table naming `[[phases]]`; adapters translate with `&Workflow`; remove `PhaseMap`, `builtin_phases`, `adapter::phase_of`
- [ ] Fold builds steps from `Started.phases` (builtin and the design -> plan rename without them), ignores unknown phases, MR enters the last phase; fold tests for each
- [ ] `flow start` and `flow new` record the workflow's phases (plain records none); `flow report phase` takes any name and refuses one the flow lacks; `created` help names the first phase; cli tests
- [ ] `VIEW_VERSION` 3; README's JSON example and the cli test that checks the version
- [ ] Theme: `PhasePalette`, `ColorKey`, `Palette::color/set`, `PhaseTokens`, `Theme::phase_style(phase, position)`; theme_file reads any phase name in `[phase]`, "design" as "plan"; theme tests
- [ ] Cards say "{phase} · agent"; detail keeps "MR" by name; goldens updated
- [ ] `[worker] prompt: Option`, `Agent.prompt: Option`, `resolve_prompts` (own, then [worker], then the built-in default of its name); `agent_argv` and the master's bare start; config tests for each step of the order
- [ ] README: `[[phases]]`, `[worker] prompt`, phase names in `flow report`, theme `[phase]` by name, JSON v3; HANDOVER and CLAUDE.md where they name the phases or `PhaseMap`
- [ ] End to end: a scratch `FLOW_HOME` and `FLOW_CONFIG` with a 3-phase workflow; `flow new`, `flow report phase`, `flow status` and `--json` show its steps; an old log with `design` reads as plan; `flow ui` on the scratch store shows the steps colored by a theme `[phase]` entry; `flow start` builds the worker's argv with the built-in prompts (claude `/`, codex `$`), then with `[worker] prompt` set for both as written
