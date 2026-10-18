# Plan: merge requests opened or merged elsewhere

Approaches page: https://claude.ai/artifact/84dFveY3vhoUb9j3hn5Edk
Implementation page: https://claude.ai/artifact/DToDyZB86tpyqASuXxKme6

## Decision

The server asks GitHub (`gh`) or GitLab (`glab`) about each flow's branch on a slow interval, on a
background thread, and appends what changed to the flow's log. An open pull request records
`merge-request` as today (the flow is `done`, "PR open"). A merged one records the new `merged`
event: the flow is `done` and `merged`. A closed one without merge, on a flow that is done because
of it, records `merge-request-closed`: the flow goes back to its last phase as `idle` with the note
"PR #42 closed", which wakes the master. No gh/glab, no auth or no network: nothing is recorded,
the error goes to server.log once, nothing blocks.

Decided with the user (via the master): approach A, keep "PR open" as `done` and add merged on
top, a closed PR sends the flow back to `idle`. Further open questions take the recommendations
below.

## Units

`src/forge.rs` (new, pure): the forges saaio asks about a flow's merge request.

```rust
/// Who hosts a repository's merge requests, as its remote says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Forge { GitHub, GitLab }

/// A flow to ask about: its id, its worktree and its branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ask { pub id: String, pub dir: PathBuf, pub branch: String }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status { Open, Merged, Closed }

/// A merge request the forge lists for a branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found { pub url: String, pub status: Status }

/// The flows worth asking about: not plain, with a branch, not merged.
pub fn asks(flows: &[FlowView]) -> Vec<Ask>;

impl Forge {
    /// The forge of the first remote whose web host (an SSH alias resolved by `real_host`) is
    /// github.com or has "gitlab" in it; None for any other host.
    pub fn of(remotes: &[String], real_host: impl Fn(&str) -> Option<String>) -> Option<Forge>;
    /// gh pr list --head <branch> --state all --json url,state --limit 10
    /// glab mr list --source-branch <branch> --all --output json --per-page 10
    pub fn argv(self, branch: &str) -> Vec<String>;
    /// The merge requests in gh's or glab's JSON (gh: url, state OPEN|MERGED|CLOSED;
    /// glab: web_url, state opened|merged|closed|locked); an error for JSON of another shape.
    pub fn parse(self, json: &str) -> anyhow::Result<Vec<Found>>;
}

/// The one that speaks for the branch: the first open, else the first merged, else the first
/// closed (both tools list newest first).
pub fn pick(found: Vec<Found>) -> Option<Found>;

/// What to append so `flow` shows `found`: merge-request for an open one it does not show yet,
/// merged for a merged one, merge-request-closed for the closed one it shows; None otherwise.
pub fn news(flow: &FlowView, found: Option<&Found>) -> Option<EventKind>;
```

`src/adapter/calls.rs` (modified): the host resolution `in_repo` already does, shared.

```rust
/// The web host a remote points to, lowercase: its host, or for an SSH remote the host
/// `real_host` resolves its alias to (ssh.github.com -> github.com). None for no host.
pub fn web_host_of(remote: &str, real_host: impl Fn(&str) -> Option<String>) -> Option<String>;
```

`in_repo` keeps its behaviour and uses `web_host_of` for the alias case.

`src/workspace.rs` (modified):

```rust
/// Runs an argv in `dir` with GH_PROMPT_DISABLED=1 and no stdin; its stdout, or an error with
/// the last line it printed.
pub fn output_in(dir: &Path, argv: &[String]) -> Result<String>;
```

`src/server/forge_watch.rs` (new): when the server asks the forges, on its own thread.

```rust
/// How often a round of questions starts; the first starts with the server.
pub const ASK_EVERY: Duration = Duration::from_mins(2);

/// What the forge thread sends back.
#[derive(Debug)]
pub enum Answer {
    /// What the forge lists for a flow's branch; None when it lists nothing.
    Found { id: String, found: Option<Found> },
    /// A question that failed: gh or glab missing, not logged in, offline, unreadable JSON.
    Failed(String),
    /// The round is over.
    Done,
}

/// When the next round starts, whether one runs, the last error logged.
#[derive(Debug, Default)]
pub struct ForgeWatch {
    asked_at: Option<Instant>,
    asking: bool,
    /// The running round had a failure.
    failed: bool,
    logged: Option<String>,
}

impl ForgeWatch {
    /// The asks of a round when one is due, none runs and `flows` has any; marks it running.
    pub fn due(&mut self, flows: &[FlowView], now: Instant) -> Option<Vec<Ask>>;
    /// The round ended; one without a failure forgets the error logged last.
    pub fn done(&mut self);
    /// `error` when it is not the one logged last.
    pub fn worth_logging(&mut self, error: String) -> Option<String>;
}

/// Asks about each flow in turn on the "forge" thread, sending an Answer for each and Done last.
pub fn ask_in_background(asks: Vec<Ask>, events: Sender<Event>);

/// One flow's question: its worktree's remotes -> Forge::of (workspace::ssh_hostname) ->
/// workspace::output_in(Forge::argv) -> Forge::parse -> pick. Ok(None) for a worktree that is
/// gone, a remote of no known forge, or no merge request.
fn ask(ask: &Ask) -> Result<Option<Found>>;

impl Server {
    /// Starts a round when ForgeWatch says one is due; from refresh.
    pub(super) fn ask_forges(&mut self);
    /// Folds the flow again from its log and appends forge::news, if any; logs a failure once.
    pub(super) fn forge_answer(&mut self, answer: Answer);
}
```

`src/server.rs` (modified): `Event::Forge(Answer)`; field `forges: ForgeWatch`; `handle` calls
`forge_answer`; `serve` calls `ask_forges` after `refresh`.

`src/event.rs` (modified): two kinds, texts cut to TEXT_LIMIT.

```rust
/// The merge request at url was merged.
Merged { url: String },
/// The merge request at url was closed without a merge.
MergeRequestClosed { url: String },
```

`src/fold.rs` (modified):

- `MergeRequest` and `Merged` share `fn open_merge_request(&mut self, url: &str, at: u64)`: last
  phase, `Done`, no blocker or delegate, `merge_request = Some(url)`, `stopped_at`.
- `MergeRequest` notes "MR opened" (unchanged). `Merged` sets `merged = true`, notes
  "{text::merge_request(url)} merged", Success.
- `MergeRequestClosed { url }`: only when `merge_request == Some(url)` and not merged:
  `merge_request = None`; a `Done` flow becomes `Idle` with `stopped_at = None`; notes
  "{text::merge_request(url)} closed", Attention.
- `shown()` includes `merged`.

`src/view.rs` (modified): `FlowView::merged: bool` (JSON `merged`), false in every constructor.

`src/text.rs` (modified): `pub fn merge_request(url: &str) -> String` moved from
`ui/sidebar/labels.rs` ("PR #12", "MR !418", "MR").

`src/proto.rs` (modified): `PROTOCOL_VERSION` 5 -> 6 (FlowView in the Board changed).

`src/ui/sidebar/labels.rs` (modified): a merged flow's label is "PR #42 merged".

`src/ui/board/cards.rs` (modified): `Done` with a merged one: "PR merged" / "MR merged".

`src/cli/output/status.rs` (modified): detail `merged: true` when merged; help for a merged flow
"Run `saaio clear` to tear down its worktree", the unmerged one keeps "after the MR is merged".

## Data

- Log lines: `{"v":1,"at":...,"kind":"merged","url":"https://github.com/a/b/pull/42"}`,
  `{"v":1,"at":...,"kind":"merge-request-closed","url":"..."}`. Older saaio skips them (unknown
  kind).
- `--json`: `merged` (bool) on every flow. A new key: `VIEW_VERSION` stays 3.
- Protocol 6: an older `saaio ui` is refused by the newer server as today.
- Fixtures `src/forge/fixtures/`: `gh-merged.json` (recorded from PR #42), `gh-open.json`,
  `gh-closed-then-open.json`, `gh-none.json`, `glab-merged.json`, `glab-opened.json`,
  `glab-closed.json` (glab's `mr list -F json` shape: `web_url`, `state`).

## Call chains

### A round of questions

1. `Server::serve` -> `Server::refresh()`, then `Server::ask_forges()`
2. `ask_forges` -> `ForgeWatch::due(&self.board.flows, Instant::now()): Option<Vec<Ask>>` -> `forge::asks(flows)`;
   None when not due, a round runs, or no flow qualifies
3. `ask_forges` -> `forge_watch::ask_in_background(asks, self.events.clone())`
4. thread, per ask -> `ask(&ask)`:
   1. `ask.dir` not a dir -> `Ok(None)`
   2. `workspace::remote_urls(dir)` -> `Forge::of(&remotes, workspace::ssh_hostname)`; None ->
      `Ok(None)`
   3. `workspace::output_in(dir, &forge.argv(&branch))?` -> `forge.parse(&json)?` -> `forge::pick`
5. thread -> `events.send(Event::Forge(Answer::Found { id, found }))`, or
   `Answer::Failed(format!("{forge}: {error:#}"))`; after the last ask `Answer::Done`
6. `Server::handle(Event::Forge(answer))` -> `Server::forge_answer(answer)`:
   - `Found`: `store.events(id)` -> `fold::fold(id, &events, None, now)` -> `forge::news(&flow,
     found.as_ref())` -> `store.append(id, &Event::new(now, kind))`. The next refresh sees the
     log change and sends the board.
   - `Failed(error)`: `ForgeWatch::worth_logging(error)` -> `server_log::write` when new.
   - `Done`: `ForgeWatch::done()`.

Errors: gh/glab missing -> `output_in` fails "could not run gh" -> Failed, logged once. Not
logged in, offline, rate limited -> the tool's last line -> Failed, logged once. Unreadable JSON
-> Failed. A flow gone before its answer -> `store.events` fails or fold is None -> nothing
appended. The send failing (server stopping) ends the thread.

### What news records

| flow shows | forge lists (picked) | appended |
| --- | --- | --- |
| no MR | open A | `merge-request A` |
| MR A | open A | nothing |
| MR A | open B (A closed) | `merge-request B` |
| no MR or MR A, not merged | merged A | `merged A` |
| merged | anything | nothing (not asked) |
| MR A | closed A | `merge-request-closed A` |
| no MR | closed A | nothing |
| anything | nothing | nothing |

### A closed PR

`merge-request-closed A` on a done flow -> fold: Idle, phase stays the last, note "PR #42
closed" -> state change -> `wake::changed_flows` sees Idle -> the master is woken.

## Test seams

- `forge`: unit tests with the fixtures: `parse` per tool and state, `pick` order, `of` for
  github.com, gitlab.com, a self-hosted gitlab.example.com, an SSH alias via a fake resolver,
  an unknown host; `argv`; `asks` (plain, no branch, merged excluded; done-not-merged and ended
  included); `news` over the table above.
- `calls::web_host_of`: https, scp-like, ssh://, alias, ssh.github.com; `in_repo` tests unchanged.
- `fold`: `merged` on a flow at review (done, last phase, merged, note), on a done flow (merged,
  no state change), `merge-request-closed` on its done flow (idle, note, changed_at), on another
  URL or a merged flow (nothing), `changes` true for merged.
- `ForgeWatch`: `due` first time, not again within ASK_EVERY, not while asking, after `done` and
  ASK_EVERY; `worth_logging` once per distinct error.
- `text::merge_request`: the moved tests.
- UI: sidebar label and card for a merged flow; golden files updated with
  `SAAIO_UPDATE_GOLDEN=1`.
- `status`: detail with `merged` and its help.
- `tests/cli.rs` E2E: scratch SAAIO_HOME, a repo with origin `https://github.com/acme/shop.git`,
  a flow with that worktree and branch, a fake `gh` first on PATH printing `gh-merged.json`;
  `saaio server` runs; the flow's log gets `merged`; `saaio status --json` shows done and
  merged. A second with `gh` exiting 1 ("not logged in"): nothing appended, server.log has the
  line once.

## Assumptions

- Interval 2 minutes, first round at server start; one round at a time, flows asked in turn.
  About 30 calls per flow per hour, well inside GitHub's 5000 per hour.
- Only while `saaio server` runs (saaio ui); `saaio status` never calls the network.
- Asked: every non-plain flow with a branch that is not merged, ended ones included (the master
  may open the PR after the worker ended). Never asked again once merged.
- The forge from the remote's host: github.com (also behind an SSH alias) -> gh; a host with
  "gitlab" in it -> glab; GitHub Enterprise or other hosts: not asked.
- gh/glab run in the worktree, so their own remote resolution (forks' upstream, SSH aliases)
  applies; `--head` / `--source-branch` match the branch name in any fork.
- Becoming merged on a done flow does not wake the master (state stays done); a flow at review
  that turns merged does (it becomes done).
- No timeout of our own on gh/glab; a hung call only delays the next round.
- No CHANGELOG entry before 0.1.0; master.md unchanged.

## Tasks

### Model

- [x] Add `EventKind::Merged` and `EventKind::MergeRequestClosed` with line round-trip tests, and
  `FlowView::merged`; move `merge_request` label to `text::merge_request` with its tests
- [x] Fold `merged` and `merge-request-closed` (`open_merge_request`, `shown` with merged) with the
  fold tests listed under Test seams; bump `PROTOCOL_VERSION` to 6
- [x] Add `calls::web_host_of` and use it in `in_repo`, with unit tests

### Forge

- [x] Add `forge.rs`: `Forge::of`, `argv`, `parse`, `pick` with the recorded gh/glab fixtures
- [x] Add `forge::asks` and `forge::news` with tests over the news table
- [x] Add `workspace::output_in` with a test running a script in a dir

### Server

- [x] Add `server/forge_watch.rs`: `ForgeWatch`, `Answer`, `ask_in_background`, `ask`, with
  `ForgeWatch` unit tests
- [x] Wire `Event::Forge`, `ask_forges` and `forge_answer` into the server; E2E tests in
  `tests/cli.rs` with a fake `gh` (merged recorded; failing gh records nothing, logs once)

### Show it

- [x] Sidebar label "PR #42 merged" and card "PR merged"/"MR merged", golden files updated
- [ ] `saaio status` detail `merged` and its help
- [ ] Docs: `docs/flows.md` (states, a section on merge requests opened or merged elsewhere,
  ending a flow), `docs/json.md` (`merged`), `docs/troubleshooting.md` (a flow stays at review:
  gh/glab missing or not logged in, saaio ui not running), plain reference style

### Check

- [ ] E2E: `saaio ui` with a scratch SAAIO_HOME on a flow seeded on `flow/config-reload` in this
  repository (PR #42, merged): the sidebar shows "PR #42 merged" and the board "PR merged"
  within the first round
