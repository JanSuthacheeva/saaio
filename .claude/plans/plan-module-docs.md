# Module docs (structure audit item 6 and the module-docs finding)

Audit: https://claude.ai/artifact/Lpm6RTzVR5vCuAU1UibgjE

## Decision

CLAUDE.md matches main exactly: every `.rs` file under `src/` and `build/` is named in the
Architecture list (test-only `tests.rs`, `tests/*.rs` and `fixtures.rs` files through the rule
in Structure or by name), every name, path and identifier it gives exists, and every claim it
makes about a file holds. Every module gets one `//!` line at its top saying what it holds, in
CLAUDE.md's voice (nouns, no "this module"), so the description lives next to the code. No
behaviour change; no CHANGELOG entry (nothing a user sees).

## CLAUDE.md rules

- Structure: "Paths are `use crate::...`, except `super::` from a child to its parent."
- Build Commands, the test rule: a fixed sleep is allowed only to assert that nothing happens.
- Build Commands, E2E runs: a run that starts `saaio server` in a scratch `SAAIO_HOME` always
  ends with `saaio --stop` on that store, so no test server with live sessions is left behind.

## Tasks

- [x] Verify every Architecture and Structure claim in CLAUDE.md against main; list the drift
- [x] Fix the Architecture list: add the unlisted files, drop or correct what no longer holds
- [x] Amend the rules: `super::`, fixed sleeps, `saaio --stop` after a scratch server
- [x] Add one `//!` line to every module in `src/`, `build/`, `build.rs` and `tests/`
- [x] fmt, clippy, test; nothing extra left on disk
