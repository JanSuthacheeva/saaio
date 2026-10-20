# Plan: a worker's start is recorded before its agent runs

## Request

`tests/cli.rs` `after_a_crash_the_server_resumes_the_master_and_every_worker` failed on the
Linux CI run of main 3bbc8be (run 39439136987): after the crash one worker was started fresh
instead of resumed. Decide whether a user can lose a worker's session this way, fix it at the
cause with a test that failed before, never by raising a timer.

## Root cause

`Server::open_worker` (`src/server/panes.rs`) started the agent first and appended the
`Workspace { window: "saaio-ui" }` event (HOSTED) after. HOSTED is what tells the next server
to resume rather than start fresh (`launch::pane::worker_spec`). A server that dies between the
two leaves an agent that ran - and may have saved a session - with no record of it, so the next
server starts it fresh with its first prompt: the session is lost (for Claude Code the fresh
start reuses the flow id as `--session-id`, which may already exist).

Reproduced in Linux (rust 1.95 container, `--cpus=2`, `stress-ng --cpu 4 --io 2`, 10 copies of
the test at once): 5 of 40 runs failed. Each failed run's logs showed the same thing: the agent
had written `fresh shop-1`, but shop-1's log had no HOSTED event from the first server.
After the fix: 0 of 60.

## Decision

Record first, then start: `open_worker` appends HOSTED before `open_pane`. A crash between them
leaves a HOSTED worker that never ran; the next server resumes it, the resume fails within
`RESUME_GRACE` and `open_fresh` starts it fresh, which is already handled. A record that cannot
be written fails the pane ("could not record its start: ...") and starts no agent, since that
agent could never be resumed. A spawn that fails after the record takes the record back (the
workspace event as it was) and blocks the flow as before, so a reopen starts it fresh: an agent
with no resume template would otherwise be refused for good.

## Tasks

- [x] `server::tests::a_worker_whose_start_cannot_be_recorded_never_starts`: a flow whose log
      is read-only gets a failed pane and its agent never runs (fails before the fix: the pane is
      Running).
- [x] `Server::open_worker` appends HOSTED before it opens the pane; resumes and spawn failures
      behave as before.
- [x] `server::tests::a_worker_whose_agent_could_not_start_starts_fresh_when_reopened`: an agent
      without a resume template that failed to spawn opens when reopened (review finding).

## Review

Reviewed in an isolated agent against this plan, CLAUDE.md and the callers of `open_worker`
(`apply`, `reopen_worker`, `open_fresh`, `worker_spec`, `reconcile`).

**Fixed**

- A spawn that failed after the new up-front HOSTED record left the worker marked as hosted
  though its agent never ran. For an agent without a resume template (a custom agent) a reopen
  after fixing the command was then refused for good with "agent X cannot resume a session";
  on main it started fresh. Proven by
  `server::tests::a_worker_whose_agent_could_not_start_starts_fresh_when_reopened` (passes on
  main, failed on 3975dc0). `open_worker` now appends the workspace event as it was before the
  Blocked event. f506e54.

**Open**

- A server that dies in the short window between the HOSTED record and the spawn leaves an
  agent without a resume template unopenable ("cannot resume a session") instead of starting
  it fresh. Resumable agents are fine (Claude Code's failed resume falls back to `open_fresh`
  within `RESUME_GRACE`; Codex with no session id starts fresh). Recommendation: accept - the
  window is microseconds and the flow can be restarted; not worth special-casing.
- `a_worker_whose_start_cannot_be_recorded_never_starts` relies on a 0444 log refusing writes,
  which root ignores. GitHub runners are not root, so CI is fine; a root container would fail it.
- `server::tests::a_typed_line_that_begins_no_turn_lets_the_next_one_follow` failed once of four
  runs under load average ~12 (asserts `Busy::Typed` right after the line appears, against a
  300 ms `unstarted`). Not touched by this branch; worth its own flake fix.
- The fix was reviewed by the same agent in a second pass, not independently.

**Where to look**

1. `src/server/panes.rs` `open_worker`: record, spawn, and on spawn failure the record taken
   back - the order is the whole change.
2. `src/server.rs` around `RESUME_GRACE` with `open_fresh`: what the next server does with a
   HOSTED worker that never ran.
3. The two new tests in `src/server/tests.rs` can be skimmed.

**Checks**

At f506e54: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, the server unit
tests (3 clean runs) and `tests/cli.rs` `after_a_crash_the_server_resumes_the_master_and_every_worker`
pass. The full suite last ran at 3975dc0 (Linux, green apart from two container-setup failures).
