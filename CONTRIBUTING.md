# Contributing

Bug reports, ideas and pull requests are welcome. Open an issue before a large change, so the
approach is agreed before the code.

- [Build and test](#build-and-test)
- [Checking the UI by hand](#checking-the-ui-by-hand)
- [CI](#ci)
- [Project rules](#project-rules)
- [Pull requests](#pull-requests)
- [Releasing](#releasing)

## Build and test

Requires rustup, git, macOS or Linux. `rust-toolchain.toml` pins the Rust version; rustup
installs it on the first `cargo` command.

```sh
cargo build                                  # debug build
cargo run -- status                          # run a command
cargo test                                   # unit and integration tests
cargo fmt                                    # format
cargo clippy --all-targets -- -D warnings    # lint, warnings fail
cargo deny check                             # advisories, licences, sources
cargo install --path .                       # install the saaio binary
```

| Test | Where |
| --- | --- |
| unit tests | inline `mod tests`, or a sibling `tests.rs` past about 400 lines |
| command line, end to end | `tests/cli/`, one module per subject (`main.rs` lists them) |
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

## CI

`.github/workflows/ci.yml` runs on every pull request and every push to `main`. Each check is a
job of its own, named so branch protection can require it:

| Check | Runs |
| --- | --- |
| `fmt` | `cargo fmt --check` on Linux |
| `clippy (linux)`, `clippy (macos)` | `cargo clippy --all-targets --locked -- -D warnings` |
| `test (linux)`, `test (macos)` | `cargo test --locked`, then fails if a test changed or left a file in the tree |
| `deny` | `cargo deny --locked check` with `deny.toml`: advisories, licences, bans, sources |

`deny` also runs alone every day at 06:00 UTC, since an advisory can appear without a commit. A
failure there means a dependency needs an update or, when none exists yet, an `ignore` entry in
`deny.toml` with its reason. A new dependency under a licence `deny.toml` does not allow fails
`deny` until the licence is added there.

`.github/workflows/stable.yml` runs clippy and the tests on the latest stable Rust every Monday
and on demand (Actions, stable, Run workflow), since `cargo install --git` builds with the user's
Rust. It is not a required check: a failure there is a new lint or a break to fix before users
meet it.

CI uses the toolchain `rust-toolchain.toml` pins, as you do locally. To move to a newer Rust,
change `channel` there and `rust-version` in `Cargo.toml` in one pull request, and fix what the
new clippy finds in the same one. The build cache is saved from `main` only, so a pull request
reads it but never fills it.

Tests run on slow, shared runners. A test waits for what happens soon with a limit that only a
failing test reaches (30 s), never with a sleep that guesses how long something takes; a bounded
sleep only asserts that nothing happens, and a timer the server acts on is chosen per test
(`server::Timing`). To check a test under load:

```sh
# macOS: the suite beside six busy cores
for i in 1 2 3 4 5 6; do (yes > /dev/null &); done
for i in $(seq 10); do cargo test -q 2>&1 | grep -A8 '^---- '; done
pkill -x yes

# Linux: two CPUs, 32 test threads, as a user that is not root
git archive --format=tar.gz -o /tmp/saaio.tgz HEAD
docker run --rm --cpus=2 -v /tmp/saaio.tgz:/src.tgz:ro rust:1.95 bash -c \
  'useradd -m runner && su runner -c "mkdir ~/w && cd ~/w && tar xzf /src.tgz &&
   for i in \$(seq 60); do cargo test -q --lib -- --test-threads=32 2>&1 | grep -A8 \"^---- \"; done"'
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
- CI passes before review. Run `cargo test`, `cargo fmt --check` and
  `cargo clippy --all-targets -- -D warnings` locally first; they are quicker than a round trip.
- `.github/pull_request_template.md` lists what CI cannot check.

## Releasing

A `v*` tag runs `.github/workflows/release.yml`. It builds saaio natively for macOS and Linux,
runs the tests on each build, attests the tarballs' build provenance and publishes a GitHub
release with the tarballs, `SHA256SUMS` and `install.sh`. The release profile builds with thin
LTO, one codegen unit and no debug info, so a release build takes about twice as long as a
plain one. It keeps the symbols: a panic's backtrace in server.log names its functions. `saaio --version` names the commit `build.rs` takes from git, only from saaio's own
checkout; `SAAIO_COMMIT` set while building overrides it.

1. Set `version` in `Cargo.toml`. In `CHANGELOG.md`, rename `## [Unreleased]` to the version and
   its date, such as `## [0.2.0] - 2026-11-03`.
2. Merge that to `main`.
3. Tag `main` and push the tag:

```sh
git tag v0.2.0
git push origin v0.2.0
```

Nothing is published when the tag does not match `Cargo.toml`, `CHANGELOG.md` has no dated entry
for the version, a test fails, a Linux build is not static, `saaio --version` names another commit
than the tag's, or `install.sh` fails to install from the built assets. The release notes are the
version's `CHANGELOG.md` section.

A tag with a `-`, such as `v0.2.0-rc.1`, publishes a pre-release, which the install script skips:
a trial run. Its `CHANGELOG.md` entry may still read `not released yet`. Delete it after the check:

```sh
gh release delete v0.2.0-rc.1 --cleanup-tag --yes
git tag -d v0.2.0-rc.1
```
