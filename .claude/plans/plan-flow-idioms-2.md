# flow: Rust idioms audit 2

## Scope and bar

Audit `git diff e2af367 HEAD -- src build.rs Cargo.toml` (PR #15 merged at
`e2af367`, 2026-10-16). Apply the [unofficial Rust idioms](https://rust-unofficial.github.io/patterns/),
the [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/checklist.html),
the pedantic Clippy configuration in `Cargo.toml`, and `CLAUDE.md`'s file structure and naming
rules. Fix real defects and useful idiom violations; do not churn code for cosmetic preferences.

Carry forward the prior audit's deliberate choices: `named(&str)` / `name()` pairs instead of
`FromStr` / `Display`, pure `Vec<String>` argv, `bool` parameters mirroring one CLI flag,
and TOON/JSON field names. Keep the named `Cargo.toml` lint allowances unless a specific use
proves wrong. The prior plan is `.claude/plans/plan-flow-idioms.md`.

`cargo clippy --all-targets -- -D warnings` passes at this audit. No UI run, terminal capture,
source fix, or test change was made.

## Findings

### Bugs

1. **A closed PR can leave another PR displayed as open** — `src/forge.rs:231`.
   `news` only emits `MergeRequestClosed` when the selected closed PR's URL equals
   `flow.merge_request`. `pick` (`src/forge.rs:238`) selects the first closed PR by list order.
   Reproduce with a flow showing PR A, both A and a newer PR B closed on its branch: `pick`
   returns B, `news` returns `None`, and A stays in status. Decide closure from the status of
   the currently shown PR, or clear the shown URL when no open/merged PR remains. Add a
   regression test for two closed PRs on one branch and for the prior URL disappearing from
   the query result.

2. **The forge timestamp parser accepts impossible dates and offsets** — `src/forge.rs:180`.
   `unix_ms` checks only day 1..31, so `2026-02-31T00:00:00Z` becomes a March timestamp;
   offset hours/minutes have no range check (`src/forge.rs:175`). `Forge::parse` promises an
   error for an unreadable time, yet these inputs become valid `Found` records and may pass
   the `since` filter. Validate actual Gregorian days and RFC 3339 offset bounds, with tests
   for leap/non-leap February and invalid offsets. Keep the parser dependency choice small.

3. **A PR opened in the flow's first second can be ignored** — `src/forge.rs:246`.
   `pick` compares a forge timestamp parsed to whole-second precision against
   `flow.started_at` in milliseconds (`src/forge.rs:216`). For a flow started at
   `12:00:00.500` and a PR opened at `12:00:00.700` whose forge timestamp is
   `12:00:00Z`, `created_at < since` and the PR is discarded. Test that boundary and use a
   conservative precision-aware cutoff that still rejects genuinely older PRs.

### Idioms and structure

4. **New internal types lack `Debug`** — `src/adapter/codex.rs:15`,
   `src/cli/commands/hook.rs:27`, `src/cli/commands/hook.rs:75`,
   `src/cli/commands/start.rs:219`, `src/forge.rs:43`, `src/forge.rs:52`,
   `src/registry.rs:17`, `src/ui/markdown.rs:32`.
   The prior audit required `Debug` on every type, matching API Guidelines
   C-COMMON-TRAITS. Derive it on these types. `server/host.rs::Process` already has a
   manual implementation for its foreign fields. Do not change runtime behavior.

5. **Theme file passes the production-line split threshold with two seams** —
   `src/ui/theme.rs:10`, `src/ui/theme.rs:401`. Its first section defines built-in palettes,
   palette keys, and theme-file section names; the later section builds and applies widget
   styles. Production code ends around line 531, above `CLAUDE.md`'s roughly 400-line rule.
   Split along the palette/style boundary into noun-named sibling modules while keeping the
   current natural public paths. `src/fold.rs` is also about 500 production lines, but its
   long, uniform event match is the explicit exception documented by the existing
   `too_many_lines` allowance, so leave it.

6. **A new function repeats its module name** — `src/launch/name.rs:3`.
   `name::flow_name` violates `CLAUDE.md`'s rule that functions do not repeat their module's
   noun. Choose a short name that reads naturally at call sites, and update only those calls.

## Tasks

- [ ] Prove and fix the stale closed-PR case in `forge::news`/`pick`.
- [ ] Reject impossible forge timestamps in `forge::unix_ms` with focused tests.
- [ ] Preserve PRs opened in the flow's first second in `forge::pick` with a boundary test.
- [ ] Add `Debug` to the listed new types.
- [ ] Split `ui/theme.rs` at the palette/style seam and adjust imports without behavior change.
- [ ] Rename `launch::name::flow_name` and its call sites.
- [ ] Run `cargo test`, `cargo clippy --all-targets -- -D warnings`, and `cargo fmt --check`; review the resulting diff against this plan.

Stop here for review before beginning any task.
