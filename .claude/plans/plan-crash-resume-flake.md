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
agent could never be resumed. A spawn that fails after the record blocks the flow as before.

## Tasks

- [x] `server::tests::a_worker_whose_start_cannot_be_recorded_never_starts`: a flow whose log
      is read-only gets a failed pane and its agent never runs (fails before the fix: the pane is
      Running).
- [x] `Server::open_worker` appends HOSTED before it opens the pane; resumes and spawn failures
      behave as before.
