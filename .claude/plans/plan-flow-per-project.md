# One flow per project: shared slots and an overview of every project

Implementation page (review surface): `.lavish/projects-implementation.html`; approaches: `.lavish/projects-approaches.html`.

## Decision

- Approach A: every store registers itself in the shared folder (`~/.local/state/flow`, the default store's
  folder); `flow start` claims its slot as a file in `slots/` under a flock held for milliseconds, before
  setup. A claim only counts while its flow is unfinished in its store, so it cleans itself up.
- Decided: a registry (stores can live anywhere), 20 slots across all projects, the overview in
  `flow status --all` and in flow ui's board (key `a`).

## Units

```rust
// src/registry.rs (new)
/// The shared folder: $XDG_STATE_HOME/flow or ~/.local/state/flow, whatever FLOW_HOME says.
pub struct Registry { root: PathBuf }
impl Registry {
    pub fn locate(xdg_state: Option<PathBuf>, home: Option<PathBuf>) -> Result<Registry>;
    pub fn root(&self) -> &Path;
    /// Writes stores/<fnv1a of the root> when it is missing; cheap enough for every command.
    pub fn register(&self, store: &Store) -> Result<()>;
    /// Every registered store whose folder still exists, sorted by project name; a vanished one is dropped.
    pub fn stores(&self) -> Vec<Store>;
}
/// A store's project name: its folder's name, "default" for the shared root.
pub fn project_name(store: &Store, registry: &Registry) -> String;

// src/slots.rs (new)
pub const MAX_SLOTS: u8 = 20;   // moved from launch.rs, raised from 9
#[derive(Serialize, Deserialize)]
pub struct Claim { pub store: PathBuf, pub flow: String }
pub struct Slots { dir: PathBuf }   // <registry root>/slots
impl Slots {
    pub fn at(registry: &Registry) -> Slots;
    /// Under an exclusive flock on slots/.lock: the lowest slot neither in `held` nor claimed by a flow
    /// `live` says is unfinished, written as this claim; None when all 20 are taken.
    pub fn claim(&self, claim: &Claim, held: &[u8], live: impl Fn(&Claim) -> bool) -> Result<Option<u8>>;
    /// Gives a claim back when flow start fails after claiming.
    pub fn release(&self, slot: u8, claim: &Claim) -> Result<()>;
}

// src/main.rs
// run(): Registry::register(store) for every command after Store::locate; a failure is ignored.
Status { flow, json, full, #[arg(long, conflicts_with = "flow")] all: bool }
/// name -> Store::create (id) -> held_slots -> Slots::claim -> workspace_for -> create_worktree -> setup
/// -> Workspace event. A failed worktree removes the log and releases the claim.
fn start(store, registry: &Registry, config, cwd, launch, now) -> Result<Document, Failure>;
fn claim_is_live(claim: &Claim, now: u64) -> bool;
fn held_slots(registry: &Registry, now: u64) -> Vec<u8>;
fn status_all(registry: &Registry, json: bool, full: bool, now: u64) -> Result<Document, Failure>;

// src/view.rs
pub struct Projects { pub version: u32, pub now: u64, pub projects: Vec<Project> }
pub struct Project { pub name: String, pub root: String, pub board: Board }
// src/output.rs
/// count, needs_you, flows[n]{project,name,state,phase,tasks,age}, blockers[n]{project,name,blocker}, help.
pub fn status_all(projects: &Projects, full: bool) -> Document;
// src/launch.rs: MAX_SLOTS moves to slots.rs; free_slot stays the dialog's preview
pub fn taken_slots(flows: &[FlowView]) -> Vec<u8>;

// src/ui/app.rs
pub enum Job { Start(NewFlowForm), Remove(String), Overview }   // `flow status --all --json`
// BoardState gains `all: bool`; `a` toggles it; while all, Job::Overview on open and every 2 s.
// Other projects' cards are read-only.
// src/ui/board.rs
pub struct BoardView<'a> { /* as now */ pub projects: Option<&'a Projects> }
```

## Data

- `~/.local/state/flow/stores/<16 hex>`: the store's root path, one line.
- `~/.local/state/flow/slots/<n>`: `{"store": "...", "flow": "<id>"}`; `slots/.lock`: empty, flocked.
- `flow status --all --json`: `{"version": 1, "now": ..., "projects": [{"name", "root", "board": <Board>}]}`.
- Flow logs unchanged; the slot still lives in the Workspace event.

## Call chains

1. flow start: `main::start -> launch::flow_name`, `Store::create(Started): id -> held_slots(registry, now)
   -> Slots::claim(&Claim{store, flow: id}, &held, |c| claim_is_live(c, now)): Option<u8> -> launch::workspace_for
   -> workspace::create_worktree -> run_hook(Setup) -> Store::append(Workspace)`.
   - None free: `Store::remove(id)`, Failure "all 20 slots are in use" (help: `flow status --all`).
   - Worktree fails: `Slots::release`, `Store::remove(id)`, the git error.
2. `flow status --all -> Registry::stores() -> per store Store::logs -> fold::views -> fold::board ->
   view::Projects -> output::status_all` (TOON) or Projects as JSON. Unreadable logs become warnings.
3. flow ui: `a` on the board -> `Effect::Run(Job::Overview)` -> thread runs `<exe> status --all --json` ->
   `App::on_ran` parses Projects -> `BoardView { projects: Some(..) }`; again every 2 s while open. A failed
   job keeps the last overview and shows a notice.

## Test seams

- Registry: temp shared root; register twice is one file; a vanished store is dropped; project names.
- Slots::claim: lowest free; held and live claims skipped; a dead claim reused; all taken is None; two
  threads at once get different slots.
- tests/cli.rs: two projects (two FLOW_HOME, one XDG_STATE_HOME) get slots 1 and 2; `status --all` lists both.
- output::status_all, BoardView with projects: rows.

## Assumptions

1. The registry lives in the default store's folder, which is also a store, registered as "default".
2. Every flow command registers its store.
3. Flows started before this change hold their slot through their log (held_slots); no claim file needed.
4. Project name is the store folder's name; duplicates get "name (2)".
5. The dialog's slot preview stays per project; flow start decides.
6. flow ui's overview refreshes every 2 s by running the CLI; flow server unchanged.
7. flow start's help drops "in a tmux window".

## Tasks

- [ ] Add registry.rs (Registry::locate, root, register, stores, project_name) with temp-dir tests
- [ ] Add slots.rs (MAX_SLOTS 20, Claim, Slots::at, claim, release) with tests incl. two threads at once; launch::taken_slots
- [ ] main: register every command's store; start claims before the worktree (held_slots, claim_is_live, release on failure); help text; cli test for two projects getting slots 1 and 2
- [ ] view::Projects, output::status_all, `flow status --all` (TOON and --json) with tests
- [ ] flow ui: Job::Overview, BoardState.all, key `a`, BoardView.projects, refresh every 2 s, read-only other projects; tests
- [ ] Docs: README, CLAUDE.md module map, HANDOVER
- [ ] End-to-end check: two tmux project sessions (private socket), a flow started in each gets distinct slots; `flow status --all` and flow ui `a` show both projects
