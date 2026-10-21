# Lint rules (structure audit items 7 and 10)

Review page: https://claude.ai/artifact/8EZojHsqJkZUe3AHyA9SW3

## Decision

Keep the ten house-style global allows. Move the six narrow ones to `#[expect(..., reason = "...")]`
at their sites. Deny `unsafe_code` with a local allow at each unsafe statement, and warn on
`clippy::undocumented_unsafe_blocks` (not `unreachable_pub`). A new `clippy.toml` bans env reads
outside `main.rs`/`build.rs` and file creation outside `private.rs`/`store.rs`. Tests get one
test-only allow at each crate root. No behaviour change.

## Cargo.toml

```toml
[lints.rust]
unsafe_code = "deny"

[lints.clippy]
undocumented_unsafe_blocks = "warn"
# removed with their comments: struct_excessive_bools, too_many_lines, maybe_infinite_iter,
# unused_self, unnecessary_wraps, option_option
```

## clippy.toml

`disallowed-methods`, each with a reason:

- `std::env::var`, `var_os`, `vars`, `vars_os`, `set_var`, `remove_var`, `home_dir`, `temp_dir`: read in main.rs into `cli::Env`
- `std::fs::write`, `std::fs::File::create`, `File::create_new`, `File::options`, `std::fs::OpenOptions::new`,
  `std::fs::copy`, `std::fs::create_dir`, `create_dir_all`, `DirBuilder::new`: create files through private.rs

## Local exceptions

- `main.rs`, the fn that fills `Env`: `#[allow(clippy::disallowed_methods, reason)]`
- `build.rs` `main`: same
- `private.rs`, `store.rs`: module-level allow
- `workspace.rs`, the info/exclude writer: allow (git's file, not the store's)
- `lib.rs`: `#![cfg_attr(test, allow(clippy::disallowed_methods, reason))]`;
  `tests/cli/main.rs`, `tests/install.rs`: `#![allow(clippy::disallowed_methods, reason)]`
- `client.rs` (2x `peer_pid`, the `pre_exec`), `text.rs` (`local_offset_secs`):
  `#[allow(unsafe_code, reason)]` on the unsafe statement, SAFETY comments kept
- `tests/cli/server.rs:101`: `rustix::process::kill_process` instead of `libc::kill`

## The 18 expect sites

- too_many_lines: `fold.rs` apply, `cli/output/status.rs`, `ui.rs`, `ui/sidebar/rows.rs`, `ui/app/tests/golden.rs`
- unused_self: `ui/outer.rs` `Outer::size`, `ui/app/draw.rs` `draw_history`
- unnecessary_wraps: `cli/commands/home.rs` `run`, `cli/commands/status.rs` `all_stores`, `build/commit.rs`
- option_option: `config/watch.rs` `ConfigWatch::stamp`, `server/host.rs` test `pump`
- struct_excessive_bools: `proto.rs` `Modes`, `cli/env.rs` `Env`, `ui/markdown.rs` `Renderer`
- maybe_infinite_iter: `cli/commands/start.rs`, `cli/commands/status.rs`, `launch/name.rs`

## Also

- `adapter/codex.rs`: the two `unwrap()` become `expect("a map of strings serialises")`.
- CLAUDE.md (Structure) and CONTRIBUTING.md name the rules. No CHANGELOG line.

## Assumptions

1. `allow`, not `expect`, for `unsafe_code` and `disallowed_methods`.
2. The env rule also bans `vars`, `vars_os`, `remove_var`; `current_dir` stays allowed.
3. Removing, renaming and `set_permissions` stay allowed.
4. `undocumented_unsafe_blocks` at warn; CI's `-D warnings` makes it an error.

## Tasks

- [x] Move the six narrow allows to `#[expect]` at the 18 sites, drop them and their comments from Cargo.toml
- [x] Deny `unsafe_code`, warn `undocumented_unsafe_blocks`; local allows at the 4 unsafe statements; rustix kill in tests/cli/server.rs
- [x] Add clippy.toml disallowed-methods with the local and test-only allows
- [x] `expect` with a reason for the two unwraps in adapter/codex.rs
- [x] CLAUDE.md and CONTRIBUTING.md name the rules
- [x] End to end: clippy, test and fmt pass; each new rule proved once by a throwaway violation

### No unsafe (the user, after the review)

Ideally no unsafe at all; keep a block only when no safe way exists. Review's open items: both left
as they are (clap's env read runs inside main.rs's parse; CI compiles the Linux peer_pid).

- `client::ensure_server`: no `pre_exec`. `saaio server` calls `rustix::process::setsid()` itself
  as it starts (ignoring EPERM, a process group leader started by a shell), so the server keeps its
  own session and no controlling terminal, as before. `process_group(0)` alone would keep the
  terminal and makes setsid impossible.
- `client::peer_pid`: `nix::sys::socket::getsockopt` with `sockopt::LocalPeerPid` (macOS) and
  `sockopt::PeerCredentials` (Linux), still refusing pid 0 (`seen_pid`). nix 0.28, the version
  portable-pty already builds, feature `socket`: no new crate compiled.
- `text::local_offset_secs`: `jiff::tz::TimeZone::system()` and `Timestamp::now()`; like
  localtime_r it reads TZ, then /etc/localtime. New dependency jiff (MIT OR Unlicense).
- All four gone: `unsafe_code = "forbid"`, `undocumented_unsafe_blocks` dropped (nothing left for
  it), libc dropped if unused. CLAUDE.md and CONTRIBUTING.md say unsafe is forbidden.

- [x] Server's own session through rustix setsid in `saaio server`, no pre_exec
- [x] peer_pid through nix's safe getsockopt on macOS and Linux
- [x] local_offset_secs through jiff
- [ ] unsafe_code = "forbid", drop undocumented_unsafe_blocks and unused libc; docs
- [ ] End to end: the UI starts the server in its own session (ps shows sess/tty), saaio --stop finds its pid, log stamps carry the local offset

## Review

### Fixed

- `clippy.toml` left ways round its own rules: `File::options` (the same as `OpenOptions::new`),
  `DirBuilder::new`, `fs::copy`, `env::home_dir` and `env::temp_dir` (HOME, TMPDIR) all passed
  clippy in production code, shown by a throwaway function in `text.rs`. Now banned; the throwaway
  fails on all five and the tree is clean (430d0f7). Config only, no production code changed.

### Open

- `#[arg(env = "SAAIO_FLOW_ID")]` in `cli/commands/report.rs` reads the environment through clap,
  which no lint sees. It runs inside `Cli::try_parse` in `main.rs`, so it keeps to the rule's
  intent; recommend leaving it.
- The Linux `peer_pid` in `client.rs` (the allow, then the SAFETY comment, on a `let`) was only
  compiled by CI's Linux job, not here. It has the same shape as the macOS one that passes, so the
  risk is small; CI shows it.

### Where to look

1. `clippy.toml` against the exceptions: `private.rs`, `store.rs` (module allows), `workspace.rs`
   `exclude_worktrees`, `main.rs` `env`, `build.rs` `main`. Whether the list of banned methods is
   the rule you mean, and whether the module-wide allow in `store.rs` is wider than it needs to be.
2. `src/lib.rs` `cfg_attr(test, allow(...))`: every test in the crate is exempt, so the rules check
   production code only.
3. `src/client.rs` and `src/text.rs`: the four `#[allow(unsafe_code)]` sites and their SAFETY
   comments.

Routine, skim: the 18 `#[expect]` sites (each a lint moved from `Cargo.toml` to its site with a
reason; an unneeded one would fail as an unfulfilled expectation), the two `expect`s in
`adapter/codex.rs`, the rustix `kill_process` in `tests/cli/server.rs`, and the docs lines.

### Checks

fmt, clippy, test and deny passed at 045d9a3; after the fix, which changes only `clippy.toml`,
`cargo clippy --all-targets -- -D warnings` and `cargo fmt --check` pass at 430d0f7. Tests were not
rerun: no compiled code changed. The fix was reviewed in a separate pass by the reviewer, not by an
independent agent.
