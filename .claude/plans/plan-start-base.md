# Stacked flows: `saaio start --base <branch>`

Review pages: approaches https://claude.ai/artifact/RnSyJ8aVFLjNYrwPjgF1ai (`docs/design/start-base-approaches.html`),
implementation https://claude.ai/artifact/GbWmPnujMRNu6Vjqw6Wyou (`docs/design/start-base-implementation.html`).

## Decision

- Approach B: `saaio start --base <branch>` and a base row in the new-flow dialog create the
  worktree from the base, recorded on the `Workspace` event; the worker gets `SAAIO_BASE` and one
  sentence in its instructions, so its merge request targets the base.
- When the server learns a flow merged, every unfinished worktree flow of that repository whose
  base is the merged flow's branch gets a `base-merged` event: its base moves to the branch the
  merge went into (gh `baseRefName`, glab `target_branch`), and its worker hears it as news (like a
  master message) telling it to rebase, retarget and push. The server runs no git in worktrees.
- A base not on any local branch is fetched from the remotes (origin first) under a deadline;
  `--no-track` always. Unknown or invalid base: refused before anything is created.
- `--base` takes a branch name only; conflicts with `--plain`.
- `saaio remove` of a base flow does not refuse for it, but names the flows standing on it
  (`stacked[n]` in its output and its unmerged refusal, with a help line).
- `--json` gains `workspace.base` (null without one); `VIEW_VERSION` stays 3.

## Units

### src/workspace/git.rs (modified)

```rust
/// Where git creates a flow's branch: a local branch, or a remote's copy of one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartPoint {
    Local(String),
    Remote { remote: String, branch: String },
}

impl StartPoint {
    /// What git worktree add starts from: `x` or `refs/remotes/origin/x`.
    pub fn rev(&self) -> String;
}

/// The start point for `base`: the local branch, else the first remote (origin first) that has
/// it, fetched with a deadline. Err for a name git refuses as a branch, or one no remote has.
pub fn start_point(repo: &Path, base: &str) -> Result<StartPoint>;

/// Whether git takes `name` as a branch name as it is (no leading `-`, no `@{`, check-ref-format).
pub fn is_branch_name(repo: &Path, name: &str) -> bool;
```

`pick_base` uses `is_branch_name` to pick the help: for an invalid name "Name a branch, e.g.
--base flow/shop-216".

Steps: `git check-ref-format --branch <base>`; `refs/heads/<base>` exists -> Local; else for each
remote (origin first, then `git remote` order) `git fetch --quiet <remote>
+refs/heads/<base>:refs/remotes/<remote>/<base>` (forced, as a remote-tracking ref is, so a rewritten base still fetches) through `process::run` with a deadline; the first
that succeeds -> Remote. A fetch past its deadline counts as not on that remote.

### src/launch/worktree.rs (modified)

```rust
/// git -C <repo> worktree add [--no-track] -b <branch> <path> [<from>]; --no-track with from.
pub fn add_argv(repo: &Path, workspace: &Workspace, from: Option<&str>) -> Vec<String>;

pub const HOOK_VARS: [&str; 6] = [.., "SAAIO_BASE"];
/// ...and SAAIO_BASE when the workspace has a base.
pub fn hook_env(id: &str, name: &str, repo: &Path, workspace: &Workspace) -> Vec<(String, String)>;
```

`workspace_for` and `folder_workspace` set `base: None`.

### src/workspace.rs (modified)

```rust
pub fn create_worktree(repo: &Path, workspace: &Workspace, from: Option<&StartPoint>) -> Result<()>;
```

### src/cli/commands/start.rs (modified)

```rust
pub struct Args {
    ..
    /// Branch the worktree starts from and the merge request targets [default: what the repo has checked out]
    #[arg(long, value_name = "BRANCH", conflicts_with = "plain")]
    base: Option<String>,
}

/// The start point --base names, before anything is created; the failure says how to make it exist.
fn pick_base(repo: &Path, asked: Option<&str>) -> Result<Option<StartPoint>, Failure>;
```

### src/adapter.rs (modified)

```rust
/// WORKER_PROMPT, and for a stacked flow: "Your branch starts from <base>, not the default branch:
/// open your merge request against <base> and rebase on it. Push <base> first if the remote lacks it."
pub fn worker_prompt(base: Option<&str>) -> String;

pub fn wiring(self, flow_exe: &Path, trusted: Option<&Path>, base: Option<&str>) -> Vec<String>;
```

`launch/pane.rs::worker_spec` passes `space.base.as_deref()`, fresh and resumed.

### src/stack.rs (new)

Which flows stand on a flow's branch, and the event each gets when it merged (pure).

```rust
/// The unfinished, unmerged worktree flows of `base`'s repository whose base is `base`'s branch;
/// never `base` itself.
pub fn standing_on<'a>(flows: &'a [FlowView], base: &FlowView) -> Vec<&'a FlowView>;

/// For each flow standing_on `merged`: its id and BaseMerged { base: merged's branch, onto }.
pub fn on_merge(flows: &[FlowView], merged: &FlowView, onto: &str) -> Vec<(String, EventKind)>;
```

### src/forge.rs (modified)

`Found { .., pub target: String }`; gh list and view ask `url,state,createdAt,baseRefName`; glab
reads `target_branch`. gh fixtures re-recorded or extended with `baseRefName`.

### src/server/forge_watch.rs (modified)

```rust
/// ...and, when the news is Merged, stack::on_merge's events appended to the flows on its branch.
pub(super) fn forge_answer(&mut self, answer: Answer);
/// Appends each (id, kind); logs a failure to server.log and goes on.
fn restack(&self, events: Vec<(String, EventKind)>, now: u64);
```

Restacks only after the Merged event was appended, so a failed append retried next round never
restacks twice. Tested end to end in `tests/cli/stack.rs` (a fake gh, as `tests/cli/store.rs`
does; `server_with_gh` moved to `support.rs`) rather than in `server/tests.rs`.

### src/wake.rs, src/deliver.rs (modified)

```rust
/// What a worker hears: the master's message, or saaio's word that its base merged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Note {
    Master(String),
    BaseMerged { base: String, onto: String },
}

/// The notes in a flow's log after the first `delivered` of them (Message and BaseMerged events).
pub fn undelivered(events: &[Event], delivered: usize) -> Vec<Note>;
/// "message from the saaio master:" + text, or "news from saaio:" + what to do after a base merged.
pub fn for_worker(notes: &[Note]) -> String;

pub enum WorkerNews { Gone, Nothing, News(Vec<Note>) }   // deliver.rs
```

The delivered count counts both kinds. `dialog_answered` keeps Message only. Callers:
`cli/commands/wait.rs`, `cli/commands/hook.rs`, `server/news.rs`.

BaseMerged text for the worker: "news from saaio: <base>, the branch this flow stands on, was
merged into <onto>. Rebase your own commits onto it (`git fetch`, then `git rebase --onto
origin/<onto> <base>`, or `origin/<base>` once that branch is gone), point your merge request at
<onto> and push with `--force-with-lease`."

### src/fold.rs (modified)

`Workspace` copies `base`; `BaseMerged { base, onto }` sets `workspace.base = Some(onto)` and notes
"base <base> merged, now on <onto>" (Tone::Attention).

### src/cli/commands/remove.rs, src/cli/output/remove.rs (modified)

```rust
pub(crate) fn removed(name: &str, pane: Closing, retired: &(Retired, Teardown), branch: BranchFate, (on, stacked): (&str, &[&str])) -> Document;
pub(crate) fn unmerged(name: &str, branch: &str, count: u32, stacked: &[&str]) -> Failure;
// cli/commands/retire.rs
pub fn refuse_unmerged_commits(flow: &FlowView, stacked: &[&str]) -> Result<(), Failure>;
```

`stacked[n]` lists the names from `stack::standing_on`, with the help line "shop-2 stands on
flow/shop-1: it keeps its commits and rebases onto origin/flow/shop-1 or where that merged" ("a
and b stand on ...: they keep their commits and rebase ..." for several). `removed` takes the
branch name beside the names for that line. A base whose commits all live on in a stacked
branch is not unmerged, so its removal deletes its branch.

### src/ui (modified)

```rust
pub struct NewFlowForm {
    ..
    /// The base typed or picked; empty starts from what the repo has checked out.
    pub base: String,
    /// What the repo has checked out, shown faint in an empty base row.
    pub checked_out: Option<String>,
    /// The branches of the repo's unfinished flows, which up and down put into the base row.
    pub bases: Vec<String>,
}
enum Field { Request, Type, Agent, Base, Mode }

pub fn set_branching(&mut self, branching: Option<Branching>, checked_out: Option<String>);
pub fn picked_base(&self) -> Option<&str>;

// app/messages.rs
pub fn on_branching(&mut self, repo: &Path, branching: Result<Option<Branching>, String>, checked_out: Option<String>);
```

- `ui.rs` answers LoadBranching with `git::current_branch(&repo)` as `checked_out`.
- `app/keys.rs` fills `bases` when it opens the dialog, from `App::bases_in(repo)`: the branches
  of the repo's unfinished, unmerged worktree flows, the focused flow's first.
- The bigger form is boxed where enums carry it (`View::NewFlow`, `Job::Start`,
  `Confirmed::Trust`), for `clippy::large_enum_variant`.
- Base row: for flows only, text with its own cursor, up/down cycle `bases`, `<checked out>
  (checked out)` faint when empty. The creates line ends in `from <base>`.
- `Job::args` adds `--base <base>` before `--`.
- `board/detail.rs` facts: `on <base>` after the branch.

### Output (modified)

`output/start.rs::started` and the status detail document gain `base` after `branch`.

## Data

```rust
// event.rs
EventKind::Workspace { path, branch, slot, window, agent,
    #[serde(default, skip_serializing_if = "Option::is_none")] base: Option<String> }
/// The branch this flow stands on merged; it now stands on `onto`.
EventKind::BaseMerged { base: String, onto: String }   // "base-merged", texts cleaned to TEXT_LIMIT

// view.rs (--json, VIEW_VERSION stays 3)
pub struct Workspace { path, branch, slot, window, agent, pub base: Option<String> }

// forge.rs
pub struct Found { url, status, created_at, precise_time, pub target: String }

// wake.rs
pub enum Note { Master(String), BaseMerged { base: String, onto: String } }
```

## Call chains

### saaio start --base

1. `start::run -> start::pick_base(&repo, args.base.as_deref()): Result<Option<StartPoint>, Failure>`
   after `pick_kind`, before `store.create`.
2. `pick_base -> git::start_point(repo, base): Result<StartPoint>`.
   Err -> `Failure::error("no branch <base> here or on a remote", ["Run `git branch -a` to see the
   branches", "Push it first when it lives in another clone"])`, exit 1, nothing created. An invalid
   name: `"<base> is no branch name"`.
3. `start::run -> worktree::workspace_for(..)` then `Workspace { base: args.base.clone(), .. }`.
4. `start::run -> workspace::create_worktree(&repo, &space, from.as_ref())
   -> process::run(&add_argv(repo, space, Some(&from.rev())))`.
5. Setup hook (hook_env with SAAIO_BASE), `append_workspace`, `output::start::started` with base.
6. Server: `worker_spec -> Adapter::wiring(exe, trusted, space.base.as_deref()) -> worker_prompt`;
   `worker_env -> hook_env` sets SAAIO_BASE.
7. `--base` with `--plain`: clap usage error, exit 2.

### The base merges first

1. `Server::forge_answer(Answer::Found { id, found })` folds the flow, `forge::news` = Merged, appends.
2. For Merged: `stack::on_merge(&self.board.flows, &flow, &found.target): Vec<(String, EventKind)>`.
3. `Server::restack(events, now)` appends BaseMerged to each; a failed append goes to server.log.
4. Refold: `workspace.base = onto`, timeline note.
5. Claude Code: `saaio wait -> deliver::worker_news -> wake::undelivered -> wake::for_worker`.
   Codex: `server/news.rs::take_news`, same text, typed when idle.

### New-flow dialog

1. Open: `NewFlowForm::new(..)` with `bases`, `Effect::LoadBranching(repo)`.
2. `ui.rs`: `app.on_branching(&repo, Branching::load(&repo), git::current_branch(&repo))
   -> form.set_branching(branching, checked_out)`.
3. `Field::Base`: chars edit, Backspace/Ctrl-u/Ctrl-w as the request, up/down cycle `bases`.
4. Enter: `Job::args` adds `--base`; a refusal shows as the form's error.

### saaio remove of a base flow

1. `remove::run` resolves the flow, `stack::standing_on(&flows, flow)` -> names.
2. Refused for unmerged commits: `output::remove::unmerged(.., &names)`; else
   `output::remove::removed(.., &names)`.

## Test seams

- `git::start_point`: real repos (`workspace::fixtures`): local, remote-only (bare clone as origin),
  invalid name, unknown.
- `add_argv`, `hook_env`: strings with and without base.
- `create_worktree`: from a base, has its commit, no upstream.
- `worker_prompt`, `wiring`: sentence only with a base, both adapters.
- `stack::standing_on`, `on_merge`: pure.
- `forge::parse`: fixtures with baseRefName / target_branch.
- `forge_answer`: `server/tests.rs`, Merged for A appends BaseMerged to B only.
- `fold`, `wake`, `deliver`: base moves; notes in order; delivered counts both.
- `NewFlowForm`, `Job::args`, dialog golden.
- `output::remove`: stacked names and help line.
- CLI `tests/cli/`: start --base local, remote-only, refused; status detail and --json base; remove
  names stacked flows.

## Assumptions

1. A local base wins and is used as it is, not fetched.
2. `--no-track` always with a base.
3. The new base after a merge is the branch the merge request went into.
4. Only bases that are flows of the same store and repository are followed.
5. `saaio remove` of a base flow does not refuse for it; it names the stacked flows.
6. master.md (old default kept as master-v12.md): start stacks with `--base <the previous flow's
   branch>`; saaio tells stacked workers when a base merges.
7. Sidebar and status list unchanged; base shows in detail views.
8. `--base` takes a branch name only.

## Tasks

### Starting from a base

- [x] Add `git::StartPoint`, `StartPoint::rev` and `git::start_point` (check-ref-format, local, fetch from remotes origin first under a deadline) with tests on local, remote-only, invalid and unknown bases
- [x] Add `Workspace.base` to `EventKind::Workspace`, `view::Workspace` and fold, `add_argv(.., from)` with `--no-track`, `create_worktree(.., from)`, `SAAIO_BASE` in `hook_env`/`HOOK_VARS`, with argv, env, fold and worktree tests
- [x] Add `--base` to `saaio start` (`pick_base`, refusal before anything is created, conflicts with `--plain`), `base` in `output::start::started` and the status detail, with CLI tests for local, remote-only, refused and `--json`
- [x] Add `adapter::worker_prompt(base)` and `wiring(.., base)`, passed by `worker_spec`, with tests for both adapters fresh and resumed

### Following the stack

- [x] Add `Found.target` (gh `baseRefName` in list and view argv, glab `target_branch`) with fixture tests
- [x] Add `EventKind::BaseMerged`, its fold (base moves, timeline note) and `src/stack.rs` (`standing_on`, `on_merge`) with pure tests
- [x] Append BaseMerged in `forge_answer` through `restack`, with a `server/tests.rs` test
- [x] Turn worker news into `wake::Note` (`undelivered`, `for_worker`, `WorkerNews::News(Vec<Note>)`) through wait, hook and server/news, with tests for order and the delivered count
- [x] Name stacked flows in `saaio remove` output and its unmerged refusal, with output and CLI tests

### UI and docs

- [x] Add the base row to the new-flow dialog (`base`, `checked_out`, `bases`, `Field::Base`, `set_branching`, `on_branching`, `picked_base`, creates line, `Job::args --base`) with form tests and the dialog golden; `on <base>` in the board's detail facts
- [x] master.md stack lines (master-v12.md kept), docs (cli.md start and remove, flows.md stacks, json.md workspace.base, ui.md dialog), CHANGELOG Unreleased, ROADMAP row dropped, CLAUDE.md architecture (stack.rs), design pages copied to docs/design/
- [x] End to end in a scratch store: a repo with a bare origin; start flow A, start B `--base` A's branch and C `--base` a remote-only branch; check worktrees, status and `--json`; the dialog's base row in a PTY; simulate A merged (a `merged` answer through the server, or a `base-merged` path test) and see B's base move and its worker news; `saaio remove` of A names B; end with `saaio --stop`

### After the review

Decided by the master: word the prompt for any base; close both restack holes.

- `Merged` records the branch it merged into (`into: Option<String>`, from `Found.target`), the fold
  keeps it as `FlowView.merged_into` (`#[serde(skip)]`, not in `--json`).
- `stack::pending(flows) -> Vec<(String, EventKind)>`: `on_merge` for every merged flow with a
  `merged_into`; empty once every stacked flow moved, so it repeats nothing.
- `forge_answer(Answer::Done)` appends `stack::pending(&self.board.flows)`: a failed BaseMerged is
  retried on the next round. Once per round, never per answer, so a board not refolded yet cannot
  append twice.
- `saaio remove` and `saaio clear` append `stack::on_merge` for a merged base before forgetting it.

- [ ] Word `adapter::worker_prompt` for any base ("Your branch stands on <base>: ..."), with its tests
- [ ] Record `Merged.into` and `FlowView.merged_into`, add `stack::pending`, append it at `Answer::Done`, with fold, stack and server tests for the retry
- [ ] Append the base-merged events in `saaio remove` and `saaio clear` of a merged base, with CLI tests; docs and CHANGELOG

## Review

Isolated review of `flow/start-base` against `main` (commits 2a49885..254e369), 2026-10-27.

### Fixed

- A remote-only base that was force-pushed since the repository last fetched it was refused as
  "no branch <base> here or on a remote": the unforced refspec failed as a non-fast-forward.
  Shown by `a_remote_base_rewritten_since_the_last_fetch_is_fetched_again`; the refspec is now
  forced (`+refs/heads/<base>:refs/remotes/<remote>/<base>`), which only ever moves the
  remote-tracking ref. 3686337.
- `src/server/master.rs` was not rustfmt-clean under the pinned toolchain (CI fmt would fail).
  00f322f.

The fix was re-read against its finding in a separate pass, not by an independent agent.

### Open

- After a base merges into the default branch, `workspace.base` becomes `main`, so a resumed
  worker is told "Your branch starts from main, not the default branch: ... Push main first if
  the remote lacks it", gets `SAAIO_BASE=main`, and the board shows `on main`. Recommendation:
  word `worker_prompt` so it holds for any base ("Your branch stands on <base>: open your merge
  request against <base> and rebase on it."), and keep the base as recorded.
- Restacking runs only where `forge_watch` appends Merged, and it asks only about flows whose
  worktree folder still exists. A base flow removed before the server saw its merge, or a
  BaseMerged append that fails (logged, never retried), leaves the stacked flows on the old base
  without news. Recommendation: accept for this PR (`saaio remove` already names the stacked
  flows and tells them where to rebase); note it in ROADMAP if it shows up in use.

### Where to look

1. `src/server/forge_watch.rs` `forge_answer` -> `stack::on_merge` -> `restack`: restacks only
   after Merged was appended, from `self.board.flows`; what a failed append leaves behind.
2. `src/wake.rs` `undelivered` and `src/deliver.rs` `worker_news`: the stored delivered count now
   counts BaseMerged beside Message. Old logs have none, so counts stay valid.
3. `src/workspace/git.rs` `start_point`: name check, local first, then a fetch per remote with a
   30 s deadline, during `saaio start`.
4. `src/ui/newflow.rs` base row (keys, cursor, `cycled_base`) with the goldens.

Routine, can be skimmed: `base: None` in fixtures, `Workspace.base` plumbing through event, view,
fold and outputs, `Found.target`, the remove help line, docs and CHANGELOG.

### Checks

`cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` and `cargo test` pass at
00f322f. `cargo deny check` not rerun (no dependency changes since 254e369).
