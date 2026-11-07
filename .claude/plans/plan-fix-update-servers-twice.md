# Fix: `saaio update` lists the same server twice

Report: after `saaio update` from 0.2.0 to 0.2.1 it printed `servers[2]` with two identical rows: store `~/.local/state/saaio/projects/saaio`, pid 3722, build `0.2.0 (162c94d)`, binary `~/.local/bin/saaio`. Only one server ran.

## Report

Expected: each running server of another build listed once.
Actual: the same server (same store, pid, build, binary) listed twice.
Steps and environment: `saaio update` from 0.2.0 to 0.2.1, macOS, the user's real shared folder `~/.local/state/saaio`.

## Reproduction

A release build of this branch (`SAAIO_RELEASE=v0.2.2 cargo build`, copied to the scratchpad as `saaio-new`), a local releases URL whose `VERSION` is `v0.2.2`, and the installed 0.2.1 as the older server:

    R=$(mktemp -d); H=$R/store; mkdir -p $H; export XDG_STATE_HOME=$R/state
    mkdir -p $R/rel/latest/download; echo v0.2.2 > $R/rel/latest/download/VERSION
    SAAIO_HOME=$H ~/.local/bin/saaio server &          # the 0.2.1 server
    SAAIO_HOME=$H saaio-new status >/dev/null           # registers stores/<fnv1a of $H>
    printf '%s\n' "$H" > $XDG_STATE_HOME/saaio/stores/7ee7933b94a1383c   # what the flow->saaio move left
    SAAIO_RELEASES_URL=file://$R/rel SAAIO_HOME=$H saaio-new update

    servers[2]{store,pid,build,binary}:
      /var/folders/.../store,24417,0.2.1 (88c1086),~/.local/bin/saaio
      /var/folders/.../store,24417,0.2.1 (88c1086),~/.local/bin/saaio

The same happens without the hand-written entry: `SAAIO_HOME=$H/ saaio-new status` (a trailing slash) registers a second entry, and `update` lists the server twice, once as `.../store/`. Fails every run. Ends with `SAAIO_HOME=$H saaio --stop`.

## Root cause

`src/registry.rs:79` `Registry::stores()` returns one `Store` per file in `stores/`, and nothing keeps two files from naming the same folder; `update::others` (`src/cli/commands/update.rs:82`) asks each one for its server, so a store with two entries shows its server twice.

Two ways write such a second entry:

1. **An entry named after another path.** `register` names the entry `fnv1a(root)` and skips writing when that file exists. The user's registry was moved from `~/.local/state/flow` to `~/.local/state/saaio` by hand for the rename (`plan-saaio-rename.md`, "After the merge" step 2), the contents rewritten, the file names kept. Proven on the user's real registry: `stores/7ee7933b94a1383c` holds `.../state/saaio/projects/saaio`, but `fnv1a` of that path is `c27da58b52265d0a` (also present, written by `register` at 08:03), and `7ee7933b94a1383c` is `fnv1a(".../state/flow/projects/flow")`. Five of the nine entries carry such names (`c35e69b590fc3b3d`, `17da5030b876d0ba`, `f5a8b1855c985f96`, `6949d87883e873cc` too), all dated Oct 18 08:01, the time of the move.
2. **Another spelling of the same folder.** `register` hashes the root's bytes as `Env::store` took them from `SAAIO_HOME`, so `$H` and `$H/` (or a path through a symlink) get two entries. The user's registry has one: `.../projects/holy-grail-ai-admin/` beside `.../projects/holy-grail-ai-admin`.

The store registry came in with `c7acbfd` (one store per repository); `stores()` never checked an entry's name against its content.

## Fix

- `Registry::stores()` lists each folder once and repairs the registry while it reads it, as it already drops vanished stores: it reads every entry, keys it by the folder's canonical path (`fs::canonicalize`, which the existing `is_dir` check guarantees exists), keeps one entry per folder, preferring the one whose name is `fnv1a` of its own line, and
  - removes the other entries of that folder,
  - and, when the kept entry's name is not `fnv1a` of its line (only a stale name exists), writes the entry under the right name with `write_atomic` and removes the stale file.
  The `Store` it returns keeps the root as its kept entry spells it, lexically normalised like `register`'s (not the canonical path), so the socket path `client::socket_path` derives for a long store stays the server's.
- `Registry::register()` and `Registry::entry()` work on the root lexically normalised (`root.components().collect::<PathBuf>()`: no trailing slash, no `.`), and `register` writes that normalised line, so `SAAIO_HOME=$H/` and `$H` share one entry. Signature unchanged: `fn entry(&self, root: &Path) -> PathBuf`.

Fixing in the registry, not deduplicating in `update::others`, fixes every reader at once.

## Also affected

- `saaio status --all` (`src/cli/commands/status.rs:83`): a store with two entries is listed twice, the second as `<name> (2)`, with the same flows.
- `held_slots` (`src/cli/commands/start.rs:286`): counts a store's slots twice; harmless (the list only excludes slots).
- The UI's mismatch warning (`binaries::mismatches`, called at `src/ui.rs:153`) is not affected: it takes the one server the UI is connected to.
- Damaged data: the user's real `~/.local/state/saaio/stores` holds five stale-named entries and one trailing-slash duplicate. The new `stores()` repairs them on the first `saaio update`, `status --all` or `start` after the fix is installed.
- Not changed: `client::private_socket` hashes the store root's spelling, so a long store reached through two spellings already gets two socket paths; that is a separate, pre-existing issue.

## Assumptions

- The regression test sits in `src/cli/commands/update.rs`'s tests beside `names_the_servers_on_another_version_with_tilde`, with the registry tests in `src/registry.rs`.
- The kept entry among several valid spellings of one folder is the first in `stores/` name order (`fs::read_dir` sorted), so the result is deterministic.

## Decisions

1. `stores()` rewrites and removes registry entries while listing them, repairing the user's registry from the move without a manual step. Decided: yes, the same way it already drops entries of stores that are gone.

## Tasks

- [x] Add `names_a_server_once_when_its_store_is_registered_twice` in `src/cli/commands/update.rs` (a store registered by `register` plus the same root under a stale entry name and as `<root>/`; fails today with `servers` holding 3 rows instead of 1), plus `lists_a_store_once_however_its_entries_name_it` in `src/registry.rs` (stale-named entries: one root each returned, one file left per store, named `fnv1a` of the root); then fix `Registry::stores()`
- [x] Add `registers_a_store_once_with_or_without_a_trailing_slash` in `src/registry.rs` (also: an entry left as `<root>/` is moved to the root's name; before normalising, `entry("<root>/")` is a name of its own, so this case belongs here), then normalise the root in `Registry::register()` and `Registry::entry()`
- [x] CHANGELOG: a `### Fixed` line under `## [Unreleased]`: `saaio update` and `saaio status --all` list each store once, and a registry with an entry left by an earlier path is repaired
- [x] End-to-end: run the reproduction again (both the stale entry and `SAAIO_HOME=$H/`) and see `servers[1]` with the 0.2.1 server once, and one file in `$XDG_STATE_HOME/saaio/stores`

## Review

Reviewed in isolation against `main`. The three new tests fail with `main`'s `src/registry.rs` (production code swapped back, tests kept) and pass with the fix.

**Fixed**

- `docs/files.md` still said a `stores/<hash>` entry is dropped only when its folder is gone; it now also says a second entry of the same folder is dropped (commit `docs: a store's registry entry is dropped for a second of one folder`). Docs only, no code changed.

**Open**

- Long store paths reached through two spellings. `Registry::stores()` now returns the root normalised (`$H/` read as `$H`) and keeps one spelling per canonical folder, while `client::private_socket` still hashes the root as `SAAIO_HOME` spelled it. For a store whose `server.sock` path is over 100 bytes, a server started with `SAAIO_HOME=$H/` (or through a symlink, when the other spelling's entry is the one kept) is now looked up at the other spelling's socket and missed by `saaio update`; before, it was found (twice). Narrow (needs a long store path plus an odd spelling; the default `~/.local/state/saaio/projects/<name>` is short), and the plan already names the socket hashing as a pre-existing issue. Recommendation: leave it for this fix; a follow-up normalises the root once in `Env::store` so the store, its socket and its entry agree.
- Symlink spellings churn: if two commands register one folder under two correctly hashed spellings (a symlink), each `register` rewrites its entry and the next `stores()` removes it again. Harmless (a small file write per command), noted only.

**Where to look**

1. `src/registry.rs` `Registry::stores()`: the sort key `(file != entry(root), file)` decides which entry survives, then the loop removes duplicates and moves a stale-named entry (write the right name first, remove the stale file only when that write succeeded, so no entry is lost).
2. `src/registry.rs` `normal`, `entry_line`, `read_entry`: every name and line goes through `normal`, so old `$H/` lines are read as `$H`.
3. `src/cli/commands/update.rs` `names_a_server_once_when_its_store_is_registered_twice`: the regression test over a real socket.

Routine, can be skimmed: the registry unit tests and the CHANGELOG line.

**Checks**

`cargo test`, `cargo clippy --all-targets -- -D warnings` and `cargo fmt` passed at `a8170c3`; production code has not changed since (only CHANGELOG.md, docs/files.md and this plan).
