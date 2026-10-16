# flow: file and module structure

Implementation page: https://claude.ai/artifact/WoZBSfBKTds3La3t4UEk8b
Approaches page: https://claude.ai/artifact/9nodKi1uqUEmoXfwQzX6xS

## Decision

Approach B: `lib.rs` is the crate root, `main.rs` only the process boundary (parse, environment
variables, clock, stdout/stderr, exit code). The three front ends `cli/`, `server/`, `ui/` are peer
folders over a flat core (no `core/` or `domain/` folder). Every big file is cut along a real seam.

- The bar is textbook Rust a professional reviewer agrees with; effort and conflicts with
  `flow/codex-support` do not count.
- No behaviour change: every TOON, JSON, stderr and terminal byte stays the same. `tests/cli.rs`
  and the UI goldens are the proof and stay unchanged.
- `cargo test`, `cargo clippy -- -D warnings` (pedantic) and `cargo fmt --check` clean at every commit.
- Module files use `foo.rs` + `foo/`, never `mod.rs`, enforced by `clippy::mod_module_files`.
- `Env` and its methods live together in `cli/env.rs`; `cli.rs` re-exports it (`pub use env::Env;`).
  Re-exports are allowed where they give a type its natural path.
- `flow wait`'s answer is `Document::Wake(String)`: stderr, exit 2.
- Modules are private in `lib.rs` except `pub mod cli`; `pub` inside private modules stays
  crate-only, so dead code still warns.
- `main.rs` reads environment variables and the clock; `current_dir`, `current_exe` and stdin are
  read by the command that needs them.

## Target tree

```
src/
  main.rs                 process boundary
  lib.rs                  module tree; pub mod cli
  cli.rs                  Cli, Command, run(); pub use env::Env
  cli/
    env.rs                Env + impl
    commands.rs           module list, views(), no_flow()
    commands/             home new report send status start remove clear retire ui server wait hook
    output.rs             Document, Failure, toon(), truncated(), DESCRIPTION, LIST_TEXT_LIMIT
    output/               status flow start remove news
  server.rs               ServerPaths, Event, Server, run, serve, handle, stop
  server/                 conn.rs panes.rs tests.rs host.rs terminal.rs master.rs watch.rs reconcile.rs
  ui.rs                   threads and the loop (was ui/mod.rs)
  ui/
    app.rs                View, App state, constructors, queries
    app/                  effect.rs messages.rs keys.rs board_keys.rs draw.rs tests.rs tests/
    dialog.rs             draw_dialog, draw_help, draw_not_running, draw_no_session
    fit.rs                columns, spans_width, tail, clip, clip_place, wrap, split
    board.rs  board/      cards.rs detail.rs
    sidebar.rs sidebar/   rows.rs labels.rs
    input layout screen rail newflow peek outer theme theme_file   (unchanged)
  adapter.rs  adapter/claude_code.rs
  deliver.rs              worker_news, master_news
  text.rs                 plural, short_duration, long_duration, clock
  launch.rs  launch/      name.rs worktree.rs pane.rs
  fold.rs  fold/tests.rs
  slots.rs                + taken_slots, free_slot
  event view wake store registry branching workspace config proto client   (unchanged)
```

## Units

### main.rs

```rust
use flow::cli::{self, Cli, Env};
use flow::cli::output::{Document, Failure};

fn main() -> ExitCode;   // try_parse; DisplayHelp/DisplayVersion print, exit 0; else fail(&Failure::usage)
fn env() -> Env;         // FLOW_HOME FLOW_CONFIG FLOW_ID HOME XDG_STATE_HOME XDG_CONFIG_HOME CLAUDE_CONFIG_DIR
fn now_ms() -> u64;      // cli::unix_ms(SystemTime::now()), the crate's one clock conversion
fn answer(result: Result<Document, Failure>) -> ExitCode;
    // Toon/Json/Text -> println, 0; Silent -> 0; Wake(news) -> eprintln, 2; Err -> fail
fn fail(failure: &Failure) -> ExitCode;   // println!(to_toon), ExitCode::from(code)
```

### lib.rs

```rust
pub mod cli;
mod adapter; mod branching; mod client; mod config; mod deliver; mod event; mod fold;
mod launch; mod proto; mod registry; mod server; mod slots; mod store; mod text;
mod ui; mod view; mod wake; mod workspace;
```

### cli.rs

```rust
mod commands;
mod env;
pub mod output;

pub use env::Env;
pub use crate::event::unix_ms;   // for main's now_ms

/// Tracks parallel feature flows: what each is doing, where it is blocked, how far it got.
#[derive(Debug, Parser)]
#[command(version, about)]
pub struct Cli { #[command(subcommand)] command: Option<Command> }

#[derive(Debug, Subcommand)]
enum Command {
    New(new::Args), Report(report::Args), Status(status::Args), Start(start::Args),
    Remove(remove::Args), Send(send::Args), Clear, Ui(ui::Args),
    #[command(hide = true)] Wait(wait::Args),
    #[command(hide = true)] Server,
    #[command(hide = true)] Hook(hook::Args),
}   // each variant keeps today's doc comment (main.rs:56-146)

/// Hook and Wait run without a store; every other command fails without one.
pub fn run(cli: Cli, env: &Env, clock: fn() -> u64) -> Result<Document, Failure>;
```

### cli/env.rs

```rust
#[derive(Debug, Clone, Default)]
pub struct Env {
    pub flow_home: Option<PathBuf>, pub flow_config: Option<PathBuf>, pub flow_id: Option<String>,
    pub home: Option<PathBuf>, pub xdg_state_home: Option<PathBuf>,
    pub xdg_config_home: Option<PathBuf>, pub claude_config_dir: Option<PathBuf>,
}
impl Env {
    pub(crate) fn store(&self) -> Option<Store>;
    pub(crate) fn registry(&self) -> Option<Registry>;
    pub(crate) fn config(&self) -> anyhow::Result<Config>;
    pub(crate) fn config_dir(&self) -> Option<PathBuf>;
    pub(crate) fn claude_home(&self) -> PathBuf;     // CLAUDE_CONFIG_DIR (not empty), else HOME/.claude, else .claude
    pub(crate) fn user_home(&self) -> Option<&Path>; // HOME, not empty
    pub(crate) fn flow_id(&self) -> Option<&str>;    // set and not empty
    pub(crate) fn bin_path(&self) -> String;         // current_exe with ~ for HOME
}
```

### cli/commands.rs and cli/commands/*.rs

```rust
// commands.rs
mod clear; mod hook; mod home; mod new; mod remove; mod report; mod retire;
mod send; mod server; mod start; mod status; mod ui; mod wait;   // pub(super) as cli.rs needs
fn views(store: &Store, now: u64) -> (Vec<FlowView>, Vec<String>);
fn no_flow(name: &str) -> Failure;

// home.rs
pub fn run(store: &Store, bin: &str, now: u64) -> Result<Document, Failure>;
// new.rs
#[derive(Debug, clap::Args)]
pub struct Args { #[arg(long)] name: String, #[arg(long)] request: String,
                  #[arg(long, value_enum, default_value_t)] mode: Mode }
pub fn run(args: Args, store: &Store, now: u64) -> Result<Document, Failure>;
// report.rs
pub struct Args { #[arg(long, env = "FLOW_ID")] flow: Option<String>,
                  #[command(subcommand)] event: ReportEvent }
enum ReportEvent { .. }   // unchanged, with into_kind()
pub fn run(args: Args, store: &Store, now: u64) -> Result<Document, Failure>;
// send.rs
pub struct Args { flow: String, text: String }
pub fn run(args: &Args, store: &Store, sender: Option<&str>, now: u64) -> Result<Document, Failure>;
    // sender = env.flow_id(): Some -> "only the master sends"
// status.rs
pub struct Args { flow: Option<String>, json: bool, full: bool, all: bool }  // attrs as today
pub fn run(args: Args, store: &Store, registry: &Registry, now: u64) -> Result<Document, Failure>;
fn one_store(..); fn all_stores(registry: &Registry, json: bool, full: bool, now: u64) -> ..;
// start.rs
pub struct Args { request: String, name: Option<String>, mode: Mode, agent: Option<String>,
                  #[arg(long = "type")] kind: Option<String> }   // replaces Launch
pub fn run(args: &Args, store: &Store, registry: &Registry, config: &Config, cwd: &Path, now: u64)
    -> Result<Document, Failure>;
fn pick_kind; fn claim_slot; fn held_slots; fn claim_is_live; fn free_branch; fn append_workspace;
struct Rollback<'a> { .. }   // + impl Drop
// remove.rs
pub struct Args { flow: String, #[arg(long)] force: bool, #[arg(long)] delete_unmerged: bool }
pub fn run(args: &Args, store: &Store, now: u64) -> Result<Document, Failure>;
// clear.rs
pub fn run(store: &Store, now: u64) -> Result<Document, Failure>;
// retire.rs
pub enum Uncommitted { Keep, Discard }
pub enum Unmerged { Keep, Delete }
pub const CLOSE_WAIT: Duration;
pub fn refuse_uncommitted_work(flow: &FlowView) -> Result<(), Failure>;
pub fn refuse_unmerged_commits(flow: &FlowView) -> Result<(), Failure>;
pub fn close_pane(store: &Store, flow: &FlowView) -> Result<Closing, Failure>;
pub fn retire_worktree(flow: &FlowView, uncommitted: Uncommitted) -> Retired;
pub fn drop_branch(flow: &FlowView, unmerged: Unmerged) -> BranchFate;
fn branch_of(flow: &FlowView) -> Option<(PathBuf, &str)>;
// ui.rs
pub struct Args { #[arg(long)] stop: bool }
pub fn run(args: Args, store: &Store, env: &Env, clock: fn() -> u64) -> Result<Document, Failure>;
fn stop_server(socket: &Path) -> Result<Document, Failure>;
// server.rs
pub fn run(store: Store, env: &Env, clock: fn() -> u64) -> Result<Document, Failure>;
// wait.rs
pub struct Args { #[arg(long)] master: bool }
pub fn run(args: &Args, env: &Env, clock: fn() -> u64) -> Document;   // Wake(news) or Silent
fn wait(store: &Store, waiter: &Waiter, clock: fn() -> u64) -> Option<String>;
const WAIT_EVERY: Duration;
// hook.rs
pub struct Args { adapter: String }
pub fn run(args: &Args, env: &Env, clock: fn() -> u64) -> Document;   // always Silent
fn hook(store: &Store, config: &Config, adapter: &str, flow: Option<&str>, payload: &str, now: u64);
```

### cli/output.rs and cli/output/*.rs

```rust
// output.rs
pub const DESCRIPTION: &str;
pub const LIST_TEXT_LIMIT: usize;
pub enum Document { Toon(String), Json(String), Text(String), Wake(String), Silent }
impl Document { pub fn text(&self) -> &str; }
pub struct Failure { pub message: String, pub help: Vec<String>, pub code: u8 }
impl Failure { pub fn error(..); pub fn usage(..); pub fn to_toon(&self) -> String; }
impl From<anyhow::Error> for Failure; impl From<std::io::Error> for Failure;
pub fn truncated(text: &str, limit: usize, full: bool) -> String;
fn toon<T: Serialize>(document: &T) -> String;   // visible to the family modules
// Only Document and Failure (with text, usage, to_toon) are pub: main uses them. Everything else
// in output is pub(crate), so pedantic's public-API lints stay quiet and dead code still warns.
pub mod flow; pub mod news; pub mod remove; pub mod start; pub mod status;

// output/status.rs  home, status_list, status_all, status_detail (+ list, detail_help, row types, NEW_HELP)
// output/flow.rs    created, reported, sent, skipped
// output/start.rs   Opening, Setup (+ Display), started
// output/remove.rs  UNMERGED, UNCOMMITTED, Retired, BranchFate (+ Display, name), removed, cleared, pane_name
// output/news.rs    news_for_worker(messages: &[&str]) -> String, news_for_master(flows: &[&FlowView]) -> String
```

### deliver.rs

```rust
pub enum WorkerNews { Gone, Nothing, News(Vec<String>) }
pub fn worker_news(store: &Store, id: &str, now: u64) -> WorkerNews;
pub fn master_news(store: &Store, clock: fn() -> u64, is_mine: &dyn Fn() -> bool) -> Vec<FlowView>;
const SETTLE: Duration;
```

### text.rs

```rust
pub fn plural<N: Copy + PartialEq + From<u8>>(count: N) -> &'static str;
pub fn short_duration(ms: u64) -> String;
pub fn long_duration(ms: u64) -> String;
pub fn clock(ms: u64, offset_secs: i64) -> String;
```

### launch.rs, launch/, slots.rs

```rust
// launch.rs          pub mod name; pub mod pane; pub mod worktree;
// launch/name.rs     flow_name, ticket, slug (+ ticket_key, words, slugify)
// launch/worktree.rs WORKTREES_DIR, workspace_for, worktree_add_argv, worktree_remove_argv, repo_of, hook_env
// launch/pane.rs     HOSTED, LaunchEnv, PaneSpec, agent_argv, resume_argv, worker_spec, pane_env (+ worker_env, fill)
// slots.rs           + taken_slots(flows: &[FlowView]) -> Vec<u8>, free_slot(flows: &[FlowView]) -> Option<u8>
```

### server.rs, server/

```rust
// server.rs  ServerPaths, Event, Server, consts,
//            pub fn run(paths: &ServerPaths, store: Store, config: Config, env: LaunchEnv, cwd: &Path,
//                       clock: fn() -> u64) -> Result<()>;
//            impl Server { serve, handle, stop }
// conn.rs    impl Server { message, hello, accept, drop_conn, send_to, flush, send_full_frame,
//                          send_master, send_board, send_panes }; bind, accept_in_background, read_in_background
// panes.rs   impl Server { refresh, apply, open_master, open_worker, reopen_worker, open_fresh,
//                          close_flow, tell_closed, end_flows, flow }; forward_host_events
// tests.rs   the socket tests from server/mod.rs
```

### ui.rs, ui/

```rust
// ui.rs  pub fn run(socket: &Path, env: UiEnv, clock: fn() -> u64) -> Result<String>;   // drops ui::now_ms
// app.rs            View, App, new, with_offset, with_store, with_home, say, say_about_file, areas,
//                   is_modal, peeked; worker, workers, in_view, status, tilde, focused_repo, sidebar
// app/effect.rs     Effect, Job (+ args, dir), Removal (+ of), outcome
// app/messages.rs   impl App { on_server, on_ran, on_branching, on_tick, overview_due, on_resize,
//                              ask_overview, here_first }
// app/keys.rs       impl App { on_input, on_command, on_keys, on_key, on_stop_key, on_history_key,
//                              on_ended_key, on_click, focus, move_cursor, open_new_flow, remove }
// app/board_keys.rs impl App { on_board_key, on_all_key, on_any_board_key, clear }; board_step
// app/draw.rs       impl App { draw, draw_board, draw_rule, draw_view, draw_stop, draw_title,
//                              title_line, border_hint, draw_bar }
// app/tests.rs      shared fixtures and render helpers; mod panes, sidebar, board, dialogs, golden
//                   in app/tests/*.rs
// dialog.rs         pub fn draw_dialog(frame: &mut Frame, pane: Rect, theme: &Theme, title: &str,
//                                      border: Style, text: Vec<Line>);
//                   pub fn draw_help(..); pub fn draw_not_running(..); pub fn draw_no_session(..);
// fit.rs            columns, spans_width, tail, clip, clip_place, wrap, split
// board.rs          BoardState, BoardView, every_flow, impl Widget, header, footer, card_rect, Item
// board/cards.rs    impl BoardView { all_header, project_line, master_card, flow_card, state_label,
//                                    render_cards, render_card, render_projects }; gerund
// board/detail.rs   impl BoardView { render_detail, render_master_detail, render_timeline, render_facts }
// sidebar.rs        Sidebar, Row, workers, digit, plain; impl { key_at, rows, flow_blocks, fit,
//                   footer, status }; impl Widget
// sidebar/rows.rs   impl Sidebar { master_rows, flow_rows, insert_peek, peek_rows }
// sidebar/labels.rs impl Sidebar { spinner, glyph, steps, label, state_label, note }; last_note, merge_request
```

## Data

- `cli::Env`: new; seven environment variables as typed fields. main -> lib.
- `Document::Wake(String)`: new variant; news on stderr, exit 2. lib -> main.
- `Command::*(x::Args)`: tuple variants over each command's `Args`; same flags, help, errors.
- `Launch`: removed; `start::Args` has the same fields.
- `deliver::WorkerNews::News(Vec<String>)`: messages, not text; `wait.rs` formats with `output::news`.
- No change to stored data, `proto`, `--json` or TOON.

## Call chains

`flow start <request>`:
1. main -> Cli::try_parse(): Result<Cli, clap::Error>
2. error: DisplayHelp/DisplayVersion -> print, exit 0; other -> Failure::usage(&error) -> fail(): exit 2
3. main -> cli::run(cli, &env(), now_ms): Result<Document, Failure>
4. cli::run -> Env::store(), Env::registry(); none -> Err(Failure::error("cannot find a state directory", ..)) (same two messages as today); Registry::register(&store) ignored
5. cli::run -> Env::config()?, std::env::current_dir()? -> start::run(&args, &store, &registry, &config, &cwd, clock())
6. start::run -> (today's start body) -> output::start::started(..): Document
7. main -> answer(): stdout, exit 0; Err(failure) -> fail(): TOON on stdout, failure.code

`flow wait [--master]`:
1. cli::run -> wait::run(&args, env, clock) before any store lookup
2. wait::run -> env.store() (None -> Silent); waiter from --master or env.flow_id() (None -> Silent)
3. wait -> deliver::worker_news(..): Gone -> None, Nothing -> sleep, News(messages) -> output::news::news_for_worker(..)
4. wait -> deliver::master_news(..): empty -> sleep, else output::news::news_for_master(..)
5. wait::run -> Document::Wake(news) | Document::Silent; main -> eprintln + exit 2 | exit 0

`flow hook <adapter>`:
1. cli::run -> hook::run(&args, env, clock) before any store lookup
2. no store -> eprintln "flow hook: cannot find a state directory", Silent
3. env.config() or Config::builtin() with the stderr note; read stdin (error -> eprintln, Silent)
4. hook(..): today's body; every error is eprintln only; always Silent, exit 0

`flow ui` key press:
1. ui::run loop -> App::on_input(routed) (app/keys.rs) -> on_command | on_keys -> on_key
2. on_key -> on_board_key | on_all_key (app/board_keys.rs) -> focus (app/keys.rs): Vec<Effect>
3. ui::run loop -> App::on_server | on_ran | on_branching | on_tick (app/messages.rs)
4. ui::run loop -> App::draw (app/draw.rs) -> BoardView (board.rs -> board/cards.rs, board/detail.rs), Sidebar (sidebar.rs -> sidebar/rows.rs, sidebar/labels.rs), dialog::draw_dialog

Server: Server::handle (server.rs) -> message (conn.rs) -> refresh / open_worker (panes.rs) -> flush (conn.rs). Unchanged order.

## Test seams

- `tests/cli.rs`: unchanged; must pass after every task. Proof of no behaviour change.
- Goldens (`every_view_looks_as_it_did`, `src/ui/golden`): unchanged files.
- `cli/env.rs`: unit tests for store, config_dir, claude_home, flow_id, bin_path from given fields.
- `cli.rs`: unit tests: wait and hook without a store answer Silent; another command without a store fails with "cannot find a state directory".
- `deliver.rs`: unit tests on a temp store: undelivered messages, a finished flow is Gone, delivered marks.
- `board/cards.rs`, `board/detail.rs`: new unit tests rendering into a `Buffer`.
- Every other unit: its tests move with it, unchanged.

## Assumptions

- Each command's arguments are `Args` (`start::Args`), not `StartArgs`.
- Files of 280-370 lines with one job stay whole: newflow, theme, host, terminal, claude_code, input, store.
- `report`'s `#[arg(env = "FLOW_ID")]` stays a clap feature, not an `Env` field read.
- `docs/HANDOVER.md` and old plans keep old paths in prose; `CLAUDE.md` and `README.md` are updated.
- Before/after check: the base commit's binary and the new one give byte-identical output for a scripted set of commands.

## CLAUDE.md structure rules (to add)

- One crate, a library with a thin binary. `main.rs` reads environment variables and the clock, prints, and maps exit codes; nothing else.
- One responsibility per file, nameable without "and". Past about 400 production lines, split along a real seam; a uniform file (a table of documents, one match of key bindings) may run longer.
- Unit tests inline in `mod tests`; past about 400 lines they move to a sibling `tests.rs` (`#[cfg(test)] mod tests;`), split further into `tests/*.rs` by subject when large. CLI behaviour is tested in `tests/cli.rs`.
- Module files are `foo.rs` + `foo/`, never `mod.rs` (`clippy::mod_module_files`).
- A command is `cli/commands/<name>.rs`: its clap `Args` and one `run`. Its TOON lives in `cli/output/`.
- Modules are nouns for what they hold, never `utils` or `helpers`. `use crate::` paths. Modules private in `lib.rs` except `cli`. Re-export where it gives a type its natural path.

## Tasks

### 1. Groundwork
- [x] Capture the before state: build the base commit's binary into the scratchpad and write a script that runs a fixed set of commands (home, new, report, status, status --json, status --all, send, remove, clear, usage errors, help) in a temp FLOW_HOME and saves stdout, stderr and exit codes
- [x] Module file style: `ui/mod.rs` -> `ui.rs`, `server/mod.rs` -> `server.rs`, `adapter/mod.rs` -> `adapter.rs`; add `mod_module_files = "warn"` to `[lints.clippy]`
- [x] Add `text.rs` (plural, short_duration from output.rs; long_duration, clock from ui/board.rs) and `ui/fit.rs` (columns, spans_width, tail, clip, clip_place, wrap, split from sidebar.rs) with their tests moved; update every import

### 2. Library and binary
- [x] Add `lib.rs` and `cli.rs`: move the module tree to lib.rs and main.rs's handlers into cli.rs as they are; add `cli/env.rs` with `Env` and its methods (`pub use env::Env`) replacing locate_store, load_config, config_dir, env_path, bin_path; `Document::Wake`; main.rs becomes main, env, now_ms, answer, fail; `ui::run` and `server::run` take the clock and the cwd; unit tests for `Env`
- [ ] Add `deliver.rs` with WorkerNews (News carries messages), worker_news, master_news; wait formats with output::news_for_worker / news_for_master; unit tests on a temp store

### 3. Commands
- [ ] `cli/commands.rs` with views and no_flow; `home.rs`, `new.rs`, `report.rs` (ReportEvent moves here), `send.rs`, `status.rs`, each with its `Args`; `Command` uses tuple variants for them
- [ ] `start.rs` with `Args` replacing `Launch`, Rollback, pick_kind, claim_slot, held_slots, claim_is_live, free_branch, append_workspace
- [ ] `retire.rs` (Uncommitted, Unmerged, CLOSE_WAIT, refuse_*, close_pane, retire_worktree, drop_branch, branch_of), `remove.rs`, `clear.rs`
- [ ] `ui.rs` (with stop_server), `server.rs`, `wait.rs`, `hook.rs` with their `Args`; `cli.rs` is only Cli, Command and run; unit tests for `cli::run` without a store

### 4. Output
- [ ] Split `cli/output.rs` (moved there in 2.1, so main could name `cli::output`; keeps Document, Failure, toon, truncated, DESCRIPTION, LIST_TEXT_LIMIT) into `output/status.rs`, `flow.rs`, `start.rs`, `remove.rs`, `news.rs`, each with its tests

### 5. Core
- [ ] Split `launch.rs` into `launch/name.rs`, `worktree.rs`, `pane.rs` with their tests; move taken_slots and free_slot to `slots.rs`
- [ ] Move fold's tests to `fold/tests.rs`

### 6. Server
- [ ] Split `server.rs` into server.rs (run, serve, handle, stop), `server/conn.rs`, `server/panes.rs`, `server/tests.rs`

### 7. UI
- [ ] Add `ui/dialog.rs` (draw_dialog, draw_help, draw_not_running, draw_no_session) and `ui/app/effect.rs` (Effect, Job, Removal, outcome)
- [ ] Split App's methods into `app/messages.rs`, `app/keys.rs`, `app/board_keys.rs`, `app/draw.rs`
- [ ] Move App's tests to `app/tests.rs` (fixtures) and `app/tests/{panes,sidebar,board,dialogs,golden}.rs`; goldens unchanged
- [ ] Split `ui/board.rs` into board.rs, `board/cards.rs`, `board/detail.rs`; add unit tests for cards (master card, flow card states, digits) and detail (timeline, facts) rendering into a Buffer
- [ ] Split `ui/sidebar.rs` into sidebar.rs, `sidebar/rows.rs`, `sidebar/labels.rs` with its tests

### 8. Rules and docs
- [ ] CLAUDE.md: a Structure section with the rules above, the Architecture list for the new files, Build Commands unchanged; README mentions of moved files
- [ ] Check every production file against the rules (size, one responsibility, style) and fix what is left

### 9. End to end
- [ ] Run the before script against the new binary and diff: byte-identical stdout, stderr and exit codes; run `flow ui` against a temp store, start a flow, open the board, the help and the stop dialog, and detach
