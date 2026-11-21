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
- [ ] End-to-end check: failing commands (`saaio report` on an unknown flow, `saaio update --version x`, `saaio send` with nothing) print the same TOON as on main
