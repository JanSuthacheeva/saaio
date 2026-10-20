# Build and release hygiene

From the structure audit (https://claude.ai/artifact/Lpm6RTzVR5vCuAU1UibgjE), findings 3, 4, 5, 9
and 14. Each was checked against main at 1d9177d and is real: Cargo.toml has no `[profile.*]`,
`build.rs` asks git from the manifest folder without checking whose repository answers, the
package has no `publish`, `description` or `repository`, CI runs only the pinned 1.95.0, and
release.yml publishes no attestation.

## Decisions

- **Release profile** on `release` itself (release.yml and `cargo install` both use it):
  `lto = "thin"`, `codegen-units = 1`, `strip = true`. `panic` stays `unwind`: the server's panic
  hook logs a panic, and a UI thread's panic must reach its hook. Size before: 5164672 bytes
  (macOS arm64, 1.95.0, `cargo build --release`).
- **Commit stamp**: `build/commit.rs` holds the pure choice, `stamp(env, toplevel, manifest, head)`:
  a non-empty `SAAIO_COMMIT` from the environment wins; else git's short HEAD only when
  `git rev-parse --show-toplevel` is the manifest folder (both canonicalized); else `unknown`.
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
- [ ] stable.yml
- [ ] Attestation in release.yml
- [ ] Docs: install.md, CONTRIBUTING.md, CLAUDE.md
