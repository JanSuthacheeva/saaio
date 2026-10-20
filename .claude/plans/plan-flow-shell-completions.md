# Plan: shell completions

Design page: https://claude.ai/artifact/WjFN4zqaaVBoYQPMbh6qeN
Source: structure audit finding 13 (https://claude.ai/artifact/Lpm6RTzVR5vCuAU1UibgjE), verified
against main at 1d9177d: saaio has no completions and no `clap_complete` dependency.

## Decision

Approach A, static scripts: `saaio completions <shell>` prints `clap_complete::generate`'s script
for `Cli::command()` for bash, zsh, fish, elvish or powershell. It runs without a store. The
release's Pack step runs the built binary once per shell and ships the scripts in `complete/` in
each tarball; install.sh is unchanged. Dynamic completion of flow names (clap_complete's unstable
`CompleteEnv`) goes to ROADMAP.md, Planned future.

Decided in review (the master, for the user):
- The command is `saaio completions <shell>`.
- ROADMAP.md gets a Planned future row for dynamic flow-name completion.
- Completions ship in 0.1.0: a line in the existing, undated 0.1.0 entry of CHANGELOG.md.

## Units

`src/cli/commands/completions.rs` (new):

```rust
use clap_complete::Shell;

#[derive(Debug, clap::Args)]
pub struct Args {
    /// The shell: bash, zsh, fish, elvish or powershell
    shell: Shell,
}

/// `saaio completions <shell>`: the shell's completion script for every command saaio --help
/// lists. Runs without a store.
pub fn run(args: &Args) -> Document; // Document::Text(script), trailing newline trimmed

/// Cli's command without the commands saaio --help hides, which clap_complete would offer.
fn listed() -> clap::Command;
```

`src/cli.rs` (modified):

```rust
enum Command {
    ...
    /// Print the completion script for a shell
    Completions(completions::Args),
    ...
}

// in run, beside BugReport, before the store opens:
Some(Command::Completions(args)) => return Ok(completions::run(args)),
// and in the final match's no-op arm with Hook | Wait | BugReport
```

Other files:
- `Cargo.toml`: `clap_complete = "4.6"`.
- `src/cli/commands.rs`: `pub(super) mod completions;`
- `.github/workflows/release.yml` Pack: `mkdir pack/complete`, the built binary writes
  `saaio.bash`, `_saaio`, `saaio.fish`, `saaio.elv`, `_saaio.ps1` there; tar adds `complete`.
- `docs/cli.md`: a `saaio completions` section, its row in Commands at a glance and its contents
  line. `docs/install.md`: a Shell completions section with one line per shell, and `complete/`
  under Manual download. `README.md`: a row in the command table.
- `CHANGELOG.md`: a line under 0.1.0 Added. `ROADMAP.md`: a Planned future row.
- `CLAUDE.md` Architecture: `completions.rs` under `cli/`, and Completions among the commands that
  run without a store.

## Call chain

1. `main -> Cli::try_parse(): Cli`. An unknown shell is a clap error: `Failure::usage`, exit 2,
   with the possible values.
2. `main -> cli::run(cli, env, clock)` matches `Completions` before the store opens.
3. `cli::run -> completions::run(args): Document`: `listed()`,
   `clap_complete::generate(shell, &mut command, "saaio", &mut buf)`, `Document::Text` trimmed.
4. `main -> answer` prints it, exit 0. No failure path after parsing.

## Test seams

- `tests/cli.rs`: each shell prints a script, exit 0, naming `bug-report` and not the hidden `hook`.
- `tests/cli.rs`: `bash -n` accepts the bash script.
- `tests/cli.rs`: works with `HOME`, `SAAIO_HOME` and `XDG_STATE_HOME` unset.
- `tests/cli.rs`: `saaio completions tcsh` exits 2 with TOON naming the possible values.

## Assumptions

- All five shells `clap_complete::Shell` knows.
- The scripts print raw, not as TOON.
- Tarball folder `complete/`; install.sh does not install the scripts.
- `saaio completions` is listed in `saaio --help`.

## Decisions made in the build

- `clap_complete` offers hidden subcommands in every shell, so `completions::listed` rebuilds
  `Cli::command()` with only the visible ones (unit test: the names `saaio --help` lists).
- `Failure::usage` now keeps clap's possible values of an invalid value
  (`...; one of bash | elvish | fish | powershell | zsh`), which helps `--mode` too.
- Fixed on the way: the flaky 5 s bound in `workspace::tests::a_hook_that_leaves_a_process_behind_still_ends`.

## Tasks

- [x] Add `clap_complete`, `cli/commands/completions.rs` (`Args`, `run`) and `Command::Completions` dispatched before the store, with the tests/cli.rs tests for every shell, `bash -n`, no store and an unknown shell
- [x] Ship the scripts in `complete/` from release.yml's Pack step
- [x] Document it: docs/cli.md, docs/install.md, README.md, CHANGELOG.md 0.1.0, ROADMAP.md Planned future, CLAUDE.md Architecture
- [x] End to end: install the build, source the bash and zsh scripts in real shells and complete `saaio st<Tab>` and `saaio completions <Tab>`
- [x] main.rs: end quietly when stdout's reader went away, fail on any other write error, with a tests/cli.rs test (decided after review: Open item 2)

## Review

Reviewed in isolation against this plan and CLAUDE.md. No finding in data, boundaries, behaviour
or errors: the command runs before the store, an unknown shell exits 2 with the shells named, the
hidden `wait`, `server` and `hook` are left out (clap_complete 4.6.11 does offer hidden subcommands
in every shell, checked in its source), and every release runner is native, so Pack can run the
built binary.

Fixed:
- The `Failure::usage` doc comment ran to 128 columns: 6cc8241.
- docs/install.md wrote the fish script into a folder it did not create, unlike the zsh and bash
  lines: 9cca5d7.

Both fixes are a comment and a doc line, reviewed by the reviewer who made them.

Open:
- `listed()` rebuilds the root command with only its version, arguments and visible subcommands;
  `about`, `after_help` and `args_conflicts_with_subcommands` are dropped. clap_complete reads none
  of them today, so the scripts are the same. Recommendation: leave it; the unit test catches a
  changed command list.
- Not from this branch: `main` prints with `println!`, so a reader that closes stdout early panics
  ("failed printing to stdout: Broken pipe"). The scripts (10-38 KB) fit in a pipe buffer, so
  `saaio completions bash | head` does not hit it in practice. Recommendation: a separate small
  fix in main.rs that ends quietly on a broken pipe.

Where to look:
1. `src/cli/commands/completions.rs` `listed()`: how the command clap_complete sees is built.
2. `src/cli/output.rs` `Failure::usage`: the new possible-values branch, which also changes the
   message of every other invalid enum value (`--mode`, report's event kinds).
3. `.github/workflows/release.yml` Pack: runs only on a tag, so it is untested until the first one.
Routine, skim: the docs, README, CHANGELOG, ROADMAP, CLAUDE.md lines and the workspace test bound.

Checks: `cargo test` passed at 9043cf6 (only a comment and docs changed since); `cargo fmt --check`
and `cargo clippy --all-targets -- -D warnings` passed at 9cca5d7; `cargo deny check` passed
before the docs commits, with no dependency change since.

Decided (the worker, under the master's go-ahead through to the PR): leave `listed()` as it is;
fix the broken pipe in this branch, since the user's rule is to fix such bugs when found.

### Second review (c2c73f0..c7ae846)

Covers only c7ae846, the broken-pipe fix in `main.rs` `print()`. No finding in data, boundaries,
behaviour or errors: a broken pipe ends with the command's own code and nothing on stderr, any
other write error prints `saaio: could not write to stdout: ...` and exits 1, and the failure
TOON goes through the same path. `main.rs` still only prints and maps exit codes, as CLAUDE.md
gives it. The new tests/cli.rs test fails without the fix (checked against c2c73f0's main.rs:
exit 101).

Fixed:
- `completions.rs` still said `println!` adds the script's line break: 0101ca1.

The fix is a comment, reviewed by the reviewer who made it.

Open: none. The "any other write error" branch has no test; macOS has no /dev/full to provoke
one, and the branch is three lines. Recommendation: leave it.

Where to look: `src/main.rs` `print()`. Routine, skim: the test and the plan line.

Checks: `cargo test`, `cargo clippy --all-targets -- -D warnings` and `cargo fmt --check` passed
at 0101ca1.
