## What and why

<!-- What changes for users or agents, and why. Link the issue. -->

## Checks

- [ ] CI passes: `fmt`, `clippy`, `test` on Linux and macOS, `deny`
- [ ] Golden files rewritten with `SAAIO_UPDATE_GOLDEN=1 cargo test` only for a change to the look that is meant
- [ ] Docs updated for anything users can see (`README.md`, `docs/`, `examples/`, `ROADMAP.md`)
- [ ] No `CHANGELOG.md` entry before 0.1.0, one line under `## [Unreleased]` after it

Rules: [CONTRIBUTING.md](../CONTRIBUTING.md).
