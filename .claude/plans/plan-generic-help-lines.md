# Generic help lines

Review page: https://claude.ai/artifact/VCtxotAR57wZwjuZ2rzmFv

Rust quality audit item 9. `Failure::error` and `Failure::refused` take their help lines as any
iterable of strings, so callers pass plain arrays instead of `vec!["...".to_string()]`. No output
changes.

## Units

`src/cli/output.rs` (modified):

```rust
impl Failure {
    /// Exit code 1.
    pub fn error(
        message: impl Into<String>,
        help: impl IntoIterator<Item = impl Into<String>>,
    ) -> Failure;

    /// Exit code 1, no next steps.
    pub fn bare(message: impl Into<String>) -> Failure;

    /// Exit code 1, a refusal: printed with its `refusal` key.
    pub fn refused(
        refusal: Refusal,
        message: impl Into<String>,
        help: impl IntoIterator<Item = impl Into<String>>,
    ) -> Failure;
}
```

The `Failure` struct, `Failure::usage` and `to_toon` stay as they are.

## Callers

- `vec!["lit".to_string()]` -> `["lit"]`; `vec![format!(..)]` -> `[format!(..)]`.
- A list mixing literals and `format!` keeps `.to_string()` on the literals (one element type).
- A help `Vec` built in branches or extended stays a `Vec`.
- `Vec::new()` -> `Failure::bare(message)` (the anyhow and io `From` impls, `send.rs`, tests).
- `src/cli/commands/update.rs:36`: `Failure { code: 2, ..Failure::error(message, [help]) }`.

## Test seams

No new tests: the `to_toon` tests in `output.rs`, the document tests in `output/*` and the CLI
tests in `tests/cli/` prove the output is unchanged.

## Assumptions

- No CHANGELOG line: nothing a user sees changes.
- The documents' own serialised `help: Vec<String>` fields stay.

## Tasks

- [x] Make `Failure::error` and `Failure::refused` take `impl IntoIterator<Item = impl Into<String>>`, add `Failure::bare`, move every caller to plain arrays or `bare`, and `update.rs:36` onto `Failure::error`; build, clippy and the full suite green
- [x] End-to-end check: failing commands (`saaio report` on an unknown flow, `saaio update --version x`, `saaio send` with nothing) print the same TOON as on main

## Review

Reviewed `flow/generic-help-lines` against `main` in an isolated context.

**Fixed** - nothing; no finding.

**Open** - none. Checked and let go:
- Every `Failure::error`/`refused` caller in `src/` moved; no `vec!["..".to_string()]` single-type list or `Vec::new()` help is left. The four remaining `.to_string()` calls sit in lists mixing literals and `format!` (`commands.rs` `no_flow`, `output/update.rs` `source` and `failed`, `output/trust.rs` `untrusted`), as the plan allows.
- `Failure::bare` keeps an empty `help`, so the TOON still prints no `help` key (`prints_no_refusal_for_a_plain_error`).
- `update.rs` `--version` keeps exit code 2 through the struct update over `Failure::error`.
- Branch-built and extended help (`start.rs` `pick_base`, `output/remove.rs` `unmerged`) stays a `Vec`, now `Vec<&str>` in `pick_base`.

**Where to look** - `src/cli/output.rs` `Failure::error`, `bare`, `refused`: the only signature change; the rest is a mechanical caller sweep and can be skimmed.

**Checks** - `cargo test --all-features`, clippy `-D warnings` and `cargo fmt --check` green at 5bc4fa5; only this plan changed after it.
