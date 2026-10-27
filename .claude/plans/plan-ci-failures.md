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
- [ ] Every `assert!(x.is_empty())` and `assert!(!x.is_empty())` clippy 1.99.1 flags rewritten as
      it suggests; `cargo clippy --all-targets -- -D warnings` clean on 1.95.0 and 1.99.1, tests
      green.
