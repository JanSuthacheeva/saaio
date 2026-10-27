# Plan: two CI failures - a refusal test that races the close, and clippy's assert-empty lint

## Request

(1) `server::tests::refuses_a_length_of_4_gib_and_still_takes_a_ui` failed on main on Linux
(run 40055009587, `src/server/tests.rs:1156`) with BrokenPipe. Reproduce it, make the test not
depend on that timing while still asserting Refused and the close, and check the other protocol
tests in `server/tests.rs` for the same race. (2) The weekly stable workflow fails on clippy
1.99.1 (run 40324170375): a new lint, "used `assert!` to check that a value is empty", fires
across the tests. Fix every site the way the lint suggests, clean on the pinned 1.95.0 and on
1.99.1; never allow the lint. No CHANGELOG entry: nothing users see.

## Root cause (1)

The test writes the 4 GiB length, then 1024 bytes in a second `write_all`. The server's reader
fails on the length at once, `refuse_unreadable` queues Refused and `drop_conn` shuts the
stream's reading side. On Linux, `shutdown(SHUT_RD)` on a Unix stream also marks the peer's
sending side shut, so the client's next write fails with EPIPE; macOS does not, which is why it
fails only on Linux. Whenever the server refuses before the second write, the test panics.

Reproduced in Linux (rust 1.95 container, 16 copies of the test looping at once): 15 of 960 runs
failed with the CI's BrokenPipe at line 1156.

The other protocol tests write one whole frame before the server acts on it (the server reads a
frame whole before it answers), or already ignore a failed write (`send_noise`,
`closes_a_connection_whose_hello_is_not_whole_by_the_deadline`, the stuck UI's `wait_until`):
none races the close.

## Decision

The trailing bytes may meet a closed stream: that is the refusal, not a failure, so their write
is let go (`let _ =`, as `send_noise` does). The test still asserts Refused and the close, and now
does what its name says: a UI connects after it and is welcomed.

## Tasks

- [x] `refuses_a_length_of_4_gib_and_still_takes_a_ui`: the trailing write may fail; a UI is
      welcomed after the refusal. Proof: with a forced ordering (the trailing write held back
      until the server refused) the old test fails every time and the new one passes; the Linux
      loop passes.
- [x] Every `assert!(x.is_empty())` and `assert!(!x.is_empty())` clippy 1.99.1 flags rewritten as
      it suggests; `cargo clippy --all-targets -- -D warnings` clean on 1.95.0 and 1.99.1, tests
      green.
- [x] `the_server_writes_to_the_new_log_once_another_process_moved_it_aside` (CI on #103,
      macOS): the server decided whether stderr is server.log (`is_stderr`) only after the bind,
      write_defaults, the master record and the host. A writer that moved the log aside in that
      window (the test's hook, right after the socket appears) left `log` None: the server wrote
      to server.log.1 for good. Reproduced on macOS under load (10 `yes`, 12 copies): 4 of 180;
      with a 1 s sleep after the start line it failed every time. Fix: `server_log::writes_to`,
      stderr is the file at `log` or at `<log>.1`; `follow` then reopens server.log on its next
      tick. Forced ordering passes; loop under load 0 of 300. CHANGELOG: Fixed, as users see it.
      Review: `a_server_whose_log_moved_aside_before_it_looked_writes_to_the_new_log` pins it
      without timing (the log is renamed before the server starts on it); without `writes_to`'s
      `<log>.1` it fails every time.
- [x] `a_ui_that_stops_reading_slows_nobody_and_is_dropped` (master, after #103's CI): its 500 ms
      budget failed on a loaded machine (3 of 3 full runs locally; 77 of 96 runs at 12 copies
      and 10 `yes`), while the regression it guards stalls WRITE_TIMEOUT, 2 s. The write timeout
      moves into `Timing` (`write`, 2 s by default); the test runs with 10 s and asserts each
      answer within half of it. Sampled under load, the late answers were the server encoding
      the flood's frames (a 200x100 screen 20 times a second), not a wait: the pane is now
      100x50 and scrolls 4 times a second, which still fills the stuck UI's socket, and the
      control requests run until the stuck UI is dropped (by 2 x `write`: a write that got
      partway waits once more), so they span the whole stall. Proof: a send that waits for
      earlier messages (the old synchronous server) fails it, "answered after 20s"; 0 of 48 at
      12 copies and 10 `yes`. The Outbox unit test's 1 s budget compares against a 20 s stall
      the same way.
- [x] `a_pane_that_prints_without_pause_slows_nobody`, the same 500 ms budget: 6 of 8 failed
      beside 10 `yes`. Without the backlog's cap the server never answers (recv's 30 s PATIENCE
      fails it), so the budget only adds flakes and goes; 0 of 48 at 12 copies and 10 `yes`,
      and the cap-less server still fails it.

## Review

Two test fixes (the 4 GiB refusal race, clippy 1.99.1's assert-empty lint), one production fix
(`server_log::writes_to`, with a CHANGELOG line) and one production refactor (the write timeout
moved into `Timing::write`, default still 2 s). No stored data or public surface changes.

- **Fixed** - the server-log fix had only a timing-dependent regression test (the CLI test that
  flaked). Added `a_server_whose_log_moved_aside_before_it_looked_writes_to_the_new_log`: the
  log is renamed to server.log.1 before the server starts on it, so a server that checks only
  server.log fails it every time (shown by reverting `writes_to`). Test-only, so not reviewed
  again.
- **Open** - none blocking. Notes: `a_ui_that_stops_reading_slows_nobody_and_is_dropped` now
  runs about 20 s (the stuck UI is dropped only after 1-2 x the 10 s `write`), the price of a
  budget far from a loaded runner's hiccups; recommend keeping it. `writes_to` misses a log moved
  aside twice before the server looks (server.log.1 replaced too); that needs 2 MiB written in
  the server's first moments, so leave it. `assert!(x.is_empty(), "msg")` sites with a message
  stay as they are: clippy 1.99.1 does not flag them.
- **Where to look** -
  1. `src/server_log.rs` `writes_to` -> `src/server.rs` `run` (`log: ... .filter(writes_to)`)
     -> `follow`: stderr at `<log>.1` now counts as server.log, and `follow` reopens server.log
     on its next tick since `is_stderr(log)` is false. A server in a terminal (stderr a tty or
     pipe) still gets `log: None`.
  2. `src/server/tests.rs` `a_ui_that_stops_reading_slows_nobody_and_is_dropped`: probes the
     stuck UI with an empty `Input` (a no-op write to the focused pane, unlike the old
     `History`, which sent a full frame) until it is dropped, asserting each control answer
     within `stall / 2`; `a_pane_that_prints_without_pause_slows_nobody` drops its budget and
     relies on recv's 30 s PATIENCE.
  3. `src/server/tests.rs` `refuses_a_length_of_4_gib_and_still_takes_a_ui`: the trailing write
     is `let _ =` (EPIPE on Linux after the server's `shutdown(SHUT_RD)` is the refusal), Refused
     and the close still asserted, a UI welcomed afterwards.
  Routine, skim: `Timing::write` threading through `Outbox::start` and its unit tests, the
  CLAUDE.md line, and the `assert!(..is_empty())` -> `assert_eq!(.., [] as [T; 0])` / `b""` /
  `""` rewrites across 17 files (`tests/cli` helpers do not trim, so meaning is unchanged).
- **Checks** - `cargo test`, `cargo clippy --all-targets -- -D warnings` (1.95.0 and 1.99.1)
  and `cargo fmt --check` passed at b0b29d9 (CI green). After the review's test: fmt, clippy on
  1.95.0 and 1.99.1 and `cargo test --test cli server::` pass; nothing else changed.
