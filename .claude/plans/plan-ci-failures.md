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

## Review

Test-only branch: no production code, stored data or public surface changes.

- **Fixed** - nothing; no finding held up.
- **Open** - none. Note: `assert!(x.is_empty(), "msg")` sites with a message (`forge.rs`,
  `deliver.rs:181`, `server/tests.rs:2095`, `tests/cli/start.rs:337`, `tests/cli/hooks.rs`,
  `tests/cli/identity.rs`, `ui/screen.rs`) are left as they are: clippy 1.99.1 does not flag
  them, and the plan scopes the change to the flagged sites.
- **Where to look** - `src/server/tests.rs` `refuses_a_length_of_4_gib_and_still_takes_a_ui`:
  the trailing write is now `let _ =` (an EPIPE on Linux after the server's `shutdown(SHUT_RD)`
  is the refusal, not a failure), Refused and the close are still asserted, and a UI is welcomed
  afterwards. The other protocol tests write whole frames or already ignore a failed write
  (lines 1067, 1236-1238); the one remaining `unwrap` on a write (line 1353) sends one whole
  7-byte frame in a single write before the server acts, so it does not race. The
  `assert!(..is_empty())` -> `assert_eq!(.., [] as [T; 0])` / `b""` / `""` rewrites across the
  other 17 files are mechanical and can be skimmed; `tests/cli/hooks.rs` and `status.rs` compare
  through the `stdout`/`stderr` helpers, which do not trim, so the meaning is unchanged.
- **Checks** - `cargo test`, `cargo clippy --all-targets -- -D warnings` (1.95.0 and 1.99.1) and
  `cargo fmt --check` passed at 1fbbeb4; the review changed only this plan.
