# Contributing

Bug reports, ideas and pull requests are welcome. Open an issue before a large change, so the
approach is agreed before the code.

- [Build and test](#build-and-test)
- [Checking the UI by hand](#checking-the-ui-by-hand)
- [Project rules](#project-rules)
- [Pull requests](#pull-requests)

## Build and test

Requires Rust 1.95+, git, macOS or Linux.

```sh
cargo build                                  # debug build
cargo run -- status                          # run a command
cargo test                                   # unit and integration tests
cargo fmt                                    # format
cargo clippy --all-targets -- -D warnings    # lint, warnings fail
cargo install --path .                       # install the saaio binary
```

| Test | Where |
| --- | --- |
| unit tests | inline `mod tests`, or a sibling `tests.rs` past about 400 lines |
| command line, end to end | `tests/cli.rs` |
| UI screens | golden files in `src/ui/golden` |

A change to the UI's look that is meant rewrites the golden files:

```sh
SAAIO_UPDATE_GOLDEN=1 cargo test
```

Read the rewritten files in the diff before committing them.

## Checking the UI by hand

Use a scratch store, never the real one: a second UI on the same store takes over the first, and
`saaio --stop` ends every session in it.

```sh
SAAIO_HOME=$(mktemp -d) cargo run
SAAIO_HOME=<the same folder> cargo run -- --stop
```

Bare `saaio` opens the UI only in a person's terminal. Inside an agent, run it in a PTY with the
agent's markers unset:

```sh
SAAIO_HOME=$(mktemp -d) env -u CLAUDECODE -u CODEX_THREAD_ID -u SAAIO_FLOW_ID -u SAAIO_MASTER cargo run
```

## Project rules

The full rules and the architecture: [CLAUDE.md](CLAUDE.md). In short:

- One crate: a library with a thin binary. `main.rs` only parses, reads the environment and the
  clock, prints and maps exit codes.
- One responsibility per file, nameable without "and". Past about 400 production lines a file
  splits along a real seam.
- A command is `src/cli/commands/<name>.rs`: its clap `Args` and one `run`. The TOON it prints
  lives in `src/cli/output/`.
- Modules are nouns for what they hold, never `utils` or `helpers`. Functions do not repeat their
  module's name.
- Module files are `foo.rs` beside `foo/`, never `foo/mod.rs`.
- The event log is private. The public interface is the command line and `saaio status --json`
  ([docs/json.md](docs/json.md)).
- Code explains itself. Comments only where it cannot.
- Text users and agents read: plain, short sentences in reference style. No em dash, no emoji.

## Pull requests

- Small and focused: one change per pull request, in small commits.
- A change users can see updates the docs in the same pull request: `README.md`, `ROADMAP.md`,
  `docs/*.md`, `examples/`.
- Building or dropping a roadmap item updates `ROADMAP.md`.
- `CHANGELOG.md` gets no entry before 0.1.0 is released. After it, every pull request adds its
  line under `## [Unreleased]`, written for users.
- Before review: `cargo test`, `cargo fmt --check` and `cargo clippy --all-targets -- -D warnings`
  pass.
- `.github/pull_request_template.md` lists the same checks.
