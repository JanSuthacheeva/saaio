# Build and release hygiene

From the structure audit (https://claude.ai/artifact/Lpm6RTzVR5vCuAU1UibgjE), findings 3, 4, 5, 9
and 14. Each was checked against main at 1d9177d and is real: Cargo.toml has no `[profile.*]`,
`build.rs` asks git from the manifest folder without checking whose repository answers, the
package has no `publish`, `description` or `repository`, CI runs only the pinned 1.95.0, and
release.yml publishes no attestation.

## Decisions

- **Release profile** on `release` itself (release.yml and `cargo install` both use it):
  `lto = "thin"`, `codegen-units = 1`, `strip = "debuginfo"` (`true` strips the symbols too and leaves the panic hooks' backtraces as
  bare addresses). `panic` stays `unwind`: the server's panic
  hook logs a panic, and a UI thread's panic must reach its hook. Size before: 5164672 bytes
  (macOS arm64, 1.95.0, `cargo build --release`); after: 4240128 (-18%), 3683568 with
  `strip = true`. A clean release build: 34 s before, 61 s after.
- **Commit stamp**: `build/commit.rs` holds the pure choice in two functions: `is_own(toplevel,
  manifest)`, whether `git rev-parse --show-toplevel` is the manifest folder (both
  canonicalized), and `stamp(env, head)`: a non-empty `SAAIO_COMMIT` from the environment wins;
  else git's short HEAD, which build.rs asks only for its own repository; else `unknown`.
  `build.rs` includes it with `#[path]`, watches the git files only for its own repository, and
  adds `rerun-if-env-changed=SAAIO_COMMIT`. `lib.rs` includes the same file under `cfg(test)` so
  `cargo test` runs its tests.
- **Cargo.toml**: `publish = false`, `description`, `repository`.
- **Stable CI**: `.github/workflows/stable.yml`, weekly (Monday 06:00 UTC) and
  `workflow_dispatch`: clippy and test on Linux and macOS with `cargo +stable`, no cache. A
  separate file keeps ci.yml's required checks and daily deny schedule as they are.
- **Attestation**: release.yml's `package` job attests the tarballs with `actions/attest`
  (v4.2.2, pinned by SHA), with `id-token: write` and `attestations: write` on that job only.
  docs/install.md shows `gh attestation verify`. Attestations need a public repository; saaio
  goes public before the first tag.
- Docs: install.md (verify), CONTRIBUTING.md (stable job, release attestation, `SAAIO_COMMIT`),
  CLAUDE.md (`build.rs`, `build/commit.rs`, CI line). No CHANGELOG entry before 0.1.0.

## Tasks

- [x] Release profile, size measured after
- [x] Cargo.toml metadata
- [x] build/commit.rs with tests, build.rs uses it, checked E2E inside a foreign repository
- [x] stable.yml
- [x] Attestation in release.yml
- [x] Docs: install.md, CONTRIBUTING.md, CLAUDE.md

## Review

Reviewed in an isolated agent against this plan, from 1d9177d to b567af7.

**Fixed**

- `strip = true` removed the symbol table, so the backtrace the panic hooks write to server.log
  (and `saaio bug-report` attaches) was only `__mh_execute_header` frames; shown with a scratch
  crate on this profile. Now `strip = "debuginfo"`, which keeps function names (2e8820f). The
  release binary's size after this change was not measured.
- The release.yml header and the `--version` line in docs/cli.md ran past the wrap (931fc96).
- The plan named `stamp(env, toplevel, manifest, head)`; the code splits it into `is_own` and
  `stamp(env, head)`. Same behaviour, plan updated to match.

The fixes were not reviewed by a separate agent; re-read once more by the reviewer.

**Open**

- None blocking. `actions/attest` v4.2.2 (SHA checked against the tag) makes SLSA provenance
  when given no predicate; it needs a public repository, so the first tag must come after saaio
  goes public, as the plan says.

**Where to look**

1. `build.rs` `main()` with `build/commit.rs` `is_own()`: when the commit comes from git. A
   cargo git checkout (`cargo install --git`) has its own `.git`, checked, so it is stamped.
2. `Cargo.toml` `[profile.release]`: thin LTO and one codegen unit roughly double release build
   time; symbols kept for backtraces.
3. `.github/workflows/release.yml` `package` job: the new `id-token`/`attestations` write
   permissions are on that job only.

Routine, skim: stable.yml, the docs, Cargo.toml metadata, the workspace test bound (20 s under
the scripts' 30 s sleep).

**Checks**: fmt, clippy and tests green at b567af7. Since then only docs, the plan and the
release profile changed (`cargo test` builds the dev profile, untouched).
