# Rename flow to saaio

Review page: https://claude.ai/artifact/CpmQ4mnWsJ7S6ei7WBZVLh
Name research: https://claude.ai/artifact/Y39dGrNNdbNyBNj7tr117e

## Decision

The product, binary and crate become `saaio`. The unit of work stays "a flow": `saaio start` creates a flow; `FlowView`, `flows/<id>.jsonl`, `--flow`, `flow_id` and the `flow/<name>` branch template keep their names. No compatibility code: before 0.1.0 Jan is the only user, and a one-time move replaces it. One PR, layer by layer, each commit compiling with its tests passing.

## Renames

| What | Today | After |
|---|---|---|
| crate, lib, binary | `flow` | `saaio` (`use saaio::`, `CARGO_BIN_EXE_saaio`) |
| variables | `FLOW_HOME`, `FLOW_CONFIG`, `FLOW_MASTER`, `FLOW_SLOT`, `FLOW_REPO`, `FLOW_WORKTREE` | `SAAIO_HOME`, `SAAIO_CONFIG`, `SAAIO_MASTER`, `SAAIO_SLOT`, `SAAIO_REPO`, `SAAIO_WORKTREE` |
| which flow a worker is | `FLOW_ID`, `FLOW_NAME` | `SAAIO_FLOW_ID`, `SAAIO_FLOW_NAME` |
| build and test | `FLOW_COMMIT`, `FLOW_UPDATE_GOLDEN` | `SAAIO_COMMIT`, `SAAIO_UPDATE_GOLDEN` |
| state, config | `~/.local/state/flow`, `~/.config/flow`, `/etc/flow` | `~/.local/state/saaio`, `~/.config/saaio`, `/etc/saaio` |
| per project | `.flow/` | `.saaio/` |
| socket folder | `/tmp/flow-<uid>` | `/tmp/saaio-<uid>` (when the store path is too long for a socket) |
| hosted-pane marker | `flow-ui` | `saaio-ui` |
| themes | `flow`, `flow-light`, `Palette::FLOW`, `Palette::FLOW_LIGHT` | `saaio`, `saaio-light`, `Palette::SAAIO`, `Palette::SAAIO_LIGHT` |
| UI wordmark | `▚▄▞ flow`, `▚▄▞ flow board` | `▚▄▞ saaio`, `▚▄▞ saaio board` |
| agent text | "worker of flow", "message from the flow master:", `flow <cmd>` in help[] and errors | "worker of saaio", "message from the saaio master:", `saaio <cmd>` |
| skill name in docs | setup-flow | setup-saaio |

Stays: "a flow"/"flows" as the unit, `.claude/plans/`, `docs/design/` and `docs/HANDOVER.md` (history; HANDOVER says so in its first line), `src/server/master-v*.md` (earlier defaults, byte for byte), docs/media file names.

## Units

```rust
// src/cli/env.rs - Env fields renamed; flow_id stays
pub struct Env {
    pub saaio_home: Option<PathBuf>,    // was flow_home, from $SAAIO_HOME
    pub saaio_config: Option<PathBuf>,  // was flow_config, from $SAAIO_CONFIG
    pub flow_id: Option<String>,  // from $SAAIO_FLOW_ID
    pub saaio_master: bool,             // was flow_master, $SAAIO_MASTER is 1
    ...
}
pub(crate) fn bin_path(&self) -> String; // falls back to "saaio"

// src/launch/pane.rs
pub const HOSTED: &str = "saaio-ui";

// src/ui/theme.rs
impl Palette {
    pub const SAAIO: Palette;
    pub const SAAIO_LIGHT: Palette;
    pub fn named(name: &str) -> Option<Palette>; // "terminal", "saaio", "saaio-light"
}

// src/proto.rs
pub const BUILD: &str = concat!(env!("CARGO_PKG_VERSION"), " ", env!("SAAIO_COMMIT"));
```

`Env` already has `home` and `config()`, so the three fields take a `saaio_` prefix (decided during the build).

## Test seams

- Unit tests move with the strings they assert; goldens are rewritten with `SAAIO_UPDATE_GOLDEN=1 cargo test` and read once.
- `tests/cli.rs` runs the `saaio` binary end to end with `SAAIO_HOME` and `SAAIO_CONFIG` scratch folders.
- A final grep proves no product use of `flow` is left outside the kept places.

## Assumptions

1. `FLOW_ID`/`FLOW_NAME` become `SAAIO_FLOW_ID`/`SAAIO_FLOW_NAME`; every other variable becomes `SAAIO_<rest>`.
2. The `flow/<name>` branch template stays.
3. docs/media keeps its names; the GIF and PNG are recorded again after the merge.
4. Repo links point to `github.com/JanSuthacheeva/saaio`.
5. The `saaio-ui` marker change is safe because no hosted flow is open when the rename is installed.

## After the merge (Jan or the master)

1. `flow clear`, stop `flow ui`, `cargo uninstall flow`, `cargo install --path .`.
2. `mv ~/.local/state/flow ~/.local/state/saaio`, `mv ~/.config/flow ~/.config/saaio`; in each project `.flow/` becomes `.saaio/` and its hooks read `SAAIO_*`.
3. Rename the GitHub repo, the setup-flow skill and any `flow` permission rules in Claude Code settings.
4. Record docs/media again.

## Tasks

- [x] Rename the crate, lib and binary to saaio: Cargo.toml, Cargo.lock, `use saaio::` in main.rs, `CARGO_BIN_EXE_saaio` in tests/cli.rs, `SAAIO_COMMIT` in build.rs and proto.rs
- [x] Rename every variable to `SAAIO_*` (`SAAIO_FLOW_ID`, `SAAIO_FLOW_NAME`) and the `Env` fields, with the tests that set them
- [x] Rename the folders: state and config `saaio`, `/etc/saaio`, `.saaio/` per project, `/tmp/saaio-<uid>`, with their tests
- [x] Rename the stored and shown names: `HOSTED = "saaio-ui"`, themes `saaio`/`saaio-light` and `Palette::SAAIO`, the UI wordmark; rewrite the goldens
- [x] Rename what agents read: `WORKER_PROMPT`, "message from the saaio master:", every help[] line, error and the about text in src/cli, and master.md (its last default kept as master-v9.md)
- [x] Rename the docs: README, docs/*.md, HANDOVER, CLAUDE.md, ROADMAP, CHANGELOG, examples, media scripts
- [x] Grep for any product use of flow left; fmt, clippy, tests
- [ ] End to end: install saaio into a scratch root and run `saaio`, `saaio new`, `saaio report`, `saaio status --json` and `saaio ui` on a scratch `SAAIO_HOME`
