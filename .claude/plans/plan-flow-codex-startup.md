# Plan: Codex workers start without a dialog

Approaches page (the review surface): https://claude.ai/artifact/HRoteRKAXRBeSepJwCxV1b

## Problem

A Codex worker that saaio starts can stop at one of two startup dialogs, and saaio then shows it
as working forever. This was seen on 2026-10-18 with Codex 0.163.0: Codex's log shows
`tui-project-trust-read` and `hooks/list`, then no turn until someone pressed Enter in the pane.

- **Folder trust**: "Trust this folder?" A worktree inherits trust from its main repo, so this
  dialog appears only for a repo that Codex never trusted.
- **Hooks review**: "Hooks need review: 8 hooks are new or changed." It appears the first time,
  and again whenever a hook's command, timeout or event set changes. A new saaio binary path or a
  new event (PreCompact, PostCompact) counts as a change.

## Decision (approved 2026-10-18)

- **H1**: `codex::hooks()` also passes the trust of saaio's own 8 hooks:
  `-c hooks.state={"<key>"={trusted_hash="sha256:..."},...}`. saaio computes each hash the way
  Codex does. Only saaio's hooks are trusted. Your `~/.codex/hooks.json` and a repo's `.codex`
  hooks still go through Codex's review, and nothing is written to `~/.codex`. This reverses the
  rejection of "copying Codex's trust hash" in `docs/HANDOVER.md:260`. Rejected:
  `--dangerously-bypass-hook-trust`, which runs every enabled hook unreviewed, including a cloned
  repo's.
- **F1**: a Codex worker or plain session trusts its own worktree:
  `-c projects={"<worktree>"={trust_level="trusted"}}`. It applies to fresh and resumed starts.
  The master's folder is left alone.
- **D1**: when a Codex worker's pane has been running for `FIRST_TURN_GRACE` (30 s) and its flow
  is still `working` with no session, saaio server appends
  `Waiting { question: STALLED_START }`. The first UserPromptSubmit then appends Working, which
  clears it.
- The Interrupt hook's timeout is 3 s, the most Codex allows. This removes Codex's startup warning
  "clamping Interrupt hook timeout to 3s".

## Verified Codex facts (0.163.0, real binary)

These were checked in tmux with a scratch `CODEX_HOME` (only `auth.json` copied in) and a scratch
repo with its worktree at `.saaio/worktrees/x`. No prompt was sent.

- The dotted form `-c 'projects."<path>".trust_level="trusted"'` does **not** skip the folder
  dialog, for the worktree path or for the repo root. The inline table
  `-c 'projects={"<worktree>"={trust_level="trusted"}}'` does skip it, and persists nothing.
- The dotted form `-c 'hooks.state."<key>".trusted_hash=...'` does **not** work either: the key
  contains a dot (`config.toml`). The inline table `-c 'hooks.state={"<key>"={trusted_hash="..."}}'`
  skips the review, and persists nothing.
- Hook key: `/<session-flags>/config.toml:<event_key>:0:0`. `event_key` is snake case:
  `user_prompt_submit`, `pre_tool_use`, `post_tool_use`, `permission_request`, `stop`,
  `interrupt`, `pre_compact`, `post_compact`.
- Hash (`codex-rs/hooks/src/engine/discovery.rs:777`, `codex-rs/config/src/fingerprint.rs`):
  `"sha256:" + hex(sha256(json))`. `json` is the compact JSON with sorted keys of
  `{"event_name":<event_key>,"hooks":[{"type":"command","command":<command>,"timeout":<secs>,"async":false}]}`.
  The timeout is the normalized one: Interrupt is clamped to at most 3. A matcher, `statusMessage`
  and `commandWindows` that are None are left out.
- Pinned values that Codex itself wrote to `~/.codex/config.toml`, for the command
  `'/Users/jansuthacheeva/.cargo/bin/saaio' hook codex`:
  - stop, timeout 10: `sha256:7fdf0398b9d5ac50aee6888193db70211a1b5026483ffe30b99bfcf90fd0bf23`
  - interrupt, timeout 3: `sha256:50f2e8b23093a3fc4e12bf9edb639b04b46adb4cbac23d59fcd894d300b9528c`
  - pre_tool_use, timeout 10: `sha256:8968411b220f168aa89dfdd45fd451ce0922ab254c2fd229348cf39cc5f8656a`
- Codex reads `hooks.state` from the SessionFlags layer (`codex-rs/hooks/src/config_rules.rs:15`).
- Any `-c` makes Codex run without the shared daemon. That is already the case today.

## Units

### `src/adapter/codex.rs` (modified)

```rust
/// One hook saaio listens to: Codex's event name, the key its trust state uses, its timeout.
struct Hook {
    event: &'static str,
    key: &'static str,
    timeout: u64,
}

/// The hook events saaio listens to; Interrupt at 3 s, the most Codex allows.
const HOOKS: [Hook; 8] = [...];

/// Where Codex says hooks passed with -c come from, in their trust keys.
const SESSION_FLAGS: &str = "/<session-flags>/config.toml";

/// Unchanged signature. Adds `-c hooks.state={...}` with trusted_hash for each hook in HOOKS, so
/// Codex runs them without its review. Each hook uses its own timeout.
pub fn hooks(flow_exe: &Path) -> Vec<String>;

/// ["-c", "projects={\"<folder>\"={trust_level=\"trusted\"}}"]: Codex trusts that folder without asking.
pub fn trusted_folder(folder: &Path) -> Vec<String>;

/// The hash Codex keeps in hooks.state.<key>.trusted_hash for one command hook: sha256 of its
/// identity as compact JSON with sorted keys.
fn trust_hash(hook: &Hook, command: &str) -> String;
```

`EVENTS` is replaced by `HOOKS`. `HOOK_TIMEOUT_SECS` stays as the timeout of every hook except
Interrupt, which gets `INTERRUPT_TIMEOUT_SECS = 3`. Build the JSON with `serde_json::json!`
(serde_json's `Map` is a BTreeMap, so the keys come out sorted) and `serde_json::to_string`.
Quote the TOML keys and strings with `toml_string`.

### `src/adapter.rs` (modified)

```rust
/// Unchanged meaning; `folder` is the worktree the session runs in (Codex trusts it).
pub fn wiring(self, flow_exe: &Path, folder: &Path) -> Vec<String>;
pub fn plain_wiring(self, flow_exe: &Path, folder: &Path) -> Vec<String>;
```

For Codex: `codex::hooks(flow_exe)`, then `codex::trusted_folder(folder)`, then (wiring only) the
worker instructions. For Claude Code: unchanged, and `folder` is unused. `wire_master` is
unchanged: it gets hook trust through `codex::hooks` and no folder trust.

### `src/launch/pane.rs` (modified)

`worker_spec` passes `Path::new(&space.path)` as `folder` to `wiring` and `plain_wiring`.

### `src/server/startup.rs` (new)

It answers one question: which Codex workers began no turn after they launched.

```rust
/// How long a Codex worker may run with no first turn before its flow shows waiting.
#[cfg(not(test))]
pub(super) const FIRST_TURN_GRACE: Duration = Duration::from_secs(30);
#[cfg(test)]
pub(super) const FIRST_TURN_GRACE: Duration = Duration::from_millis(300);

/// The question a stalled start waits on.
pub(super) const STALLED_START: &str =
    "no turn began after launch: answer the dialog in its pane";

impl Server {
    /// Appends Waiting { question: STALLED_START } to each flow whose start stalled.
    pub(super) fn notice_stalled_starts(&mut self);
}

/// True when a worker opened at `opened_at` began no turn by `now`: its pane runs a Codex
/// session, its flow is not plain, still Working and has no session, and FIRST_TURN_GRACE passed.
pub(super) fn stalled(
    flow: &FlowView,
    adapter: Option<Adapter>,
    status: Option<&PaneStatus>,
    opened_at: Option<Instant>,
    now: Instant,
) -> bool;
```

`notice_stalled_starts` collects the ids where `stalled(...)` is true (it reads `self.board.flows`,
`self.adapters`, `self.host.status(&key)` and `self.opened_at`). For each one it calls
`self.store.append(id, &LogEvent::new((self.clock)(), EventKind::Waiting { question }))`. An
append error goes to `server_log::write`, as in `panes.rs`. It appends once per stall: once the
event is folded the flow is Waiting, no longer Working. Register the module in `src/server.rs`.

### `src/server.rs` (modified)

In `serve`, inside `if self.refreshed_at.elapsed() >= WATCH_EVERY`, call
`self.notice_stalled_starts()` after `self.refresh()`, so it reads the fresh board.

### `Cargo.toml` (modified)

Add `sha2 = "0.10"`.

## Call chains

1. Fresh Codex worker:
   - `Server::open_worker -> pane::worker_spec(flow, agent, env): Option<PaneSpec>`
   - `-> Adapter::Codex.wiring(flow_exe, space.path): Vec<String>`
   - `-> codex::hooks(flow_exe)`: `-c check_for_update_on_startup=false`, 8 × `-c hooks.<Event>=[...]`,
     then `-c hooks.state={...}`, with `trust_hash(hook, command)` for each
   - `-> codex::trusted_folder(folder)`, then `instruct(WORKER_PROMPT)`
   - `-> Host::open`. Codex shows no dialog, runs UserPromptSubmit, and the flow gets its session.
2. Resumed Codex worker: the same wiring through the `resume_argv` branch, so a resume after a new
   saaio install is not reviewed again.
3. Codex master: `Adapter::wire_master` uses `codex::hooks(flow_exe)` and gets hook trust. Its
   folder is not trusted.
4. Stalled start:
   - `Server::serve` tick `-> notice_stalled_starts()`
   - `-> stalled(flow, adapters.get(key), host.status(key), opened_at.get(key), Instant::now())`
   - true `-> Store::append(id, Waiting { question: STALLED_START })`
   - `-> StoreWatch`, refold: the board shows waiting with the question as the blocker,
     `saaio status` shows its help, and `wake.rs` wakes the master.
   - The user answers the dialog in the pane `-> UserPromptSubmit -> saaio hook codex` appends
     Session and Working `->` the flow is working again (timeline "answered").
   - Error path: the append fails `->` `server_log::write`. The flow stays working, and the next
     tick tries again.
5. A hash format change in a later Codex: the review dialog returns, and chain 4 shows it as
   waiting.

## Test seams

- `codex.rs`:
  - `trust_hash` matches the three pinned hashes above.
  - `hooks()` contains `hooks.state` as one `-c` value that parses as TOML. It has 8 keys
    `/<session-flags>/config.toml:<key>:0:0`, and each hash equals `trust_hash` of that hook's
    command and timeout.
  - The Interrupt handler has timeout 3.
  - `trusted_folder` parses as TOML to `projects.<path>.trust_level = "trusted"`, also for a path
    with a dot or a quote.
- `adapter.rs`: the Codex worker and plain wiring end with `trusted_folder(folder)` (before the
  instructions for a worker). Claude Code is unchanged.
- `launch/pane.rs`: the Codex `worker_spec` argv trusts `space.path`, fresh and resumed.
- `server/startup.rs`: `stalled` is true only for Codex, a running pane, not plain, Working, no
  session, and the grace passed. One false case for each condition.
- `server/tests.rs`: a fake Codex agent (`adapter = "codex"`, `command = ["sh", "-c", "sleep 30"]`)
  opened by the server. Within a few hundred ms past `FIRST_TURN_GRACE`, its log ends in
  `Waiting { question: STALLED_START }`, exactly once.

## Assumptions

- The grace is 30 s. Codex starts in 2 to 5 s, and its first UserPromptSubmit follows at once.
- Codex only. Claude Code reports no session of its own, so it gives no signal, and it starts
  without dialogs today.
- The worktree path is passed as recorded (absolute, not canonicalized). If Codex compares it with
  a symlink-resolved cwd and misses, D1 shows the stall.
- No CHANGELOG entry: `CHANGELOG.md` holds only 0.1.0 until it is released.
- The hint text is `STALLED_START` above. It shows as the flow's blocker, and in `saaio status` as
  waiting with the existing "Run `saaio report --flow <name> working` once answered" help.

## Tasks

- [x] Add `sha2`, `Hook`, `HOOKS` (Interrupt at 3 s) and `codex::trust_hash` with unit tests
      pinning the three hashes Codex wrote
- [x] Make `codex::hooks()` pass `-c hooks.state={...}` with each hook's trusted hash; tests that
      it parses as TOML with 8 keys whose hashes match, and that Interrupt's timeout is 3
- [x] Add `codex::trusted_folder(folder)`, and thread `folder` through `Adapter::wiring` and
      `Adapter::plain_wiring` and `worker_spec` (`space.path`); update the adapter, pane and
      claude_code tests
- [x] Add `server/startup.rs`: `FIRST_TURN_GRACE`, `STALLED_START` and `stalled` with unit tests
      for each condition
- [x] Add `Server::notice_stalled_starts`, call it from `serve` after `refresh`, and add a
      `server/tests.rs` test with a fake Codex agent that never reports (Waiting appended once)
- [x] Docs:
  - `docs/troubleshooting.md`: replace the two dialog rows with one row: a Codex flow waiting
    with "no turn began after launch" means a dialog in its pane.
  - `docs/HANDOVER.md` Codex decisions: H1, F1, D1 and the facts above.
  - The "How a worker reports" and Codex sections of `README.md` and `docs/flows.md` (states),
    wherever they mention the review.
  - `CLAUDE.md`: list `server/startup.rs` in the server module and this plan in the plan list.

  All in the plain reference style.
- [ ] End to end, cheap: build, then use a scratch repo, a scratch `SAAIO_HOME` and a scratch
      `CODEX_HOME` (only `auth.json` copied; repo untrusted, no hook trust):
  - Start one Codex flow with a single short request ("reply with ok"). Confirm that it reaches a
    session and goes idle with no dialog, and that the scratch `CODEX_HOME/config.toml` gained no
    `projects` or `hooks.state`.
  - Then start a custom agent with `adapter = "codex"` and `command = ["sh", "-c", "sleep 120"]`,
    and confirm that `saaio status` shows it waiting with `STALLED_START` after 30 s. This step
    uses no Codex.
