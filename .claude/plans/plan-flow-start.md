# flow start: a flow, a worktree, a stack slot and a worker window

Implementation page (review surface): `docs/design/start-implementation.html`.
System context: `docs/HANDOVER.md` (piece 5 of 6). Core plan: `plan-flow-core.md`.

## Decision

- `flow start <request>` creates the flow, a git worktree of its own and a port slot, runs the
  repo's optional setup hook in the worktree, then opens a tmux window there running the worker
  agent from a launch template, with `FLOW_ID` set. `flow clear` runs the teardown hook and
  removes the worktree of a finished flow.
- `flow` makes the worktree; an agent is a command template (Claude Code built in, others in
  `~/.config/flow/config.toml`); `flow` stays agent- and Docker-agnostic.
- Docker: tests run inside the containers, so every worktree gets its own stack. The setup hook
  gets a slot (1-9) and turns it into ports, starts the stack and migrates. Compose names a
  project after its folder, so `docker compose exec` in a worktree reaches its own stack. Each
  project once needs its ports from variables (`"${HTTP_PORT:-80}:80"`).
- Claude workers start with `/ship-feature {request}` (linked into `~/.claude/skills`).

## Commands

```
flow start <request> [--name <name>] [--mode guided|auto] [--agent <name>]   in a git repo, inside tmux
flow clear                                                                  also tears worktrees down
```

## Units

### src/launch.rs (new, pure)

```rust
pub const MAX_SLOTS: u8 = 9;
pub const WORKTREES_DIR: &str = ".flow/worktrees";

/// Ticket key lowercased (SHOP-217 -> shop-217), else the first four words as a slug; -2, -3 ... when taken.
pub fn flow_name(request: &str, taken: &[String]) -> String;
/// The lowest slot 1..=MAX_SLOTS no unfinished flow holds; None when all are taken.
pub fn free_slot(flows: &[FlowView]) -> Option<u8>;
/// <repo>/.flow/worktrees/<name> and branch flow/<name>.
pub fn workspace_for(repo: &Path, name: &str, slot: u8, agent: &str) -> Workspace;
/// git -C <repo> worktree add -b <branch> <path>
pub fn worktree_add_argv(repo: &Path, workspace: &Workspace) -> Vec<String>;
/// git -C <repo> worktree remove <path>  (no --force: a dirty worktree is kept)
pub fn worktree_remove_argv(repo: &Path, workspace: &Workspace) -> Vec<String>;
/// The agent's command with {id}, {name}, {prompt}, {request} filled in ({prompt} itself filled first).
pub fn agent_argv(agent: &Agent, id: &str, name: &str, request: &str) -> Vec<String>;
/// tmux new-window -d -P -F '#{window_id}' -n <name> -c <path> -e K=V ... -- <agent argv>
pub fn window_argv(workspace: &Workspace, name: &str, env: &[(String, String)], agent: &[String]) -> Vec<String>;
/// FLOW_ID, FLOW_NAME, FLOW_SLOT, FLOW_WORKTREE, FLOW_REPO for the hooks.
pub fn hook_env(id: &str, name: &str, repo: &Path, workspace: &Workspace) -> Vec<(String, String)>;
```

### src/workspace.rs (new, I/O)

```rust
pub enum Hook { Setup, Teardown }   // .flow/setup, .flow/teardown

/// git rev-parse --show-toplevel in cwd.
pub fn repo_root(cwd: &Path) -> Result<PathBuf>;
/// Creates the worktree and branch; adds .flow/worktrees/ to .git/info/exclude once.
pub fn create_worktree(repo: &Path, workspace: &Workspace) -> Result<()>;
/// Runs <repo>/.flow/<hook> in the worktree with env when it exists and is executable; Ok(false) when none.
/// A non-zero exit is an Err with the last stderr line.
pub fn run_hook(repo: &Path, hook: Hook, workspace: &Workspace, env: &[(String, String)]) -> Result<bool>;
/// Opens the worker window; returns its id (@14). Needs $TMUX (checked by main).
pub fn open_window(argv: &[String]) -> Result<String>;
/// Removes a clean worktree; Ok(false) when git refuses (uncommitted work).
pub fn remove_worktree(repo: &Path, workspace: &Workspace) -> Result<bool>;
```

### src/config.rs (new)

```rust
#[derive(Debug, Clone, Deserialize)]
pub struct Agent { pub command: Vec<String>, pub prompt: String }

#[derive(Debug, Deserialize)]
pub struct Config {
    #[serde(default = "default_agent_name")] pub agent: String,          // "claude"
    #[serde(default)] pub agents: BTreeMap<String, Agent>,               // over the built-in ones
}

impl Config {
    /// $FLOW_CONFIG, else $XDG_CONFIG_HOME/flow/config.toml, else ~/.config/flow/config.toml; built-ins when absent.
    pub fn load(flow_config: Option<PathBuf>, xdg_config: Option<PathBuf>, home: Option<PathBuf>) -> Result<Config>;
    /// The named agent, or the default one.
    pub fn agent(&self, name: Option<&str>) -> Option<(&str, &Agent)>;
}

/// claude: ["claude", "-n", "{name}", "--session-id", "{id}", "{prompt}"], prompt "/ship-feature {request}".
pub fn builtin_agents() -> BTreeMap<String, Agent>;
```

New dependency: `toml`.

### src/main.rs (modified)

```rust
/// Start a worker: a flow, a worktree, the setup hook and an agent in a tmux window
Start { request: String, #[arg(long)] name: Option<String>, #[arg(long, value_enum, default_value = "guided")] mode: Mode, #[arg(long)] agent: Option<String> },

/// What flow start was asked for (bundled while building, instead of four parameters).
struct Launch { request: String, name: Option<String>, mode: Mode, agent: Option<String> }

fn start(store: &Store, config: &Config, cwd: &Path, in_tmux: bool, flow_home: Option<PathBuf>,
         launch: Launch, now: u64) -> Result<Document, Failure>;
fn append_workspace(store: &Store, id: &str, space: &view::Workspace, now: u64) -> Result<(), Failure>;
fn clear(store: &Store, now: u64) -> Result<Document, Failure>;   // + teardown and worktree removal
/// The repository above <repo>/.flow/worktrees/<name> (added while building).
fn repo_of(space: &view::Workspace) -> Option<PathBuf>;
```

### event.rs, fold.rs, view.rs, output.rs (modified)

```rust
// event.rs
Workspace { path: String, branch: String, slot: u8, window: Option<String>, agent: String },

// view.rs (--json, additive)
#[serde(rename_all = "camelCase")]
pub struct Workspace { pub path: String, pub branch: String, pub slot: u8, pub window: Option<String>, pub agent: String }
pub struct FlowView { /* ... */ pub workspace: Option<Workspace> }   // the last Workspace event

// output.rs
/// flow, id, worktree, branch, slot, setup (ran / none / failed), agent, window, help.
pub fn started(flow: &FlowView, setup: &str) -> Document;
/// cleared: n, names[n], kept[n]{name,path} for dirty worktrees, help.
pub fn cleared(names: &[String], kept: &[(String, String)]) -> Document;
```

`launch.rs` uses `view::Workspace` directly.

## Call chains

### flow start "SHOP-217 add CSV export"
1. main -> Command::Start -> Config::load(env FLOW_CONFIG, XDG_CONFIG_HOME, HOME) -> config.agent(--agent)
2. unknown agent -> Failure "no agent named x", help "Agents: claude, ..."; broken config -> Failure naming the file and the TOML error
3. $TMUX unset -> Failure "flow start opens the worker in a tmux window - run it inside tmux"
4. workspace::repo_root(cwd); not a repo -> Failure "flow start needs a git repository"
5. views(store) -> launch::flow_name(--name or request, unfinished names); launch::free_slot(views); none -> Failure "all 9 slots are in use", help "Run `flow clear` after finished flows"
6. launch::workspace_for(repo, name, slot, agent) -> workspace::create_worktree; git error -> Failure with git's line, nothing recorded
7. store.create(Started { name, request, mode }) -> id; store.append(id, Workspace { window: None })
8. workspace::run_hook(repo, Setup, &ws, hook_env) -> "ran" / "none"; Err -> store.append(id, Blocked "setup failed: <line>"), setup "failed: <line>", go on
9. launch::agent_argv(agent, id, name, request) -> launch::window_argv(&ws, name, [FLOW_ID=id, FLOW_HOME=..when set], argv) -> workspace::open_window -> "@14"
10. tmux fails -> store.append(id, Ended), Failure with tmux's line (worktree kept)
11. store.append(id, Workspace { window: Some("@14") }) -> fold -> output::started(&flow, setup)

### flow clear
1. views -> finished flows; with a workspace: repo = path up to `.flow/worktrees`; run_hook(Teardown) (an error is noted, clear goes on); remove_worktree -> false -> kept
2. store.remove(id) for every finished flow -> output::cleared(names, kept); the branch always stays

## Test seams

- launch.rs unit: names (ticket, words, collisions), slots (lowest free, finished free theirs, full), paths, every argv exactly, template filling, hook env.
- config.rs unit: built-ins only, a file adding an agent and overriding claude's prompt, a broken file, precedence.
- fold.rs unit: workspace on the view, the last one wins.
- tests/cli.rs: a temp git repo, a fake `tmux` on PATH that prints @14 and records argv, a `.flow/setup` writing a file: start, outside tmux, outside a repo, a failing hook, a full slot table, clear removing a clean worktree and keeping a dirty one.

## Assumptions

1. Worktrees in <repo>/.flow/worktrees/<name> on branch flow/<name> from the current HEAD; `.flow/worktrees/` in .git/info/exclude.
2. Hooks .flow/setup and .flow/teardown in the repo, run in the worktree with FLOW_ID, FLOW_NAME, FLOW_SLOT, FLOW_WORKTREE, FLOW_REPO.
3. Nine slots per machine.
4. The window opens in the current tmux session in the background; FLOW_HOME is passed on when set.
5. A failed setup marks the flow blocked but still opens the window.
6. Workers run in the default permission mode.
7. clear keeps a dirty worktree and always the branch.
8. The name follows the board's rule.

## Decided in review

- Approach A; Docker through repo hooks and a slot; `/ship-feature {request}`; command surface as proposed; `.flow/` in the repo; TOML config.

## Tasks

- [x] Add `EventKind::Workspace`, `view::Workspace`, `FlowView.workspace` and fold it, with unit tests (last one wins, JSON shape)
- [x] Add `launch.rs` (`flow_name`, `free_slot`, `workspace_for`, `worktree_add_argv`, `worktree_remove_argv`, `agent_argv`, `window_argv`, `hook_env`) with unit tests
- [x] Add `config.rs` with the `toml` crate (`Config::load`, `agent`, `builtin_agents`) with unit tests
- [x] Add `workspace.rs` (`repo_root`, `create_worktree`, `run_hook`, `open_window`, `remove_worktree`) with tests on a temp git repo
- [x] Add `flow start` (`start`, `output::started`) with tests/cli.rs cases using a fake tmux: start, outside tmux, outside a repo, failing hook, full slots
- [x] Extend `flow clear` with teardown and worktree removal (`output::cleared` with kept) with tests/cli.rs cases for clean and dirty worktrees
- [x] Update README, CLAUDE.md and the handover: `flow start`, hooks, slots, a Docker hook example
- [x] End-to-end: in a real repo inside tmux, `flow start` a tiny request, watch the worker run /ship-feature with the board in the master, then finish or end it and `flow clear`
