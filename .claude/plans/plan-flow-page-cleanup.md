# Plan: the master offers to delete a removed flow's review pages

## Decision

Workers leave review pages behind: local HTML under the worktree's `.lavish/` and hosted pages (claude.ai
Artifacts or the like). When the master asks to remove or clear a flow, it offers in the same line to delete
them, and deletes them only on the user's yes. Local pages go before the remove, since they are untracked
files that block it. The master finds hosted pages in the worker's answers (`flow status <name> --full`,
the last answer only) and in its harness's own page list, by title. The text names no harness tool.

As with every change to the default prompt, the current `master.md` becomes `master-v3.md` in
`PREVIOUS_DEFAULTS`, so an unedited master.md is upgraded.

## Units

`src/server/master-v3.md` (new): the current `src/server/master.md`, byte for byte.

`src/server/master.rs` (modified)

```rust
const PREVIOUS_DEFAULTS: [&str; 3] = [
    include_str!("master-v1.md"),
    include_str!("master-v2.md"),
    include_str!("master-v3.md"),
];
```

`src/server/master.md` (modified): after the `flow remove` / `flow clear` bullet:

```
- A removed flow's worker may leave review pages: HTML under its worktree's `.lavish/`, and hosted pages
  such as claude.ai Artifacts. When you ask to remove or clear a flow, offer in the same line to delete
  them, and delete them only on the user's yes. Delete the `.lavish/` pages before the remove: they are
  untracked files that block it. Find the hosted ones in the worker's answers (`flow status <name>
  --full`) and in your harness's own list of pages, by title.
```

## Test seams

`master.rs` tests already cover it: `ensure_instructions` upgrades every entry of `PREVIOUS_DEFAULTS`
(now including v3) and asserts each differs from `DEFAULT_INSTRUCTIONS`. No golden pins the prompt.

## Assumptions

- One yes covers the local and hosted pages; a no keeps both (the remove then needs `--force`, which the
  master already asks about).

## Tasks

- [x] Copy `master.md` to `master-v3.md`, add it to `PREVIOUS_DEFAULTS`, add the review-pages bullet to `master.md`; `cargo test`, `cargo clippy -- -D warnings`, `cargo fmt --check` pass
- [x] End-to-end: with `FLOW_HOME` and `XDG_CONFIG_HOME` in a scratch dir and `master.md` holding the v3 text, start `flow server`; check `master.md` now holds the new text, then stop the server

## Review

Reviewed `flow/offer-page-cleanup` against `main` (commits cdacb23..39385af) in an isolated context.

- **Fixed** - nothing; no finding was proven.
- **Open**
  - A repo that gitignores `.lavish/` does not have its pages block `flow remove` (`has_uncommitted_work`
    reads `git status --porcelain`, which skips ignored files), so "they are untracked files that block it"
    is true only for the usual case. Harmless: deleting them first is right either way. Recommendation: keep.
  - Hosted pages are found only in the worker's last answer (`--full`) and the harness's list by title; a
    page announced in an earlier answer with an unrelated title can be missed. Accepted by the plan's
    decision; recommendation: keep, revisit if pages get left behind in practice.
- **Where to look**
  1. `src/server/master.md`, the new bullet after `flow remove` / `flow clear`: the only behaviour change,
     a prompt to the master. Read it as the master would: offer in the same line, delete only on yes,
     `.lavish/` before the remove.
  2. `src/server/master.rs` `PREVIOUS_DEFAULTS`: v3 added so an unedited master.md upgrades;
     `replaces_an_earlier_default_nobody_edited` covers it.
  - Routine, skim: `src/server/master-v3.md` is main's `master.md` byte for byte (checked with `cmp`).
- **Checks** - `cargo test`, `cargo clippy -- -D warnings`, `cargo fmt --check` passed at 37b14fb; later
  commits change only this plan.
