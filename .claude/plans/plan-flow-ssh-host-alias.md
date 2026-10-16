# Plan: merge requests behind an SSH host alias

Review page: https://claude.ai/artifact/FTYZT6AcuGkPGLD8jXXohH

## Decision

A repo cloned as `git@github-work:org/repo.git` opens `https://github.com/org/repo/pull/5`, and
`flow hook` drops it because `calls::in_repo` compares the host literally. `in_repo` takes a
resolver from an SSH host to its real host and uses it only when the remote is in SSH form, its
path matches and its literal host does not. The hook passes `workspace::ssh_hostname`, which
reads `ssh -G`; tests use a fake resolver.

## Units

`src/adapter/calls.rs` (modified):

```rust
/// True when `url` is a merge request of the repo `remote` points to: same host, its path
/// followed by /pull/ or /-/merge_requests/. An SSH remote's host may be an alias from the
/// ssh config: when only the host differs, `real_host` names the host it stands for.
pub fn in_repo(url: &str, remote: &str, real_host: impl Fn(&str) -> Option<String>) -> bool;

/// git@host:path or ssh:// (git+ssh://, ssh+git://): a remote ssh connects to.
fn is_ssh(remote: &str) -> bool;
```

`host_and_path` unchanged. `in_repo` checks the path first, then the literal host, and calls
`real_host` only on a host mismatch of an SSH remote, comparing its answer lowercased.

`src/workspace.rs` (modified):

```rust
/// ssh -G -- <host>: the hostname its ssh config gives it (the host itself when no rule
/// names it); None when ssh does not run or prints none.
pub fn ssh_hostname(host: &str) -> Option<String>;
```

Runs through `stdout_of`, reads the `hostname ` line.

`src/cli/commands/hook.rs` (modified): `of_own_repo` calls
`calls::in_repo(url, remote, workspace::ssh_hostname)`.

## Data

None new. No stored format, flag or output changes.

## Call chain: `flow hook` sees `gh pr create`

1. `hook::record` -> `of_own_repo(store, id, url, now): bool`
2. `of_own_repo` -> `workspace::remote_urls(dir): Vec<String>`
3. for each remote: `of_own_repo` -> `calls::in_repo(url, remote, workspace::ssh_hostname): bool`
4. `in_repo` -> `host_and_path(url)`, `host_and_path(remote)`; no path match -> `false`, no ssh
5. hosts equal -> `true`, no ssh
6. `in_repo` -> `is_ssh(remote)`; https -> `false`
7. `in_repo` -> `ssh_hostname(repo_host): Option<String>` -> `stdout_of(ssh -G -- host)`; the `hostname` line
8. `Some(real)` equal to the URL's host -> `true`; else `false`

Errors: ssh missing or failing -> `None` -> `false`, as today; the hook prints its existing
"is not a merge request of this flow's repository". An unknown alias resolves to itself -> no match.

## Test seams

- `calls::in_repo` unit tests with a fake resolver: an alias resolving to github.com matches; a
  plain host matches with a resolver that panics; an unknown alias (resolver returns it
  unchanged) does not match; an https remote never asks; a non-matching path never asks. Existing
  cases pass `|_| None`.
- `workspace::ssh_hostname` unit test with the real ssh: an unknown name resolves to itself.
  Decided in review: it must not fail on a machine without ssh - it skips when ssh does not run.
- `tests/cli.rs` E2E: origin `git@github-work:shop/app.git`, a fake `ssh` first on PATH printing
  `hostname github.com`; `flow hook` with `gh pr create` output records the PR. Written first, to
  reproduce the bug.

## Assumptions

- Only SSH forms resolve: scp-like `host:path`, `ssh://`, `git+ssh://`, `ssh+git://`; https does not.
- ssh runs at most once per remote whose path matches, only on merge request creation; no cache.
- The URL's host is never resolved; only the remote's.
- A `Match exec` rule in the user's ssh config may run during `ssh -G`; their own config, accepted.
- The E2E test fakes ssh on PATH rather than writing an ssh config.

## Tasks

- [x] Add the E2E test in `tests/cli.rs` (alias origin, fake ssh on PATH) and see it fail
- [x] Add `workspace::ssh_hostname(host)` with a unit test that an unknown name resolves to itself, skipped without ssh
- [x] Give `calls::in_repo` the `real_host` resolver and `is_ssh`, with unit tests for an alias, a plain host, an unknown alias, an https remote and a non-matching path; pass `workspace::ssh_hostname` from `hook::of_own_repo`; the E2E test passes
- [x] End-to-end check: run `flow hook` by hand on a scratch store against a repo with an alias origin, and confirm the merge request is recorded

## Review

Reviewed in an isolated context against this plan (main...8dd5ca0). The change matches the plan's
units, signatures and call chain; no production-code findings.

### Fixed

- `is_ssh`'s `git+ssh://` and `ssh+git://` arms had no test: dropping either passed every test.
  Unit cases for both, and for a `file://` remote that never asks ssh (8dd5ca0, test only).

### Open

- An alias whose `HostName` is a provider's alternate SSH endpoint does not match: `Host
  github-work` with `HostName ssh.github.com` / `Port 443` (GitHub over 443), or GitLab's
  `altssh.gitlab.com`, resolves to a host that is not the PR URL's `github.com`, so the merge
  request is still dropped as before. Recommendation: leave it for now (no regression, rare);
  if it comes up, map those two known endpoints to their web host in `in_repo`.

### Where to look

1. `src/adapter/calls.rs` `in_repo` and `is_ssh`: the path check runs before any ssh call, and
   only SSH forms resolve (scp-like, `ssh://`, `git+ssh://`, `ssh+git://`).
2. `src/workspace.rs` `ssh_hostname`: `ssh -G -- host`, `None` on any failure, which keeps
   today's "not a merge request of this flow's repository".
3. Skim: `hook.rs` (one argument), the `tests/cli.rs` helper split (`hook_command`, `feed`) and
   the E2E test with a fake `ssh` on PATH.

### Checks

`cargo test` and `cargo clippy --all-targets -- -D warnings` passed at dd45b4d; at 8dd5ca0 (tests
only) the `calls` tests, clippy and `cargo fmt --check` pass.
