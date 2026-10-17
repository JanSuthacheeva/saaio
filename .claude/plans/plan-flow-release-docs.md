# Plan: release docs for flow 0.1.0

Implementation page: https://claude.ai/artifact/7dTRB6UGKHJtdQvHx5drDm (approaches: https://claude.ai/artifact/Sig9Dnm6Jui3Z1VFdJ3oQN). Pages in `docs/design/release-docs-*.html`.

## Decision

The README becomes a short front page; the manual it holds today moves into `docs/`, one
Markdown file per subject, rewritten as reference in plain words and checked against the code and
real output. Commented example files live in `examples/`; `flow server` writes them beside
`config.toml` on first run with the logic that already writes `master.md`, made shared.

- Tone: humble and plain. Jan built flow for himself and his workflow, then thought it might help
  others. No hype, no "best/only", no design rationale (that stays in `docs/HANDOVER.md`). Never an
  em dash.
- Examples: every setting commented out at its default, sshd_config style: `#key = value` (no
  space after `#`) is a setting, `# text` is explanation, section headers commented too
  (`#[worker]`, `#[[phases]]`). A setting without a default appears only in explanation
  (`# For example: prompt = "..."`).
- An untouched example is refreshed by a newer flow (earlier versions kept for the check); an
  edited one is never touched. Only `flow server` writes them, not every command.
- Theme indexes 0-15 parse to the terminal's named colors (4 is `Blue`), so the terminal theme can
  be written in its own file.
- The built-in `phase_colors` becomes `off` (state colors only; finished dots plain, the current dot
  in the state color).
- README picture: a VHS tape in the repo against real workers (Claude Code and at least one Codex)
  in a scratch `FLOW_HOME` on a throwaway sample repo, flow's dark theme, `phase_colors` off. PNG at
  the top, a GIF of navigation below the quick start.
- Install: `cargo install --git https://github.com/JanSuthacheeva/flow --tag v0.1.0`; skills with
  `npx skills add JanSuthacheeva/agent-skills ...`, one line if it takes several `--skill` flags,
  else one line per skill.
- CHANGELOG: Keep a Changelog, everything under `## [Unreleased]` (no date; it becomes
  `## [0.1.0] - <date>` when Jan tags). CLAUDE.md rule: every PR adds its line under Unreleased.
- `flow example config|theme` (a command printing the examples) goes under Future improvements in
  `docs/HANDOVER.md`, not built now.

## Units

### `src/default_file.rs` (new)

```rust
/// A file flow writes with its default text: when it is missing, or still holds an earlier default
/// nobody edited. An edited file is never overwritten.
#[derive(Debug, Clone, Copy)]
pub struct DefaultFile {
    pub text: &'static str,
    /// Every earlier default, oldest first.
    pub earlier: &'static [&'static str],
}

impl DefaultFile {
    /// Writes `text` to `path` when it is missing or an earlier default, creating its folder.
    pub fn ensure(&self, path: &Path) -> Result<()>;
}

/// An example's settings uncommented: every `#key` and `#[section]` line loses its `#`.
#[cfg(test)]
pub fn uncommented(text: &str) -> String;
```

Registered in `lib.rs` as a private module.

### `src/server/master.rs` (modified)

`DEFAULT_INSTRUCTIONS`, `PREVIOUS_DEFAULTS` and `ensure_instructions` become:

```rust
pub const INSTRUCTIONS: DefaultFile = DefaultFile {
    text: include_str!("master.md"),
    earlier: &[include_str!("master-v1.md"), /* ... */ include_str!("master-v7.md")],
};
```

Behaviour unchanged; its two write tests move to `default_file.rs` (one master-specific check
stays: every earlier default differs from the current one).

### `src/config.rs` (modified)

```rust
/// What flow writes as config.toml on its first run: every setting at its default, commented out.
pub const EXAMPLE: DefaultFile = DefaultFile {
    text: include_str!("../examples/config.toml"),
    earlier: &[],
};
```

Test-only `#[cfg_attr(test, derive(Serialize))]` on `Config`, `Agent`, `WorkerConfig`,
`MasterConfig`, `UiConfig`, `PhaseSpec`, `Adapter` (kebab-case); `Workflow` serializes with
`into = "Vec<PhaseSpec>"`, `Prefix` with `into = "String"` (its name). `PartialEq` on `Config`.

### `src/ui/theme_file.rs` (modified)

```rust
/// What flow writes as theme.toml on its first run: the terminal theme, commented out.
pub const EXAMPLE: DefaultFile = DefaultFile {
    text: include_str!("../../examples/theme.toml"),
    earlier: &[],
};

/// "#rrggbb", an ANSI index 0-255 (0-15 the terminal's named colors), "default", or another key.
fn color(value: &Value, palette: &Palette) -> Result<Color>; // signature unchanged
```

0 Black, 1 Red, 2 Green, 3 Yellow, 4 Blue, 5 Magenta, 6 Cyan, 7 Gray, 8 DarkGray, 9 LightRed,
10 LightGreen, 11 LightYellow, 12 LightBlue, 13 LightMagenta, 14 LightCyan, 15 White; 16-255 stay
`Indexed`.

### `src/ui/theme.rs` (modified)

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PhaseColors {
    /// State colors only.
    #[default]
    Off,
    /// The current step dot; spinner, label, rail and title keep the state color.
    Steps,
    /// Everything that takes the state color.
    Full,
}
```

### `src/server.rs` (modified)

```rust
pub struct ServerPaths {
    pub socket: PathBuf,
    pub master: PathBuf,
    pub instructions: PathBuf,
    /// Where flow writes the example config.toml; None without a config location.
    pub config: Option<PathBuf>,
    /// Where flow writes the example theme.toml, beside config.toml.
    pub theme: Option<PathBuf>,
}

/// Writes master.md and, when missing, the example config.toml and theme.toml; logs what fails.
fn write_defaults(paths: &ServerPaths, clock: fn() -> u64);
```

`run` calls `write_defaults` where it calls `ensure_instructions` today.

### `src/cli/commands/server.rs` (modified)

Fills `config` from `config::config_file(env.flow_config, env.xdg_config_home, env.home)` and
`theme` from `env.config_dir()` joined with `theme_file::THEME_FILE`.

### Files

- `examples/config.toml`: header (what it is, flow wrote it, edit freely, never overwritten once
  edited, link to `docs/configuration.md`); `#[worker]` agent, prompt (example only); `#[master]`
  wake, agent (example only); `#[ui]` prefix; the four built-in `#[[phases]]`; `#[agents.claude]`,
  `#[agents.codex]`, `#[agents.master]` with command, prompt, resume, adapter; the placeholders
  explained.
- `examples/theme.toml`: header (reload, color values, link to `docs/themes.md`); `#extends`,
  `#nerd_font`, `#phase_colors = "off"`; `#[ui]` all nine keys, `#[state]` all five, `#[phase]`
  the four built-in names, each with the terminal theme's value and a one-line explanation.
- `README.md` rewritten as on the implementation page: picture, intro, quick start, what it does,
  how it works, for agents and scripts (real `flow status` output, command table), similar tools
  (neutral, where flow falls short), documentation, license.
- `docs/README.md` (index), `install.md`, `ui.md`, `flows.md`, `configuration.md`, `themes.md`,
  `repository.md`, `cli.md`, `json.md`, `files.md`, `troubleshooting.md` with the contents listed
  on the implementation page.
- `docs/media/demo-setup.sh` (scratch `FLOW_HOME`, `FLOW_CONFIG` with the flow dark theme, a
  throwaway sample repo), `docs/media/demo.tape`, `docs/media/flow-ui.png`,
  `docs/media/flow-ui.gif`.
- `CHANGELOG.md`; `CLAUDE.md`: a `## Changelog` rule and `default_file.rs`, `examples/` in
  Architecture; `docs/HANDOVER.md`: `flow example config|theme` under Future improvements.

## Call chains

flow ui starts the server for the first time:

1. `commands::server::run -> server::run(&ServerPaths { config, theme, .. }, ...)`
2. `server::run -> write_defaults(paths, clock)`
3. `write_defaults -> master::INSTRUCTIONS.ensure(&paths.instructions): Result<()>`
4. `write_defaults -> config::EXAMPLE.ensure(path)` when `paths.config` is Some
5. `write_defaults -> theme_file::EXAMPLE.ensure(path)` when `paths.theme` is Some
6. `DefaultFile::ensure`: file equals `text` or is not in `earlier` -> nothing; missing or an
   earlier default -> `create_dir_all(parent)`, `fs::write(path, text)`
7. Error: an unreadable or unwritable file returns `Err` naming the path; `write_defaults` writes it
   to server.log and the server starts anyway.

The config is loaded before this, so a written config.toml applies from the next command (it
changes nothing anyway). ThemeWatch sees the new theme.toml within a second and reloads the same
colors.

## Test seams

- `DefaultFile::ensure`: writes when missing, replaces an earlier default, keeps an edited file,
  creates the folder.
- `uncommented`: uncomments `#key` and `#[section]` lines, leaves `# text` and `## ` lines.
- config: the uncommented example parses and equals `Config::builtin()`; serialized to a TOML
  table, `Config::builtin()` and the uncommented example are equal (every setting listed, none
  extra); the raw example parses to the defaults too.
- theme_file: the uncommented example as `theme.toml` gives every `PaletteKey` and each built-in
  phase the terminal theme's color, `nerd_font` false, `phase_colors` off; every `PaletteKey::ALL`
  name appears as a `#name =` line; `4` parses to `Blue`, `15` to `White`, `16` to `Indexed(16)`.
- theme: `PhaseColors::default()` is `Off`; tests that assumed `Steps` set it or expect the state
  color; goldens rewritten with `FLOW_UPDATE_GOLDEN=1` and checked by eye.
- server/tests.rs: a server on a temp config dir writes master.md, config.toml and theme.toml and
  keeps an existing config.toml byte for byte.
- No test writes into the real `~/.config/flow`: check the CLI tests that start a server set HOME,
  XDG_CONFIG_HOME or FLOW_CONFIG to a temp dir.
- Docs: every output block pasted from a real run in a scratch `FLOW_HOME`; every key in `ui.md`
  checked against `ui/app/keys.rs`, `board_keys.rs`, `newflow` and the `?` overlay; every config
  and theme key against the structs.

## Assumptions

- Only `flow server` writes the examples, not every command (hooks run `flow` constantly).
- With `FLOW_CONFIG` set, the example goes to that path, theme.toml beside it.
- The demo repo is a throwaway app in the scratchpad; requests like "add CSV export". 1600x900.
- `docs/design/` and `HANDOVER.md` stay as they are; the public docs do not link them.
- The README's reference tables move to docs unchanged in substance, in plainer words.
- `help[]` lines and error texts are documented as they read today; no CLI text changes.
- `vhs` (with `ttyd`) is installed with brew for the recording.

## Build notes

- `config::EXAMPLE` and `theme_file::EXAMPLE` carried `#[allow(dead_code)]` from their own tasks
  until `write_defaults` used them, so every commit stayed clippy-clean.
- `panel` colors the title row above the pane, not a bottom bar: the example, `themes.md` and the
  `UiTokens::bar` comment say so.
- `Cargo.toml` sets `rust-version = "1.95"`: 1.94 fails on if-let guards, 1.95 builds.
- The README picture: the demo workers run small stand-in skills kept in the sample repo
  (`demo-plan`, `demo-build`, `demo-review`, `demo-ship`) mapped to the four built-in phase names,
  so they walk real phases without publishing pages or opening merge requests. The sample repo was
  `~/projects/shop` for the recording, so the picture's paths match the docs, and deleted after.
- The recording showed two Codex limits, added to troubleshooting and the CHANGELOG's known issues:
  a worker that reads a later skill's `SKILL.md` early jumps to its phase, and its sandbox cannot
  commit in a worktree without asking.
- The sidebar has no `plain` tag (the old README said so): a plain session shows its state.
- The end-to-end check installed the branch build into a scratch `--root`, not `~/.cargo/bin`.
- Re-recorded after the review in Alacritty instead of VHS, as Jan asked: `docs/media/record.sh`
  (flow ui in a private tmux, an Alacritty window from `docs/media/alacritty.toml`, the picture by
  window id with `screencapture`, the GIF with ffmpeg cropped to the window) replaces `demo.tape`.
  The demo config sets `[master] wake = false`, so the master's pane ends on its answer.
- Found while recording, not fixed here: flow ui panics (index outside the buffer, from
  `sidebar.rs` `Row::render`) when the terminal shrinks and a redraw comes before the resize event,
  since `App::draw` lays out from the app's stored size, not the frame's. Reproduced with an app
  laid out for 150x45 drawn into a 130x40 `TestBackend`.

## Tasks

### Code

- [x] Add `DefaultFile::ensure` and `uncommented` in `src/default_file.rs` with unit tests (missing, earlier default, edited, folder; uncommenting rules); move master.md onto it as `master::INSTRUCTIONS`
- [x] Make theme indexes 0-15 the terminal's named colors in `theme_file::color`, with tests (4 Blue, 15 White, 16 Indexed)
- [x] Change the built-in `PhaseColors` default to `Off`; update the tests that assumed `Steps`, rewrite and eye-check the goldens, update the README's theme lines until docs replace them
- [x] Write `examples/theme.toml` and `theme_file::EXAMPLE` with tests: uncommented gives the terminal theme, every key listed, raw parses
- [x] Write `examples/config.toml` and `config::EXAMPLE` with test-only Serialize; tests: uncommented equals `Config::builtin()`, same TOML table, raw parses
- [x] Add `ServerPaths::config`/`theme` and `write_defaults`, filled in `commands/server.rs`; server test writes all three and keeps an existing config.toml; check no test touches the real config dir

### Docs

- [x] `docs/cli.md` and `docs/json.md`, every command, flag, event and field checked against the code and real output in a scratch `FLOW_HOME`
- [x] `docs/configuration.md` and `docs/themes.md`, every key with its default, pointing to the examples
- [x] `docs/repository.md` and `docs/files.md`: branches, setup/teardown, slots and ports, the setup-flow skill, every file flow keeps
- [x] `docs/ui.md` and `docs/flows.md`: layout, every key, dialogs, the master, guided vs auto, plain sessions, reporting, the workflow
- [x] `docs/install.md`, `docs/troubleshooting.md` and the `docs/README.md` index; check whether `npx skills add` takes several `--skill` flags
- [x] Record the picture: `docs/media/demo-setup.sh` and `demo.tape` (now `record.sh`, Alacritty) with Claude Code and Codex workers, flow dark theme, phase_colors off; `flow-ui.png` and `flow-ui.gif`, from a scratch `FLOW_HOME` only
- [x] Rewrite `README.md` as on the implementation page, with real `flow status` output and the picture
- [x] `CHANGELOG.md` under Unreleased with known issues; the CLAUDE.md changelog rule and Architecture lines; `flow example config|theme` under HANDOVER's Future improvements

### Check

- [x] End to end: install the branch build, start `flow ui` with a fresh scratch `FLOW_HOME` and empty config dir, confirm the three files appear and change nothing, edit one setting and restart (kept), follow the README quick start as a new user would, and read every docs page once against what flow does

### After review

- [x] Fix the flow ui panic when the terminal shrinks before its resize event: `App::draw` lays out for the frame it draws into, not the stored size; regression test drawing an app laid out for a larger terminal into a smaller one; a CHANGELOG line; a short second review of the commit

## Review

Reviewed in an isolated context against this plan, CLAUDE.md and the code. The code matches the
plan's units, call chains and test seams; a scratch `flow server` (scratch `FLOW_HOME`,
`XDG_STATE_HOME`, `XDG_CONFIG_HOME`, `HOME`) wrote master.md, config.toml (byte for byte the
example) and theme.toml, and `flow status` still read the commented config as the defaults. The
docs were fact-checked line by line against the code and real runs. No production code changed in
the review; every fix is documentation.

**Fixed**

- `docs/cli.md`, `docs/json.md`: report retries of `review`, `merge-request` and `ended` do add
  timeline entries; `age` moves on phase and blocker changes too; `flow wait` prints to stderr and
  `flow hook` exits 2 for Codex news; the detail's optional fields (`delegate`, `folder`, `answer`,
  `worktree_exists`) named; `--all` skips stores without flows and suffixes taken names (49c50ff).
- `docs/configuration.md`, `examples/config.toml` header: a change to `[worker] prompt` or an
  agent's `prompt`, and the new-flow dialog's default agent, need a new flow ui (the server fills
  the prompt from the config it loaded at start, `server/panes.rs`); the CLI's config error shows
  the full path; theme error texts as flow prints them; the config folder's precedence; the
  `/tmp/flow-<uid>/<hash>.sock` fallback in `docs/files.md` (c029a72).
- `docs/flows.md`, `docs/ui.md`, `docs/troubleshooting.md`, `CHANGELOG.md`: an exited worker is
  not resumed by a running server, only by `C-space r` or the next server start; the rail's title
  row names one flow; the master's title shows its folder with the rail; a turn ending with
  background tasks reports `delegated`; server.log's start line carries the commit (66b315b).

**Open**

- The README picture shows rough edges of a real run: "Stop hook feedback" at the top, the master
  remarking that commits lack a `Refs:` line because the branch name does not match the ticket
  pattern, and "the status still showed it as idle right after". Recommendation: re-record before
  tagging if Jan wants a clean first impression; the content is honest, so it is not a blocker.
- `[worker] prompt` needing a server restart is documented now, but it is surprising next to
  `[worker] agent` applying at once. Recommendation: leave as documented for 0.1.0; consider
  filling the prompt in `flow start` (recorded in the `started`/workspace event) later.
- Default files come back whenever they are missing at server start, not only on the first run
  (a deleted theme.toml reappears). Harmless, since they are all commented out; the docs' "first
  run" wording is close enough. Recommendation: keep.
- `flow status --all --json` example `root` reads `.../state/flow/projects/shop`; not verified
  against how per-project stores are named. Worth a glance.
- Fixes were reviewed by me in a separate pass, not by an independent agent; they are docs only.

**Decided** (Jan, after the review)

- The README picture: re-recorded in Alacritty (`1a5c6d5`).
- `[worker] prompt` needing a new flow ui: stays as documented for 0.1.0; filling the prompt at
  `flow start` is under HANDOVER's Future improvements.
- Default files coming back when deleted: kept.
- The `--all --json` example's `root`: right as written (per-project stores are
  `~/.local/state/flow/projects/<name>`).

**Where to look**

1. `src/default_file.rs` `ensure` with `src/server.rs` `write_defaults`: what is written, when, and
   that a read error is logged rather than overwriting a file.
2. `src/ui/theme_file.rs` `ansi` and `src/ui/theme.rs` `PhaseColors::default`: the two
   user-visible behaviour changes (indexes 0-15 as named colors, phase colors off by default).
3. `docs/configuration.md` "When a change applies" and `docs/flows.md` states and adapters: the
   places where docs and behaviour most easily drift.
4. `README.md` and `docs/media/flow-ui.png`: the first impression.

The rest (`examples/*.toml` against their tests, test-only `Serialize` derives, the docs index,
the demo script and tape) is routine and can be skimmed.

**Second review** (3881dbf, the resize panic, in an isolated context)

- Fixed: nothing; the fix is right and complete. Both draw paths lay out from the frame:
  `App::draw` through `areas_in(frame_size(frame))` and `draw_board` through
  `layout::screen(frame_size(frame))`; the title, rule, pane, sidebar or rail, every dialog
  (`Dialog` clamps to its area), the history hint and the cursor take their rects from those
  areas, and `ScreenView` clips server rows longer than the pane. The regression test fails with
  ratatui's "index outside of buffer" panic when either call goes back to `self.size`, so it
  covers both paths.
- Open: input still uses the stored size until the resize arrives (`ui.rs` routing mouse to the
  pane, `keys.rs` `on_click`, history paging, `folded()` choosing sidebar or rail). Every use is
  guarded by `Rect::contains` or only reads a height, so the worst case is one click mapped to the
  old layout for a moment; recommendation: keep, since the stored size is what the server sized
  the pane to. The CHANGELOG's `### Fixed` in the first release fixes nothing a user had;
  recommendation: keep only if Jan wants it there, else drop it before tagging. The race itself
  was not reproduced in a real `flow ui` (it needs a draw between SIGWINCH and its event); the
  unit test reproduces the exact panic instead.
- Where to look: `src/ui/app/draw.rs` `draw` and `draw_board`, then the test in
  `src/ui/app/tests/panes.rs`.

**Checks**

`cargo test` passed after the review's last code-adjacent change (the example config's comment);
clippy and fmt last passed at 1de3dc4, and the review changed no Rust code.
