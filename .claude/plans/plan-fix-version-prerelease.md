# Fix: a release candidate's `saaio --version` says 0.2.0, not 0.2.0-rc.1

Report: `saaio --version` of the v0.2.0-rc.1 release binary prints `saaio 0.2.0 (95aec22)` without
the pre-release suffix, while `saaio update` names it `0.2.0-rc.1`. A release candidate must be
told apart in `--version`, the UI, the bug report and the server build the mismatched-binaries
check compares. Add a CHANGELOG Fixed line under Unreleased.

## Report

Expected: every place saaio names its own build names the release it is: `saaio 0.2.0-rc.1
(95aec22)`.
Actual: `--version` (and so the bug report, server.log's start line, the server's `Binary` in its
welcome, the UI's mismatch notice, the completions' version) says `0.2.0 (95aec22)`, while
`saaio update` reads the tag and says `running: v0.2.0-rc.1`.
Steps and environment: the v0.2.0-rc.1 release binary from GitHub, macOS arm64.

## Reproduction

The real release, into a scratch folder:

    SAAIO_VERSION=v0.2.0-rc.1 SAAIO_INSTALL_DIR=$S/bin sh install.sh
    $S/bin/saaio --version
    # saaio 0.2.0 (95aec22)
    strings $S/bin/saaio | grep -m1 -o 'v0\.2\.0-rc\.1'
    # v0.2.0-rc.1          (the tag is in the binary, only --version ignores it)

Same sha256 as the installed `~/.local/bin/saaio` (83d1df16...). A second symptom of the same
binary: with its own server running in a scratch store, rc.1's `saaio update` lists that server
(same pid, same executable) as another build and says to stop it:

    H=$(mktemp -d); export SAAIO_HOME=$H XDG_STATE_HOME=$H/state; cd $H
    $S/bin/saaio server &          # with CLAUDECODE, SAAIO_FLOW_ID, SAAIO_MASTER unset
    PATH=$S/bin:$PATH $S/bin/saaio update
    # running: v0.2.0-rc.1
    # latest: v0.1.2
    # update: none
    # servers[1]{store,pid,build,binary}:
    #   <H>,88298,0.2.0 (95aec22),<S>/bin/saaio
    # help[2]: "Run `SAAIO_HOME=<H> saaio --stop`, then start saaio again; ..."
    SAAIO_HOME=$H $S/bin/saaio --stop

Fails every time, for every pre-release.

## Root cause

A pre-release is tagged from the final version's Cargo.toml: release.yml's "Match the tag with
Cargo.toml" strips the pre-release before comparing (`version=${version%%-*}`), so v0.2.0-rc.1 was
built with `version = "0.2.0"`. `binaries::BUILD` (`src/binaries.rs:16`) is built from
`CARGO_PKG_VERSION`, so it says `0.2.0`; only `release::Build::running` (`src/release.rs:75`) reads
the tag (`SAAIO_RELEASE`) and says `0.2.0-rc.1`. `cli/commands/update.rs` `others` compares a
server's `Binary::version()` (from BUILD) with `Build::running().version()`, hence its own server
listed as another build.

Proven by building from source with `SAAIO_RELEASE=v0.2.0-rc.1 cargo build`: `--version` prints
`saaio 0.2.0 (95aec22)`. The stripping came with the release workflow; 856d606 "release: tag and
build stamp of a release binary" added `SAAIO_RELEASE` for `Build` and left `BUILD` on the package
version.

## Fix

Decided (master): Cargo.toml is the one source of the version, the pre-release included
(`0.2.0-rc.2` for the next candidate). `CARGO_PKG_VERSION` is then right everywhere - `BUILD`,
`Build::running`, `cargo install --git`, binstall and Homebrew later - with no version logic in
build.rs. `binaries::BUILD` and `release::Build` do not change.

- release.yml, job `check`, step "Match the tag with Cargo.toml": the tag without its `v` must
  equal Cargo.toml's version exactly (no `${version%%-*}`). Outputs `version` (the full version,
  `0.2.0-rc.2`) and a new `notes` output, the version without its pre-release part (`0.2.0`).
- release.yml, "Take the release notes from CHANGELOG.md": looks up `## [$NOTES]` (the version
  without its pre-release), so an rc reads the `## [0.2.0] - not released yet` entry as today; the
  "date the entry" check stays for a final release only.
- The build and install checks already compare `saaio $VERSION (...)` with the job's `version`
  output, which now carries the pre-release: no change beyond that.
- Not in this PR: bumping Cargo.toml to `0.2.0-rc.2`; the RC prep does that.

## Also affected

- `saaio update` of an rc binary naming its own server as another build: fixed by the same change.
- Data: none stored. A running rc.1 server keeps announcing `0.2.0 (95aec22)`; a fixed binary sees
  it as another build and asks to `saaio --stop`, which is right since it is another binary.
- CONTRIBUTING.md Releasing: a pre-release sets the pre-release in Cargo.toml (`0.2.0-rc.2`), its
  CHANGELOG entry stays `## [0.2.0] - not released yet`; the final release sets `0.2.0` and dates
  the entry. Cargo.lock changes with every version bump.
- docs/cli.md:47 and docs/install.md:5 show the `--version` form; name the pre-release form too.

## Assumptions

- The new output is named `notes` (the version whose CHANGELOG section is the release notes).
- The regression guard for the workflow is running its shell steps locally against a Cargo.toml
  copy set to the rc version; there is no harness that runs release.yml in tests.
- The CHANGELOG line goes under `## [Unreleased]` -> `### Fixed`, as asked.

## Tasks

- [x] release.yml: run the "Match the tag" step locally with `GITHUB_REF_NAME=v0.2.0-rc.2` against
  a scratch copy whose Cargo.toml says `0.2.0-rc.2` (fails today with `tag v0.2.0-rc.2 does not
  match Cargo.toml 0.2.0-rc.2`); then compare the tag exactly, add the `notes` output and read the
  CHANGELOG by it; run both steps again for `v0.2.0-rc.2` / `0.2.0-rc.2` (passes, notes from
  `## [0.2.0]`), `v0.2.0` / `0.2.0` (passes), `v0.2.0-rc.2` / `0.2.0` (fails)
- [ ] Add a test in `src/release.rs` that `Build::running().version()` is the version
  `binaries::BUILD` names, so the two cannot drift again
- [ ] Docs: CONTRIBUTING.md Releasing with the rc steps; docs/cli.md and docs/install.md name the
  pre-release form; CHANGELOG `### Fixed` under Unreleased: a release candidate's `saaio
  --version`, bug report and UI name its pre-release, and its `saaio update` no longer lists its
  own server as another build
- [ ] End-to-end: in a scratch copy of the repo with Cargo.toml set to `0.2.0-rc.2`,
  `SAAIO_RELEASE=v0.2.0-rc.2 cargo build --release`; `--version` prints `saaio 0.2.0-rc.2
  (<commit>)`, `saaio bug-report` names it; with that binary's server running in a scratch store,
  `saaio update` (PATH starting with its folder) shows no `servers[]`; end with `saaio --stop` on
  the scratch store
