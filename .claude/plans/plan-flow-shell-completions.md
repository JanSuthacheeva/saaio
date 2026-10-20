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
3. `cli::run -> completions::run(args): Document`: `Cli::command()`,
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

## Tasks

- [ ] Add `clap_complete`, `cli/commands/completions.rs` (`Args`, `run`) and `Command::Completions` dispatched before the store, with the tests/cli.rs tests for every shell, `bash -n`, no store and an unknown shell
- [ ] Ship the scripts in `complete/` from release.yml's Pack step
- [ ] Document it: docs/cli.md, docs/install.md, README.md, CHANGELOG.md 0.1.0, ROADMAP.md Planned future, CLAUDE.md Architecture
- [ ] End to end: install the build, source the bash and zsh scripts in real shells and complete `saaio st<Tab>` and `saaio completions <Tab>`
