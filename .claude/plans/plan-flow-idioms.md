# flow: Rust idioms audit

Findings page: https://claude.ai/artifact/Mka1aqyzapaqnYnnXGUdQ5

## Decision

Audit the whole crate against the unofficial Rust idioms, the Rust API Guidelines and clippy
pedantic, and fix every real finding. Behaviour stays the same except for the bugs below and:

- A bad `adapter` name in `~/.config/flow/config.toml` fails `Config::load`, naming the file and
  the value (today the agent silently runs without hooks).
- `Cargo.toml` gets a `[lints.clippy]` table: `pedantic` on, the noisy lints allowed by name, so
  `cargo clippy -- -D warnings` keeps the new bar.

Kept on purpose: `named(&str)` / `name()` pairs instead of `FromStr` / `Display`, argv as pure
`Vec<String>`, `bool` parameters that mirror one CLI flag, TOON/JSON field names.

Pedantic lints allowed: `doc_markdown`, `unreadable_literal`, `cast_possible_truncation` (bounded
values), `struct_field_names`, `struct_excessive_bools`, `too_many_lines` (after the splits below),
`infinite_iter`, `unused_self`, `similar_names`, `match_same_arms`, `module_name_repetitions` and
any other that turns out to be noise, each named.

## Tasks

### 0. Bugs, each proven by a test that fails first
- [x] sidebar: the fit loop keeps a running total, so "N more" shows and no block is cut (sidebar.rs:169); the sidebar hides an empty `0/0` task count like the board
- [x] main: `bin_path` shortens HOME with `Path::strip_prefix` (main.rs:1083)
- [x] ui: `draw_dialog` and the new-flow error row cannot overflow or underflow on extreme sizes (app.rs:1376, newflow.rs:204)
- [x] ui: `shown_request` trims by display width (newflow.rs:137); one shared width helper on `unicode-width`
- [x] ui: clicks use `Rect::contains` (app.rs:904, input.rs:251); idiom only, a click past the last row already found no pane
- [x] theme_file: a color needs six ASCII hex digits (theme_file.rs:163)
- [x] workspace: the worktree removal uses `split_first` like `run` (an empty argv cannot happen, idiom only)
- [x] client: `setsid` failure is returned and the SAFETY comment names async-signal-safety (no reaper: `flow ui` quits on ServerGone, so init reaps the server)
- [x] host: the PTY reader retries on `Interrupted` (host.rs:256)
- [x] claude_code: `task_counts` parses "all N done" with `split_once` (claude_code.rs:289)
- [x] registry: store roots are written and read as raw OS bytes (registry.rs:46)

### 1. Conversions and common traits
- [x] `impl From<anyhow::Error> for Failure`, replacing `Failure::from_io`
- [x] `Display` for `Tasks`, `ReviewCount`, `PaneKey` (replacing `describe`)
- [x] `From<view::Workspace> for EventKind` (the fold keeps its destructuring: the other direction would be a TryFrom used once)
- [x] `Default` for `Config` and `MasterConfig` via `#[serde(default)]`; `Mode` default via `#[default]` + `default_value_t`
- [x] `Theme`: `const fn hex` with `to_be_bytes`, palette consts, `impl Default`; `Theme::state` -> `state_style`, `tone_style`
- [x] `Key`, `MouseEvent` `Copy`; `Modes` by value
- [x] `Debug` on every type, `Eq` where every field is `Eq`, `ServerPaths: Clone`
- [x] lossless casts via `From`; `pid` as `libc::pid_t` via `try_from`

### 2. Typed values
- [x] `State::name`, `Mode::name`, `Mode::toggled`, `StepStatus::name`; drop `name_of`, `name_of_mode`
- [x] outcome enums `Setup`, `BranchFate`, `Retired` (the pane outcome stays `client::Closing`) with `Display` in output.rs; `output::started` takes `&Workspace`
- [x] enums for `retire_worktree` / `drop_branch` flags (`Uncommitted`, `Unmerged`, done with 3.2); `WorkerNews` instead of `Option<Option<String>>`; `ui --stop` dispatched in `run`
- [x] server: `ControlFlow<()>` for `handle` / `message`, `stop` without `Result`, master `Start { Fresh, Resume }`
- [x] proto: typed `Flags` with `BitOr`; terminal `Arc<Mutex<Size>>`
- [x] ui: `Command::Cursor(Step)`; one `Group` enum for fold, sidebar and rail; `digit: Option<usize>` on board cards; peek `len: Option<u64>`
- [x] theme: `PaletteKey` with `named` / `name` (the crate's convention, not `FromStr`), `Palette::color` / `color_mut`
- [x] config: `Agent.adapter: Option<Adapter>`, bad names fail `Config::load` (test)

### 3. Borrowing and allocation
- [x] fold: `changes` chains instead of copying the log, `shown` compares references, `views(&[FlowLog])`
- [x] output: rows borrow (`&'a str`), blocker via `as_deref`; launch `strip_prefix`
- [x] event: `cut_to` truncates, `cut_texts(&mut self)`; main: drop needless clones, `branch_of` borrows
- [x] ui app: `mem::replace` for the view, `worker(id)` and `in_view(id)` (`workers()` keeps its Vec: it is indexed, and a handful of refs costs nothing), borrowed dialog data, `Removal::of(String)`, no clone-to-measure
- [x] ui views: `matches!` for pane keys, `unicode-width` helper, board/peek/theme_file clones
- [x] server: `Host::status` / `Host::keys`, no double key clone, early return; workspace `git` helper with `Command::arg(path)`; registry `sort_by_cached_key`

### 4. Std combinators
- [x] let-else and let chains (main.rs, store.rs, master.rs), `while let` read loops (ui/mod.rs, server/mod.rs)
- [x] fold: `find_map`, `Phase::index`, `Phase::ALL.len()`; launch `Option::filter`
- [x] board `checked_add_signed`; `zip(area.rows())` loops in board, sidebar, rail, screen
- [x] `write!` instead of `push_str(&format!)`; `on_ran` one match arm; `Path::ancestors`; theme_file chain with `?`
- [x] remaining small rewrites: input `find`, sidebar/newflow/peek tidy-ups, redundant closures (`map().unwrap_or_else()` stays: `map_or_else` puts the default first and reads worse)

### 5. unsafe and RAII
- [x] slots: `File::lock` instead of `libc::flock`
- [x] `flow start` rolls back through a drop guard
- [x] local UTC offset read moves out of board.rs to the I/O side, with `&raw`

### 6. Errors, dead code, docs
- [x] error context in store, config, branching; `Store::locate` / `Registry::locate` return `Option`, Store reuses `Registry::default_root`
- [x] remove `pane_env`'s `id` and `Conn.role`; fix stale and misplaced docs

### 7. Duplication and long functions
- [ ] main: split `start` (done with 6.2); `locate_store`, `load_config`, `config_dir` (done with 7.1); `unix_ms`
- [ ] one `write_atomic` for store, registry, slots
- [ ] ui: split `App::draw`, newflow `render`, sidebar `render`; board `digit`, `card_rect`, one footer, `spinner`, shared `Row`, `plural`

### 8. Tests
- [ ] theme_file watch test without mtime dependence; cli socket wait by connect; one deadline
- [ ] cli helpers `flow_command`, `git`, `wait_until`; `Server` / waiter drop guards; imports and naming

### 9. Lint gate and end to end
- [ ] `[lints.clippy]` pedantic table in Cargo.toml; clippy, fmt, tests green
- [ ] end to end: install, run `flow start`, `flow status`, `flow ui` with flows, `flow remove`, `flow clear` like a user
