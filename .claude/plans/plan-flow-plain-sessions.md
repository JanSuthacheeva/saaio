# Plan: plain sessions

Review page: https://claude.ai/artifact/5MgfSYc5X9MGDNqe2m5ib4

## Decision

Open an agent in a folder you name, for tinkering: no worktree, no `flow/<name>` branch, no
`/ship-feature` prompt, no port slot. It shows in the sidebar and resumes after a restart like any
worker.

- Plain is a third `Mode` (`Mode::Plain`, `"plain"`). A plain session's `Workspace` has the folder as
  `path` and `branch: None`, `slot: None`.
- `flow start --plain <folder>`; a request, `--mode` and `--type` are refused with it (clap, exit 2).
  `--name` and `--agent` work. `Mode::Plain` is not a `--mode` value.
- No first prompt: the agent opens at an empty prompt.
- Plain sessions are the user's own: no worker system prompt, no `flow wait` hook, they never wake
  the master, `flow send` refuses them. The reporting hooks stay, so state glyphs work.
- The new-flow dialog's mode row is auto, guided, plain; in plain mode the text box edits the folder.
- The server does not change: it already opens every unfinished flow with a workspace through
  `worker_spec` and resumes it once `window = "flow-ui"`.
- `VIEW_VERSION` and `PROTOCOL_VERSION` go to 2 (decided in review): `branch` and `slot` can be null.

## Units

`src/event.rs` (modified)

```rust
pub enum Mode {
    #[default]
    Guided,
    Auto,
    /// An agent in a folder: no worktree, branch, slot, prompt or master.
    #[value(skip)]
    Plain,
}

impl Mode {
    /// auto, guided, plain - the dialog's order.
    pub const ALL: [Mode; 3] = [Mode::Auto, Mode::Guided, Mode::Plain];
    pub fn name(self) -> &'static str; // + "plain"
    // toggled() is removed: the dialog cycles through ALL
}

EventKind::Workspace {
    path: String,
    branch: Option<String>, // None for a plain session
    slot: Option<u8>,       // None for a plain session
    window: Option<String>,
    agent: String,
}
```

`src/view.rs` (modified)

```rust
pub const VIEW_VERSION: u32 = 2;

/// Where a started flow's agent runs: its worktree or, for a plain session, its folder; branch and
/// port slot (none for a plain session), window and agent.
pub struct Workspace {
    pub path: String,
    pub branch: Option<String>,
    pub slot: Option<u8>,
    pub window: Option<String>,
    pub agent: String,
}

impl FlowView {
    /// Started with --plain: no phases, no master.
    pub fn is_plain(&self) -> bool;
}
```

`src/proto.rs` (modified): `PROTOCOL_VERSION = 2`.

`src/fold.rs` (modified): `finish` leaves `steps` empty for a plain session. Phase events still set
`phase`.

`src/wake.rs` (modified)

```rust
/// Flows whose state needs the master and that changed since it last saw them, oldest first;
/// never a plain session.
pub fn changed_flows<'a>(flows: &'a [FlowView], seen: &Seen) -> Vec<&'a FlowView>;
```

`src/slots.rs` (modified): `taken_slots` reads `workspace.slot` through `and_then`.

`src/launch/worktree.rs` (modified)

```rust
/// <repo>/.flow/worktrees/<name> on `branch` (Some(branch), Some(slot)).
pub fn workspace_for(repo: &Path, name: &str, branch: String, slot: u8, agent: &str) -> Workspace;

/// A plain session's workspace: `folder`, no branch, no slot.
pub fn folder_workspace(folder: &Path, agent: &str) -> Workspace;

/// The repository a flow worktree belongs to; None for a plain session (no branch), even one opened
/// inside a worktree, so remove never tears down a folder it did not create.
pub fn repo_of(space: &Workspace) -> Option<PathBuf>;

/// FLOW_ID, FLOW_NAME, FLOW_SLOT (when it has one), FLOW_WORKTREE, FLOW_REPO for the hooks.
pub fn hook_env(id: &str, name: &str, repo: &Path, workspace: &Workspace) -> Vec<(String, String)>;

/// git -C <repo> worktree add -b <branch> <path>; workspace_for always sets the branch.
pub fn worktree_add_argv(repo: &Path, workspace: &Workspace) -> Vec<String>;
```

`src/launch/pane.rs` (modified)

```rust
/// The agent's command with {id} and {name} filled in and {prompt} and {request} dropped:
/// a plain session opens at an empty prompt.
pub fn plain_argv(agent: &Agent, id: &str, name: &str) -> Vec<String>;

/// As today, plus: a plain session runs plain_argv fresh (resume_argv once hosted) and gets the
/// adapter's plain_wiring instead of its wiring.
pub fn worker_spec(flow: &FlowView, agent: &Agent, env: &LaunchEnv) -> Option<PaneSpec>;
```

`worker_env` is unchanged: with `repo_of` None it gives FLOW_ID + `pane_env`.

`src/adapter.rs`, `src/adapter/claude_code.rs` (modified)

```rust
impl Adapter {
    /// The arguments that make a plain session report to `<flow_exe> hook <name>`: no master
    /// waiter, no worker prompt.
    pub fn plain_wiring(self, flow_exe: &Path) -> Vec<String>; // ["--settings", plain_settings]
}

// claude_code.rs
/// The reporting hooks alone: each event in EVENTS runs `<flow_exe> hook claude-code`.
pub fn plain_settings(flow_exe: &Path) -> String;
/// report_hooks plus `<flow_exe> wait` on WAIT_EVENTS, as today.
pub fn settings(flow_exe: &Path) -> String;
fn report_hooks(flow_exe: &Path) -> serde_json::Map<String, Value>;
```

`src/cli/commands/start.rs` (modified)

```rust
pub struct Args {
    /// What to build, e.g. "SHOP-217 add CSV export"
    #[arg(required_unless_present = "plain")]
    request: Option<String>,
    /// Open the agent in this folder instead: no worktree, branch, slot or prompt
    #[arg(long, value_name = "FOLDER", conflicts_with_all = ["request", "mode", "kind"])]
    plain: Option<PathBuf>,
    // name, mode, agent, kind unchanged
}

/// Starts a worker: a flow, a worktree, the setup hook, and the agent flow ui opens;
/// with --plain, plain::open.
pub fn run(args: &Args, store: &Store, registry: &Registry, config: &Config, cwd: &Path,
           home: Option<&Path>, now: u64) -> Result<Document, Failure>;

/// The config's agent `asked` names (or its default); the failure lists the agents.
fn pick_agent<'a>(config: &'a Config, asked: Option<&str>) -> Result<&'a str, Failure>;

/// Whether flow ui opens the new agent now or once it runs.
fn opening(store: &Store) -> Opening;

/// The names unfinished flows hold; a new flow takes another.
fn live_names(flows: &[FlowView]) -> Vec<String>;
```

`src/cli.rs` passes `env.user_home()` to `start::run`.

`src/cli/commands/start/plain.rs` (new)

```rust
/// Started{mode: plain} and a Workspace with the folder; flow ui opens the agent there.
/// (Built taking `args` for --name and --agent, to stay within clippy's seven arguments.)
pub(super) fn open(folder: &Path, args: &Args, store: &Store, config: &Config, cwd: &Path,
                   home: Option<&Path>, now: u64) -> Result<Document, Failure>;

/// `given` made absolute: a leading ~ from $HOME (the dialog passes it unexpanded), relative to
/// `cwd`; fails unless it is an existing folder.
fn resolve(given: &Path, cwd: &Path, home: Option<&Path>) -> Result<PathBuf, Failure>;
```

`src/cli/commands/send.rs` (modified): refuses a plain session, `"{name} is a plain session; it is
not the master's"`, help `"Plain sessions are the user's own"`.

`src/cli/commands/retire.rs` (modified): `branch_of` reads `space.branch.as_deref()?`;
`refuse_uncommitted_work` checks only a worktree remove tears down (`repo_of` Some), so a plain
session's dirty folder does not block its remove (found in the build).

`src/cli/output/start.rs` (modified)

```rust
/// flow, id, folder, agent, pane, help.
pub(crate) fn opened(flow: &FlowView, workspace: &Workspace, opening: Opening) -> Document;
// started: branch and slot read from the Options (always Some for a worktree flow)
```

`src/cli/output/status.rs` (modified): `detail` shows `folder` for a plain session in place of
`branch`, `worktree` and `worktree_exists`, and no "commits stay on" help.

`src/ui/newflow.rs` (modified)

```rust
pub struct NewFlowForm {
    pub request: String,
    /// The folder a plain session opens in; prefilled with the place in view (~/projects/shop).
    pub folder: String,
    pub mode: Mode,
    // repo, error, branching, kind, field unchanged
}

impl NewFlowForm {
    pub fn new(repo: PathBuf, place: String) -> NewFlowForm;
    /// The text the box edits: the folder for a plain session, else the request.
    fn text_mut(&mut self) -> &mut String;
    pub fn text(&self) -> &str;
    /// The type row shows only outside plain mode.
    fn extra_rows(&self) -> u16;
}

impl NewFlowView<'_> {
    fn header(&self, width: usize) -> Line<'static>;  // "new session" when plain
    fn mode_row(&self) -> Line<'static>;               // auto  guided  plain, and the picked one's hint
    fn creates(&self, width: usize) -> Line<'static>; // plain: "opens in <folder> · no worktree, branch or slot"
}
```

`NewFlowView` moved to `src/ui/newflow/view.rs` (re-exported), since the dialog passed 400 lines
(done in the build). Ctrl-w in a folder drops its last folder.

Box label is `request` or `folder`. Left/right on the mode row cycle `Mode::ALL`, wrapping. Tab skips
the type row in plain mode. Submit needs a non-empty `text()`.

`src/ui/app/effect.rs` (modified): `Job::args` for a plain form is `["start", "--plain", folder]`.

`src/ui/app.rs`, `src/ui/app/keys.rs` (modified)

- `open_new_flow` passes `self.tilde(&repo)` as the place.
- `focused_repo`: a focused plain session gives its folder.
- The C-space Q list shows the folder in place of the branch.

`src/ui/app/chrome.rs` (modified): `title_line` shows a plain session's folder (tilde'd) where a worker
shows `branch · slot N`.

`src/ui/board/cards.rs`, `src/ui/board/detail.rs`, `src/ui/sidebar/rows.rs`, `src/ui/peek.rs`
(modified)

- Sidebar second row: no steps; the right side says `plain`, as it says `guided`.
- Card: `· plain` after the label; a finished card's third row shows the folder.
- Board detail facts: mode, folder, age; no branch, slot or empty request.
- `PeekLine::Code { branch: Option<String>, worktree, exists }`; no branch row when None.
  The place row says `folder` then.
- `Sidebar` gains `home: Option<&str>` and `tilde(path)` (`fit::tilde`, moved from `App::tilde`), so
  cards, detail, peek and chrome show `~/scratch` (done in the build).
- Board detail draws no steps column without steps.

`src/server/master.md` (modified): one line, plain sessions are the user's own; leave them alone.

## Data

| Type | Change | Crosses |
|---|---|---|
| `Mode` | + `Plain` (`"plain"`), not a `--mode` value | log, --json, TOON, postcard |
| `EventKind::Workspace` | `branch`, `slot` Option; old lines always have them | log |
| `view::Workspace` | same; `null` for a plain session | --json, postcard |
| `VIEW_VERSION`, `PROTOCOL_VERSION` | 1 -> 2 | --json, the flow ui socket |
| `NewFlowForm` | + `folder: String` | UI |
| `PeekLine::Code` | `branch: Option<String>` | UI |

```
{"v":1,"at":…,"kind":"started","name":"scratch","request":"","mode":"plain"}
{"v":1,"at":…,"kind":"workspace","path":"/Users/jan/scratch","branch":null,"slot":null,"window":null,"agent":"claude"}
```

## Call chains

`flow start --plain ~/scratch`

1. `cli::run -> start::run(args, store, registry, &config, &cwd, env.user_home(), now)`
2. `start::run -> plain::open(folder, args.name, args.agent, store, config, cwd, home, now): Result<Document, Failure>`
3. `plain::open -> start::pick_agent(config, agent): Result<&str, Failure>`
4. `plain::open -> resolve(folder, cwd, home): Result<PathBuf, Failure>`
5. `plain::open -> views(store, now)`, `name::flow_name(name, <folder's last component>, &taken): String`
6. `plain::open -> Store::create(&Event{Started{name, "", Mode::Plain}}): String`
7. `plain::open -> worktree::folder_workspace(&folder, agent): Workspace`, `Store::append(&id, Workspace event)`
8. `plain::open -> fold::fold(..)`, `start::opening(store): Opening`, `output::start::opened(&flow, &space, opening): Document`

Errors:
- Unknown agent: the failure `flow start` gives today, before anything is written.
- Folder missing or a file: `no folder <path>`, help "Name an existing folder", exit 1, nothing written.
- Appending the workspace fails: `Store::remove(&id)`, then the error.
- `--plain` with a request, `--mode` or `--type`: clap usage error, exit 2.

C-space n, plain, enter

1. `App::open_new_flow -> NewFlowForm::new(repo, self.tilde(&repo))`, `Effect::LoadBranching(repo)`
2. Tab to the mode row, right twice: `form.mode = Mode::Plain`; the box edits `form.folder`
3. `NewFlowForm::on_key(Enter) -> FormAction::Submit` when `text().trim()` is not empty
4. `App -> Effect::Run(Job::Start(form))`; `Job::args() -> ["start", "--plain", folder]`, in `Job::dir()` = `form.repo`
5. Ok: the server opens it (next chain). Err: `App::on_ran` puts the message in `form.error` and reopens the dialog.

The server opens it, then resumes it after a restart

1. `StoreWatch -> fold -> reconcile`: an unfinished flow with a workspace and no pane (unchanged)
2. `Panes -> pane::worker_spec(&flow, agent, &env): Option<PaneSpec>`
3. `worker_spec`: no window and plain -> `plain_argv(agent, id, name)`; window set -> `resume_argv`
4. `worker_spec -> Adapter::plain_wiring(&env.flow_exe)`, spliced after the program
5. cwd = the folder; env = FLOW_ID + `pane_env` (no FLOW_SLOT, FLOW_WORKTREE, FLOW_REPO)
6. The server records `window = "flow-ui"`; the next flow ui resumes with `claude --resume <id>` in the folder

Error: an agent without a resume template cannot be reopened; `reconcile::unopenable` shows on its row.

The session stops; flow remove

1. `flow hook claude-code` appends Idle / Waiting as for any worker; the glyph follows.
2. `flow wait --master -> deliver::master_news -> wake::changed_flows`: plain sessions filtered out.
3. `flow remove scratch -> retire::close_pane`; `retire_worktree -> Retired::NoWorktree` (`repo_of`
   None); `drop_branch -> BranchFate::NoBranch`; the log goes. The folder is never touched.

## Test seams

- `event`: plain Started and a null-branch Workspace round-trip; an old Workspace line parses.
- `fold`: a plain session has no steps.
- `wake`: `changed_flows` skips a waiting plain session.
- `launch::pane`: `worker_spec` for plain: no prompt, plain wiring, no FLOW_SLOT; resumes once hosted.
- `launch::worktree`: `repo_of` is None for a plain session inside `.flow/worktrees`.
- `adapter::claude_code`: `plain_settings` has every report hook and no waiter; `plain_wiring` no prompt.
- `ui::newflow`: mode cycles auto/guided/plain; plain edits the folder, hides the type row; submit needs a folder.
- `ui::app`: `Job::args` for plain; golden `new-flow-plain.txt`; a plain session in a sidebar golden.
- `tests/cli.rs`: `start --plain` outside git (no worktree, no slot file, TOON); with a request exits 2;
  missing folder exits 1; `send` refuses; `remove` keeps the folder; board `version` is 2.

## Assumptions

1. The name is the folder's last component, slugged (`-2` when taken); `--name` overrides.
2. The request is stored empty; the folder shows where a worker shows its branch.
3. The folder need not be a git repo; it must exist; flow never creates or deletes it.
4. `flow start` expands a leading `~` itself, since the dialog runs no shell.
5. The dialog's mode order is auto, guided, plain; it opens on guided; it says "new session" when plain.
6. C-space n from a focused plain session prefills its folder.
7. Waiting and blocked plain sessions count as "needs you" like any worker.
8. `master.md` gets one line about plain sessions; README and HANDOVER describe the feature.

## Tasks

### Data

- [x] `Mode::Plain` (`#[value(skip)]`), `name()`; `FlowView::is_plain`; event tests for a plain Started round-trip (`Mode::ALL` and removing `toggled()` moved to the dialog task, whose code first uses them; `fold::finish` moved here, the first user of `is_plain`)
- [x] `Workspace.branch`/`slot` as Option in `EventKind::Workspace` and `view::Workspace`, every reader adjusted (`slots::taken_slots`, `worktree::{workspace_for, hook_env, worktree_add_argv}`, `retire::branch_of`, `output::start::started`, `output::status::detail`, chrome, cards, detail, peek, keys); tests: an old Workspace line parses, a null one round-trips
- [x] `VIEW_VERSION` and `PROTOCOL_VERSION` to 2; test boards use `VIEW_VERSION` instead of `1` (sidebar, app tests, status output tests); `view.rs` test, `tests/cli.rs` board version, README `--json` example with a plain session's workspace
- [x] `fold::finish` leaves steps empty for a plain session, with a fold test (built with the first task)

### Launch

- [x] `repo_of` None without a branch, `hook_env` FLOW_SLOT only with a slot; tests incl. a plain session inside `.flow/worktrees`
- [x] `claude_code::report_hooks`, `plain_settings`, `Adapter::plain_wiring`; tests: every report hook, no waiter, no prompt
- [x] `pane::plain_argv`, `worker_spec` for plain sessions; tests: fresh without prompt and with plain wiring, resumed once hosted, env without FLOW_SLOT

### Commands

- [x] `start::Args` with `--plain` and optional request, `pick_agent` and `opening` lifted out, `start/plain.rs` `open` + `resolve`, `worktree::folder_workspace` (moved here, its first user), `output::start::opened`, `cli.rs` passes home; `tests/cli.rs`: plain start outside git, no worktree or slot file, request + `--plain` exits 2, missing folder exits 1, `~` expanded
- [x] `send` refuses a plain session; `wake::changed_flows` skips plain sessions; `status::detail` shows the folder; tests in `wake` and `tests/cli.rs` (send refused, remove keeps the folder)

### flow ui

- [x] `Mode::ALL`, remove `toggled()`; `NewFlowForm` folder, `text`/`text_mut`, mode cycling through `Mode::ALL`, type row hidden in plain, `new(repo, place)`; `Job::args` for plain; `open_new_flow` place, `focused_repo` for plain; newflow and app tests, golden `new-flow-plain.txt`
- [x] Plain session look: sidebar `plain` tag and no steps, card `· plain` and folder, board detail facts, chrome title folder, C-space Q list, `PeekLine::Code` branch Option; golden with a plain session

### Docs and check

- [ ] `master.md` line on plain sessions; README (`flow start --plain`, the dialog's plain mode); HANDOVER: move plain sessions from Future improvements to a built section
- [ ] End to end: `cargo install --path .`, `flow start --plain` in a scratch folder and from the dialog in a real `flow ui`, see it in the sidebar, quit and restart flow ui and see it resume, `flow remove` it and check the folder is still there
