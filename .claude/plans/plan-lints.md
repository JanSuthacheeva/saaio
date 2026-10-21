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

- `std::env::var`, `var_os`, `vars`, `vars_os`, `set_var`, `remove_var`: read in main.rs into `cli::Env`
- `std::fs::write`, `std::fs::File::create`, `File::create_new`, `std::fs::OpenOptions::new`,
  `std::fs::create_dir`, `create_dir_all`: create files through private.rs

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
- [ ] CLAUDE.md and CONTRIBUTING.md name the rules
- [ ] End to end: clippy, test and fmt pass; each new rule proved once by a throwaway violation
