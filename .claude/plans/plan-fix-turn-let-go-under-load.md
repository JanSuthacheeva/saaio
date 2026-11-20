# Fix: a heavy run loses its turn while it still runs when the machine is loaded

Report: "heavy commands still run in parallel on holy-grail-ai-admin (repo `.saaio/config.toml`
`[exclusive] commands = ["artisan test", "make test", "pest", "pint", "phpstan"]`, saaio 0.3.0).
The turn is taken (flow logs show turn-waiting/turn-taken), but it is let go while the run still
goes on." Suspected by the reporter: `ps` times out at 1 s under load and the watcher fails open;
and/or "the run ended" is misread (Sail/docker compose exec, run_in_background, wrappers).

## Report

Expected: a worker's heavy run keeps the project's turn as long as it runs; another worker's heavy
call waits until it ends. The turn is let go early only when its holder is really gone.
Actual: a second worker's call takes the turn and runs while the first run still goes on.
Steps and environment: two or more Claude Code workers on holy-grail-ai-admin running
`make test` / `artisan test` / `pint` through Docker (Sail-style `docker exec`/`docker compose
exec`), macOS, 16 GB with 11 GB of swap in use, Docker Desktop and other load. saaio 0.3.0.

Evidence in the real store (read only):
`~/.local/state/saaio/projects/holy-grail-ai-admin/server.log` holds 23 lines
`turn watch flow ...: no answer within 1s, 3 times; the turn is let go`, 9 `no run showed`, 309
`the run ended`, one hook line `no answer within 1s; the tool runs without the turn` (11-17) and
one `waited 30m for the turn (hgai-3678 runs artisan test); the tool runs` (11-16). Flow
`dd7daadc` waited from 17:12:45 for `artisan test` and logged `turn-taken` at 17:17:17, the very
second `851e5d29`'s watcher logged `no answer within 1s, 3 times; the turn is let go`; six seconds
later `dd7daadc`'s own watcher let go for the same reason.

## Reproduction

The worktree's `saaio` (debug build) with a scratch store, a scratch shared folder and a scratch
config, a real `saaio server` and two real `saaio start` workers. The worker's agent is a stand-in
(`[agents.fake]`, adapter claude-code) that does what a Claude Code worker does on a Bash call:
pipes a PreToolUse payload for `./vendor/bin/sail artisan test` into `saaio hook claude-code`
(with the pane's `SAAIO_FLOW_ID`/`SAAIO_PROOF`), then runs it as a child
`/bin/zsh -c "true && eval '<cmd>' && pwd -P"`, and stays alive. `vendor/bin/sail` execs
`docker compose exec app php artisan test`; a `docker` stand-in on PATH logs its start and end and
runs 40 s. A `ps` shim on PATH sleeps 1.5 s before `/bin/ps` while a flag file exists: the
loaded machine, where `ps` takes past 1 s.

Script: `/private/tmp/claude-501/-Users-jansuthacheeva-projects-saaio--saaio-worktrees-exclusive-lets-go/fb4c5a7c-369f-49da-ad8a-729969f2c281/scratchpad/e2e/run.sh <secs|none> [ps-delay]`
(the folder also holds `fake-agent.sh`, `config.toml`, `bin/ps`, `bin/docker`, `repo/`). It starts
the server, worker 1, worker 2 four seconds later, makes `ps` slow by `ps-delay` (default 1.5 s)
`<secs>` later, prints
`runs.log` and the server.log, and ends with `saaio --stop`.

`run.sh 6` (ps slow once both hooks are done):

    5be68f1c hook-returned 1795133817
    5be68f1c start 1795133818
    slow ps from 1795133827
    c5a8596e hook-returned 1795133831
    c5a8596e start 1795133831        <- worker 2 starts
    5be68f1c end 1795133858          <- worker 1 still ran for 27 s
    c5a8596e end 1795133872
    turn watch flow 5be68f1c-...: no answer within 1s, 3 times; the turn is let go

`run.sh none` (control, ps fast): the runs are serial, `5be68f1c` `end 1795133972`, then
`de2ee847` `start 1795133973`, and server.log says `the run ended; the turn is let go` for each.
So the Sail `docker compose exec` chain is seen as a run for its whole length; only the slow `ps`
breaks it. Fails every run with a slow `ps`.

The machine itself was loaded during the first attempt (load average 24): both workers' hooks
logged `hook claude-code flow ...: no answer within 1s; the tool runs without the turn` and ran at
once, with no shim slowing anything. `ps -A -o pid=,ppid=,command=` took 0.81 s once there, and up
to 0.27 s under a plain 48-process CPU load.

## Root cause

`src/cli/commands/turn.rs:66-76` treats a process list that failed as a sign the turn is no longer
in use: three failed `processes::list()` in a row end `saaio turn`, which frees the flock, though
nothing says the run ended. `processes::list()` (`src/processes.rs:9`, `LIST_LIMIT` 1 s) fails
whenever `ps` takes past a second, which is exactly when a heavy run loads the machine (memory
pressure and swap make `ps` page in every process's arguments). So the run that loads the machine
loses its own turn, and the waiting worker starts a second heavy run beside it. Proven by the
reproduction: the same run serialises with a fast `ps` and overlaps with a slow one, with the
real log's line. Introduced with the watcher in 50df719 "turn: saaio turn keeps a flow's turn while
its agent runs the command", whose plan (`plan-exclusive-runs.md`) chose "3 failures in a row ->
release" as a fail-open.

The same 1 s limit makes the hook fail open before the turn is even taken:
`src/cli/commands/hook.rs:316` `agent_of` cannot list the processes and the call runs without the
turn (reproduced naturally above, and once in the real log).

Ruled out: "the run ended" misread for Sail or Docker (the control run holds the turn for the whole
`docker compose exec`); Claude Code's shells, `run_in_background` ones included, run as children of
the `claude` process with the command in their arguments (seen in this session's process table), so
they count as runs. The 309 `the run ended` lines seconds apart are short runs (pint, single tests)
ending; nothing points to them being early.

## Fix

A look that cannot list the processes knows nothing about the run, so it never lets the turn go;
only the agent being gone does, and whether the agent lives is asked of the kernel, which needs no
child process and has no deadline.

- `processes::LIST_LIMIT` goes from 1 s to 10 s: a loaded machine's `ps` gets the time it needs,
  for the hook (`agent_of`, which blocks a tool call that is about to wait anyway) and the watcher.
  The limit stays as the guard against a `ps` that hangs.
- `processes::alive(pid: u32) -> bool`: new, `rustix::process::test_kill_process` (signal 0) is
  `Ok`. (`Pid::from_raw` of 0 or a pid past `i32` is not alive.)
- `turn_watch::Look`: `runs: bool` becomes `runs: Option<bool>`, None when the look could not list
  the processes. `TurnWatch::step`: the agent gone releases (`AgentGone`) as now; `runs: None`
  holds, leaves `quiet` and `seen` as they are, and its time does not count against GRACE (like
  `asks`), since no one could see whether the run started. Doc comments say so.
- `cli/commands/turn.rs`: `LIST_FAILURES` and the release on failures go. Each step:
  `agent_alive = processes::alive(args.agent)`; `runs = processes::list().ok().map(|all|
  runs(&all, args, &exclusive))`. `look` becomes `fn runs(all: &[Process], args: &Args,
  exclusive: &Exclusive) -> bool` (the harness still left out). A failed list is logged once per
  stretch of failures: `could not list the processes (<error>); the turn is kept while the agent
  lives`, and `the processes list again` when one works, so server.log shows how long the watch was
  blind.
- `docs/repository.md` (Waiting for a turn): a turn is kept for as long as the run lasts even when
  the machine is too busy to answer quickly; it is let go early only when the worker is gone.
- `CHANGELOG.md` `## [Unreleased]` `### Fixed`: a heavy command no longer loses its turn while it
  runs when the machine is under load, so a second worker's heavy command waits for it.

## Also affected

- `hook.rs:316` `agent_of`, same limit (covered by the longer limit; still fails open past 10 s,
  as designed: the hook blocks a tool call).
- `hook.rs:221` `TURN_WAIT`: a call that waited 30 min runs beside the holder even when the holder
  still runs (once in the real log, 11-16). A different, deliberate valve; see open question 2.
- No stored data is wrong: the turn lives in a flock, and the `turn-*` events in the logs are true.

## Assumptions

- 10 s for `LIST_LIMIT`: long enough for a swapping machine, short enough that a hung `ps` does not
  stall a tool call noticeably more than the wait itself.
- Agent liveness from `kill(pid, 0)` in every look, not from the `ps` table, so both answers come
  from one source whether `ps` works or not. A zombie agent counts as alive until the server reaps
  it, which it does at once.
- The regression test is a `TurnWatch` unit test (pure, deterministic); the end-to-end reproduction
  covers the process side.

## Decisions

Approved by the user on 2026-11-20 with both recommendations.

1. While `ps` keeps failing and the agent lives, hold the turn without limit? - decided: yes.
   The report asks to fail open only when the holder is really gone. The worst case, `ps` broken
   for good, holds the turn until the agent exits; waiting workers still run after their 30 min
   (`TURN_WAIT`), so nothing deadlocks.
2. Change the 30 min wait (`TURN_WAIT`), after which a waiting call runs beside a holder that still
   runs? - decided: no, not in this fix. Claude Code ends a PreToolUse hook at its 1980 s
   timeout anyway, and a run past 30 min is rare; refusing the call instead would change what the
   model sees. Not added to `ROADMAP.md`.

## Tasks

- [x] Add `a_look_that_cannot_list_never_lets_go` in `src/turn_watch.rs` (a run seen, then looks
  with `runs: None` for 60 s hold, then `Some(false)` twice releases `RunEnded`; and before a run
  was seen, `None` looks past GRACE hold) - fails today to compile, `runs` is a `bool`; then change
  `Look::runs` to `Option<bool>` and `TurnWatch::step` as in Fix, the existing tests moved to
  `Some(..)`
- [x] Add `processes::alive` with a test (this process alive, a reaped child's pid not), raise
  `LIST_LIMIT` to 10 s, and rework `cli/commands/turn.rs` as in Fix: `runs()` replaces `look()`
  (its tests kept on the run detection), `LIST_FAILURES` gone, failures logged once per stretch
- [x] Docs and changelog: `docs/repository.md` Waiting for a turn, `CHANGELOG.md` Fixed
- [x] End-to-end: `cargo build`, then `run.sh 6` (ps 1.5 s) again and see the runs serial
  (worker 2's `start` after worker 1's `end`) with `the run ended; the turn is let go`, no
  `3 times`; `run.sh 6 12` (ps 12 s, past the new limit) serial too, server.log with `could not
  list the processes ...; the turn is kept while the agent lives`; and `run.sh none` still serial
