# Release binaries on a tag

Implementation page: https://claude.ai/artifact/CTosRq4BjYs5ZtFEam4jZe
(approaches: https://claude.ai/artifact/7ZUKHRyyNawsEMBoUXFZRg)

## Decision

A hand-written `.github/workflows/release.yml` builds saaio natively for four targets on a `v*`
tag, tests and checks each, and publishes a GitHub release with tarballs, `SHA256SUMS` and our own
POSIX `install.sh` (approach B, not cargo-dist). No Homebrew tap now (roadmap, 0.1.1). Windows
comes later: matrix rows name their archive format and assets are named by target triple, so a
`windows-latest` row with `.zip` and an `install.ps1` fit in without renames. No `build.rs` change:
native builds see the checkout, and the workflow fails when `saaio --version` names another commit.

Static musl is proven: an `aarch64-unknown-linux-musl` build in `rust:1-alpine` links statically,
runs, and passes the suite except two environment-caused failures (Alpine has no `/usr/bin/true`;
the flaky watch test below).

## Units

### `.github/workflows/release.yml` (new)

On `push: tags: ['v*']`. `permissions: contents: read` at the top, `contents: write` only on
`publish`. Third-party actions pinned to commit SHAs (with the version in a comment).

```yaml
jobs:
  check:          # ubuntu-24.04: VERSION = tag without "v" and without "-*";
                  #   must equal Cargo.toml's version, else fail
                  #   "tag v0.2.0 does not match Cargo.toml 0.1.0"; outputs version, prerelease
                  #   notes.md = CHANGELOG.md section "## [$VERSION]" up to the next "## ["
                  #     (none: fail "CHANGELOG.md has no entry for $VERSION"; a release, not a
                  #     pre-release, whose heading still says "not released yet" fails too);
                  #   upload-artifact notes
  build:          # needs check; matrix below; per row:
                  #   checkout, the toolchain rust-toolchain.toml pins + target, (linux) apt musl-tools, CC_<target>=musl-gcc
                  #   cargo test --release --locked --target $T
                  #   cargo build --release --locked --target $T
                  #   target/$T/release/saaio --version must equal
                  #     "saaio $VERSION ($(git rev-parse --short HEAD))"
                  #   (linux) `file` must say static
                  #   pack saaio-$T.tar.gz: saaio, LICENSE-MIT, LICENSE-APACHE, README.md (top level)
                  #   upload-artifact saaio-$T
  package:        # needs build: download all, sha256sum saaio-*.tar.gz > SHA256SUMS,
                  #   add install.sh, upload-artifact release
  install-check:  # needs package; matrix os [ubuntu-24.04, macos-15]:
                  #   SAAIO_DOWNLOAD_URL=file://$dir SAAIO_INSTALL_DIR=$RUNNER_TEMP/bin sh install.sh
                  #   $RUNNER_TEMP/bin/saaio --version names $VERSION
  publish:        # needs check, install-check; contents: write
                  #   gh release create $TAG --verify-tag --title "saaio ${TAG#v}"
                  #     --notes-file notes.md [--prerelease when the tag has "-"] release/*
```

Changed during the build: the notes are taken in `check`, so a missing CHANGELOG entry fails
before the builds; signal-hook builds C, so cc-rs is pointed at musl-gcc for the target; the title names the tag, so a pre-release reads "saaio 0.1.0-rc.1".

```yaml
matrix.include:
  - { target: aarch64-apple-darwin,       runner: macos-15,         archive: tar.gz }
  - { target: x86_64-apple-darwin,        runner: macos-15-intel,   archive: tar.gz }
  - { target: x86_64-unknown-linux-musl,  runner: ubuntu-24.04,     archive: tar.gz }
  - { target: aarch64-unknown-linux-musl, runner: ubuntu-24.04-arm, archive: tar.gz }
```

### `install.sh` (new, repo root, also a release asset)

POSIX sh, `set -eu`, needs curl, tar and `sha256sum` or `shasum`.

```sh
# Inputs (environment):
#   SAAIO_VERSION      tag to install, default: the latest release
#   SAAIO_INSTALL_DIR  default: $HOME/.local/bin
#   SAAIO_DOWNLOAD_URL folder holding the assets, default: the release on GitHub (used by tests)

main()                  # resolves inputs, runs the steps below, prints what it installed
target()                # -> aarch64-apple-darwin | x86_64-apple-darwin
                        #    | x86_64-unknown-linux-musl | aarch64-unknown-linux-musl
                        #    an arm64 Mac under Rosetta gets arm64 (sysctl.proc_translated = 1)
                        #    else exit 1
base_url()              # -> $SAAIO_DOWNLOAD_URL, or
                        #    https://github.com/JanSuthacheeva/saaio/releases/latest/download
                        #    or .../releases/download/$SAAIO_VERSION
fetch(url, file)        # curl -fsSL (--proto =https --tlsv1.2 for the default base); exit 1 naming the url
sha256(file)            # -> hex digest via sha256sum or shasum -a 256
verify(tarball, sums)   # exit 1 when the tarball's line is missing or differs
place(binary, dir)      # mkdir -p dir; copy beside the target, chmod 755, mv over it (atomic,
                        #    a running saaio server keeps its old binary)
path_hint(dir)          # prints how to add dir to PATH when it is not on PATH; edits no file
fail(message)           # "saaio: <message>" on stderr, exit 1 (added during the build)
shown(path)             # the path with $HOME as ~, for the messages (added during the build)
```

Install line (docs): `curl -fsSL https://github.com/JanSuthacheeva/saaio/releases/latest/download/install.sh | sh`

### `tests/install.rs` (new)

Runs `install.sh` against a temp folder of fake assets (a shell script named `saaio` that prints a
version, tarred as `saaio-<this machine's target>.tar.gz`, with `SHA256SUMS`) via
`SAAIO_DOWNLOAD_URL=file://...`.

```rust
fn installs_the_binary_for_this_machine()      // into SAAIO_INSTALL_DIR, executable, prints its --version
fn replaces_an_installed_binary()
fn refuses_a_tarball_whose_checksum_differs()  // exit 1, nothing installed
fn names_the_folder_when_it_is_not_on_path()
fn fails_when_the_installed_binary_does_not_run() // exit 1 naming the path (added during review)
fn installs_the_version_asked_for()            // SAAIO_VERSION without SAAIO_DOWNLOAD_URL: the url it names is .../download/<tag>
```

### `src/config/watch.rs` test `reads_the_file_again_once_it_changed` (modified)

The second write gets a really new length (`agnet` has the same length as `agent`, so on Linux,
whose mtimes move in ticks of a few ms, the change goes unseen). The error stays `config.toml:2`.

### Docs (modified)

- `README.md` Install: the curl line first; from source with `cargo install ...` (Rust 1.95+).
  Requirements lose Rust.
- `docs/install.md`: Install (curl line, what it does, `SAAIO_VERSION`, `SAAIO_INSTALL_DIR`, PATH),
  manual download with `sha256sum -c`, From source. Updating: run the installer again, then
  `saaio --stop` and `saaio`; from source `cargo install --force`. Requirements: Rust only for source.
- `docs/files.md` Removing saaio: `rm ~/.local/bin/saaio` or `cargo uninstall saaio`; fix the stray
  spaces before `# or saaio remove`.
- `docs/cli.md:68` example: `bin: ~/.local/bin/saaio`.
- `ROADMAP.md`: Prebuilt binaries row becomes Homebrew tap, `cargo binstall saaio`, install script
  at saaio.dev. The Windows row is already on main (PR #76); add none.
- `CHANGELOG.md` 0.1.0 Added: prebuilt binaries for macOS and Linux and the install script.
- `CONTRIBUTING.md`: a Releasing section (version, date the CHANGELOG, push the tag; a `-rc.N` tag
  for a trial, deleted after).
- `CLAUDE.md` Structure: one line for `install.sh` and `.github/workflows/release.yml`.

## Data

| Asset | Contents |
| --- | --- |
| `saaio-<target>.tar.gz` | `saaio`, `LICENSE-MIT`, `LICENSE-APACHE`, `README.md` at the top level |
| `SHA256SUMS` | `<hex>  saaio-<target>.tar.gz`, one line per tarball (sha256sum format) |
| `install.sh` | the script of the tagged commit |

No version in asset names, so `releases/latest/download/<name>` always works. Tag `v0.1.0-rc.1`
gives `VERSION=0.1.0` for the Cargo and CHANGELOG checks.

## Call chains

Pushing a tag (any failing step stops the run; nothing is published):

1. `git push origin v0.1.0` -> `check`: VERSION from the tag; differs from Cargo.toml -> fail.
2. `build[T]` -> `cargo test --release --locked --target T`; a failing test fails the run.
3. `build[T]` -> `cargo build ...` -> `saaio --version`; not `saaio 0.1.0 (<HEAD short>)` -> fail.
4. `build[T]` -> `tar czf saaio-T.tar.gz` -> `upload-artifact`.
5. `package` -> `sha256sum > SHA256SUMS`, copies `install.sh`.
6. `install-check[os]` -> `install.sh` with `SAAIO_DOWNLOAD_URL=file://...` -> `saaio --version`; mismatch -> fail.
7. `publish` -> `gh release create ... --verify-tag [--prerelease]` with the notes `check` took (a missing
   CHANGELOG entry already failed `check`, step 1).

A user installs:

1. `main` -> `target()`; unsupported -> "saaio: no prebuilt binary for Linux riscv64. Build it with
   cargo install (docs/install.md)." exit 1.
2. `main` -> `fetch(base_url()/saaio-T.tar.gz)`, `fetch(.../SHA256SUMS)`; failure -> "saaio: could
   not download <url>" exit 1.
3. `main` -> `verify`; mismatch -> "saaio: checksum mismatch for saaio-T.tar.gz" exit 1, nothing installed.
4. `main` -> `tar xzf` in a temp dir (removed by `trap`) -> `place(saaio, $SAAIO_INSTALL_DIR)`.
5. `main` -> runs `saaio --version`; fails -> "saaio: ~/.local/bin/saaio does not run on this machine"
   exit 1 (added during review) -> prints "installed saaio 0.1.0 (abc1234) to ~/.local/bin/saaio" -> `path_hint`.

## Test seams

- `install.sh`: `tests/install.rs` with fake assets over `file://`; the real binaries in install-check.
- Commit naming: the `--version` step on every target.
- Static musl and both arches: `cargo test` per target in the workflow.
- The whole workflow: the `v0.1.0-rc.1` run.

## Assumptions

- No strip, no new release profile: symbols stay for panic backtraces in server.log.
- The release runs the full test suite per target; PR CI belongs to the `ci` flow.
- Release notes are the CHANGELOG section of the version; a pre-release uses the same section.
- install.sh never edits shell rc files; it needs curl (no wget fallback).
- `SAAIO_DOWNLOAD_URL` is documented in the script header only.
- The runners build with the toolchain `rust-toolchain.toml` pins (merged from main with CI).
- macOS binaries are not signed or notarised; curl sets no quarantine flag.

## Decisions during review

- Install line: the release asset URL above. Default folder `~/.local/bin`, `SAAIO_INSTALL_DIR`,
  `SAAIO_VERSION`.
- Jan approved pushing the branch and a `v0.1.0-rc.1` tag during the build without asking again;
  the pre-release and tag are deleted after the check. No `v0.1.0` is created.
- The `ci` flow also edits `CONTRIBUTING.md`: expect a small conflict there.

## Tasks

- [x] Fix the flaky `config::watch` test `reads_the_file_again_once_it_changed` (a really new length)
- [x] Add `install.sh` (`main`, `target`, `base_url`, `fetch`, `sha256`, `verify`, `place`, `path_hint`) with `tests/install.rs` (installs, replaces, refuses a checksum mismatch, PATH hint, `SAAIO_VERSION` url)
- [x] Add `.github/workflows/release.yml` (check, build x4 with test and `--version` check, package, install-check x2, publish)
- [x] Docs: README Install, docs/install.md (install, manual download, from source, updating), docs/files.md, docs/cli.md, ROADMAP (binaries row), CHANGELOG 0.1.0, CONTRIBUTING Releasing, CLAUDE.md Structure
- [x] End to end: push the branch and `v0.1.0-rc.1`, all jobs green, download the pre-release, `sha256sum -c`, install from it on this Mac and check `saaio --version` names the branch head, then delete the pre-release and tag
- [x] After review (Jan: fix all three): install.sh removes its temp folder on Ctrl-C and TERM too, `place` removes its `.saaio.<pid>` file when copying fails, and `SAAIO_INSTALL_DIR` loses a trailing slash; test `takes_the_folder_with_a_trailing_slash`

## Review

Two rounds. The first covered everything up to b0f65cb; the second the fixes for its open items
(f5a2916).

Fixed
- install.sh reported a binary that does not run as installed: a tarball whose saaio exits 126
  printed "installed  to ..." and exited 0 (on a noexec folder, say). It now fails with
  "saaio: <path> does not run on this machine", exit 1; test
  `fails_when_the_installed_binary_does_not_run` (b0f65cb).
- Round one's open items, all three fixed at Jan's call (f5a2916): the temp folder is removed on
  INT, TERM and HUP, `place` removes its `.saaio.<pid>` file when copying fails, and a trailing
  slash in `SAAIO_INSTALL_DIR` no longer prints the PATH hint or `~/tools//saaio`; test
  `takes_the_folder_with_a_trailing_slash`.
  Round two found nothing new. Proved by hand: install.sh killed mid-download (a FIFO served
  as the tarball over `file://`) with INT, TERM and HUP sent to its process group exits 1 and
  leaves no temp folder under /bin/sh (bash 3.2), bash and dash. The trailing-slash loop stops
  at `/` for `/` and `///`.

Open
- The signal cleanup and the failed-copy cleanup have no test in the suite: the first needs a
  blocking download and signals, the second a `mv` that fails after `cp` succeeded, which a
  normal user cannot set up reliably (a read-only folder fails `cp` first; root ignores modes).
  Recommend leaving them proved by hand.
- Under zsh (`zsh install.sh`), a trap set inside `main` runs when `main` returns, so the signal
  cleanup does not hold. The install line pipes to `sh`, so this is not a supported way to run
  it; leave.

Where to look
1. `.github/workflows/release.yml` `check`: the tag-to-Cargo.toml match and the CHANGELOG
   extraction (a release whose heading says "not released yet" fails; a pre-release does not).
2. `publish`: `--verify-tag`, `--prerelease` only for tags with "-", `contents: write` on this job
   alone.
3. `install.sh` `main` and `place`: the EXIT trap with `trap 'exit 1' HUP INT TERM` beside it, the
   `cp && chmod && mv` group inside `if !` (where `set -e` is off, so the cleanup runs), the
   Rosetta case in `target` and the checksum line match in `verify`.
Routine, skim: the docs, ROADMAP and CHANGELOG lines, the two `tests/cli.rs` test robustness fixes,
the `config::watch` test length change.

Checks
- `cargo test`, `cargo clippy --all-targets -- -D warnings` and `cargo fmt --check` pass at
  f5a2916; `tests/install.rs` also passed on Linux under busybox sh and dash there. The release
  workflow last ran green at ac92772; install.sh changed since, only its last step and cleanup.
- b0f65cb was reviewed again by the agent that wrote it, in a separate pass, not independently.
  f5a2916 was reviewed by round two, which did not write it and made no fixes of its own.
