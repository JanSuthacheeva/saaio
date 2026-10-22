# Small fixes for 0.1.1

No review page: the request asked to go with the recommendation at every decision.

## Decision

Four fixes, each at its cause, each reproduced end to end before the fix:

| Issue | Fix |
| --- | --- |
| `saaio bug-report` leaves `file://` URLs, Claude Code's dash-encoded home (`-Users-jan-...`) and a symlinked home's real path verbatim | redact the home, its canonical path and the dash-encoded form of both; a `/` or `:` before a match is a boundary |
| `saaio report merge-request` stores any text; `gh pr view <url>` reads `--web` as a flag | the URL must be a merge request of the flow's repository (`calls::in_repo`, as the hook already checks); gh and glab get their flags first, then `--`, then the URL or number |
| a `{request}` inside a request is filled again, so the worker's argv grows past the system limit and it never opens | every placeholder filled in one pass |
| a request, folder or flow name starting with `-` fails with a usage error in the UI's jobs | options first, then `--`, then the positional argument |

## Units

`src/text.rs` (modified)

```rust
/// `template` with each `{key}` of `values` replaced by its value in one pass: a value is never
/// read for placeholders again; an unknown `{...}` stays as written.
pub fn fill(template: &str, values: &[(&str, &str)]) -> String;
```

`src/launch/pane.rs` (modified): the private `fill` calls `text::fill` with prompt, request,
name, session, id. `agent_argv` fills the prompt template first, then each command part once.

`src/branching.rs` (modified): `branch_name` fills type, ticket, slug, name through `text::fill`.

`src/bug_report.rs` (modified)

```rust
/// The whole report, each of `homes` replaced by `~` everywhere in it.
pub fn text(facts: &Facts, homes: &[PathBuf]) -> String;
/// `text` with every home path, and its dash-encoded form, written `~`.
fn tilde(text: &str, homes: &[PathBuf]) -> String;
/// How Claude Code names a folder's project: every character but ASCII letters and digits a `-`.
fn dashed(path: &str) -> String;
```

A match counts when the character before it is none or not a name character (`/` and `:` are
boundaries now) and the one after is none, the form's separator (`/`, or `-` for the dashed
form) or not a name character.

`src/cli/commands/bug_report.rs` (modified): passes `env.user_home()` and its
`fs::canonicalize` (when it differs).

`src/cli/commands.rs` (modified)

```rust
/// True when `url` is a merge request of a repository whose remotes are read in `dir`: origin,
/// or the upstream a fork's `gh pr create` opens it in.
pub fn of_own_repo(dir: &Path, url: &str) -> bool;
/// Where a flow's remotes are read: its main checkout, else its own folder; None without one.
pub fn repo_dir(flow: &FlowView) -> Option<PathBuf>;
```

`src/cli/commands/hook.rs` (modified): `of_own_repo` replaced by the shared one, on
`repo_dir` (the main checkout, never the worktree's own .git).

`src/cli/commands/report.rs` (modified): `MergeRequest { url }` refused unless
`of_own_repo(repo_dir(flow) or the current folder, url)`. `run` gets `cwd: &Path`.

`src/cli/output/flow.rs` (modified)

```rust
/// `<url> is not a merge request of <flow>'s repository`; help: report the URL gh or glab printed.
pub fn foreign_merge_request(flow: &FlowView, url: &str) -> Failure;
```

`src/forge.rs` (modified): `view_argv`:
`gh pr view --json url,state,createdAt -- <url>`,
`glab mr view --repo=<repo> --output json -- <id>`.

`src/ui/app/effect.rs` (modified): `Job::args`:
`start --plain=<folder> [--agent a]`, `start --mode m [--type t] [--agent a] -- <request>`,
`remove [--force] [--delete-unmerged] -- <name>`.

## Call chains

`saaio report merge-request <url>`: `report::run` -> `fold::resolve` -> `repo_dir(flow)` or cwd
-> `of_own_repo(dir, url)` -> `workspace::git::remote_urls` -> `calls::in_repo` per remote;
false -> `Err(flow::foreign_merge_request)` (exit 1, nothing appended).

## Tests

- `text::fill`: one pass, unknown placeholders kept, a value holding `{key}` kept as is.
- `pane`: a request holding `{request}` and `{name}` lands verbatim, once.
- `bug_report::tilde`: after `file://`, after `:`, the canonical home, the dashed form followed by
  `-projects-...`, not `-Users-janet`.
- `forge`: view argv with `--` before the URL or number; `--repo=`.
- `effect`: job args with `--` and a request, folder and name starting with `-`.
- CLI (`tests/cli/report.rs`): a foreign host and `--web` refused, the repo's own URL accepted.

## Tasks

- [x] `text::fill` in one pass; `launch::pane` and `branching` use it, with tests that a request holding placeholders is passed once and verbatim
- [ ] `bug_report::text`/`tilde` take every home form, dash-encoded and after `/` and `:`; the command passes the canonical home; tests
- [ ] `commands::of_own_repo` and `repo_dir` shared by hook and report; `saaio report merge-request` refuses a URL of another repository, with CLI tests
- [ ] `Forge::view_argv` puts flags first and `--` before the URL or number, with tests
- [ ] `Job::args` puts `--` before the request and the flow name and passes the folder as `--plain=`, with tests
- [ ] CHANGELOG Fixed lines under `## [Unreleased]`, docs/cli.md on the refused URL, CLAUDE.md E2E note on `XDG_STATE_HOME`
- [ ] End to end: rerun the four reproductions on scratch stores, stop every server, remove the scratch folders
