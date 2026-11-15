# One dispatch and named threads

Review page: https://claude.ai/artifact/23W5yA7pJCJnPKneq7CTTr (approaches: https://claude.ai/artifact/L6mqSuYWBsKwhrSMhL1pQM).
From the rust-quality-audit report, items 3 and 4. Stacked on `flow/typed-errors`.

## Decision

- `cli::run` builds one `Context { env, clock }` and matches the command once (no early-return
  match, no dead arm). Every command's `run` takes the Context; a command that needs the store
  calls `ctx.open()?` first and derives socket, trust, config and cwd itself.
- Every thread starts through `threads::spawn(name, body)`; `clippy.toml` bans
  `std::thread::spawn` and `std::thread::Builder::spawn` elsewhere.
- A panic in any UI thread ends the UI: the terminal is restored (existing outer hook), the panic
  and backtrace go to server.log, one line says where, exit 101.

Reproduced before the design: with a panic injected in the UI's server-reader thread, bare `saaio`
in a PTY on a scratch store ignored `saaio --stop` and ran until killed; server.log said "panic in
an unnamed thread". The outer hook had already restored the terminal, so the UI went on drawing
into a cooked terminal.

## Units

### src/cli/context.rs (new, private module of cli)

```rust
pub struct Context<'a> {
    pub env: &'a Env,
    pub clock: fn() -> u64,
}

pub struct Opened {
    pub store: Store,
    pub registry: Registry,
}

impl Context<'_> {
    /// The clock read once.
    pub fn now(&self) -> u64;
    /// The store Env::store picks for the current folder and the registry, the store registered
    /// there (best effort). Err: "could not open the store: {error:#}" / "cannot find a state
    /// directory" with today's help lines.
    pub fn open(&self) -> Result<Opened, Failure>;
}
```

Commands reach it as `crate::cli::context::{Context, Opened}`.

### src/cli.rs (modified)

```rust
pub fn run(cli: Cli, env: &Env, clock: fn() -> u64) -> Result<Document, Failure>;
// same signature: builds Context, one `match cli.command`.
```

### src/cli/commands/*.rs (modified)

```rust
// store-less
hook::run(args: &Args, ctx: &Context) -> Document;
turn::run(args: &Args, ctx: &Context) -> Document;
wait::run(args: &Args, ctx: &Context) -> Document;
completions::run(args: &Args) -> Document;                        // needs nothing
update::run(args: &Args, ctx: &Context) -> Result<Document, Failure>;
trust::run(args: &Args, ctx: &Context) -> Result<Document, Failure>;
bug_report::run(ctx: &Context) -> Result<Document, Failure>;

// with a store, each opens it first
new::run(args: Args, ctx: &Context) -> Result<Document, Failure>;
report::run(args: Args, ctx: &Context) -> Result<Document, Failure>;
status::run(args: &Args, ctx: &Context) -> Result<Document, Failure>;
start::run(args: &Args, ctx: &Context) -> Result<Document, Failure>;
remove::run(args: &Args, ctx: &Context) -> Result<Document, Failure>;
send::run(args: &Args, ctx: &Context) -> Result<Document, Failure>;
clear::run(ctx: &Context) -> Result<Document, Failure>;
server::run(ctx: &Context) -> Result<Document, Failure>;
stop::run(ctx: &Context) -> Result<Document, Failure>;
ui::run(ctx: &Context) -> Result<Document, Failure>;
home::run(ctx: &Context) -> Document;   // drops its #[expect(clippy::unnecessary_wraps)]
```

Inner helpers (`start::plain::open`, `status::one_store`, `retire::*`) keep explicit arguments.

### src/threads.rs (new)

```rust
/// A thread called `name` running `body`; Err when the system cannot start one.
pub fn spawn<T, F>(name: impl Into<String>, body: F) -> io::Result<JoinHandle<T>>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static;
```

Holds the one `#[allow(clippy::disallowed_methods, reason = ...)]`. Bottom module in `layering.rs`.

### clippy.toml (modified)

```toml
{ path = "std::thread::spawn", reason = "start threads with a name through threads::spawn" },
{ path = "std::thread::Builder::spawn", reason = "start threads with a name through threads::spawn" },
```

### src/server.rs and server/* (modified)

`spawn_named` removed; its callers use `threads::spawn(name, body)`, `.map(drop)` where they keep
only `io::Result<()>`. Names unchanged.

### src/ui.rs, src/ui/update_watch.rs (modified)

```rust
fn read_stdin(events: Sender<Event>) -> io::Result<()>;                 // "ui stdin"
fn read_server(client: Client, events: Sender<Event>) -> io::Result<()>; // "ui server"
fn watch_resize(events: Sender<Event>) -> Result<()>;                   // "ui resize"
fn tick(events: Sender<Event>) -> io::Result<()>;                       // "ui tick"
fn run_job(exe: &Path, job: Job, events: Sender<Event>);  // "ui job"; spawn error -> Event::Ran(job, Err)
fn ask_tmux(text: String, events: Sender<Event>, log: &Path, clock: fn() -> u64); // "ui tmux"; error logged
UpdateWatch::ask_if_due(&mut self, now: u64, events: &Sender<Event>);  // "ui update"; error leaves asking false
```

The four startup threads fail `ui::run` with "could not start the ui's <name> thread".

### src/system.rs, src/workspace/process.rs (modified)

```rust
// system.rs: "probe uname", "probe <tool>", "probe <program> stdout";
// a probe whose thread cannot start is Probe::Failed("could not start a thread: ...").
fn read_first_line(out: ChildStdout, name: String) -> io::Result<Receiver<Option<String>>>;

// process.rs: "<program> stdout" / "<program> stderr"; the run fails with the spawn error.
fn read_in_background(pipe: Option<impl Read + Send + 'static>, name: String) -> io::Result<Pipe>;
```

### src/server_log.rs (modified)

```rust
/// Appends `<source>: panic ...` and its backtrace to `log`, runs the hook before it, says where
/// the backtrace is, and ends the process (exit 101).
pub fn append_and_exit_on_panic(log: PathBuf, source: &'static str, clock: fn() -> u64);
// replaces append_on_panic; its one caller is cli/commands/ui.rs
```

## Call chains

saaio remove (a command with a store):
1. main -> cli::run(cli, &env, clock): Result<Document, Failure>
2. cli::run -> Context { env, clock }
3. cli::run -> remove::run(&args, &ctx)
4. remove::run -> Context::open(): Result<Opened, Failure>
5. Context::open -> Env::store(cwd), Env::registry(), Registry::register(&store) (error ignored)
6. remove::run -> Env::socket(&store), Trust::at(&registry), Context::now()
7. Error: no store or registry -> Failure with today's message and help, exit 1.

saaio hook claude-code (no store):
1. cli::run -> hook::run(&args, &ctx): Document -> Ok(document); open is never called.

A UI thread panics:
1. commands::ui::run -> server_log::append_and_exit_on_panic(log, "ui", clock)
2. ui::run -> Outer::enter() -> install_panic_restore() (wraps it)
3. panic -> restore() -> append(log, "ui: panic in thread 'ui server' at ...") -> default hook
   -> eprintln "the saaio ui stopped after a panic; its backtrace is in <log>" -> exit(101)
4. Server and panes keep running; `saaio` attaches again.

## Test seams

- `Context::open`: unit tests in cli/context.rs (scratch SAAIO_HOME opens and registers; none fails
  with today's message); the two tests in cli.rs stay.
- `threads::spawn`: unit test that the body sees its name and the handle returns its value.
- Commands: the end-to-end suite in tests/cli/.
- Naming: clippy.
- UI panic: end to end with the PTY script and a temporarily injected panic (not committed).

## Assumptions

- Infallible commands return `Document`; `completions::run` takes no Context.
- Store commands open the store first, so a missing store fails before anything else.
- Registration stays best effort.
- Each command reads `ctx.now()` once.
- CHANGELOG: one Fixed line for the UI panic; nothing for dispatch or names.

## Tasks

- [x] Add `threads::spawn` (src/threads.rs) with its unit test, in lib.rs and layering.rs
- [ ] Move the server to `threads::spawn` and remove `server::spawn_named`
- [ ] Name the UI's threads (ui.rs, ui/update_watch.rs) through `threads::spawn`, spawn errors handled as planned
- [ ] Name system.rs's and workspace/process.rs's threads through `threads::spawn`
- [ ] Ban `std::thread::spawn` and `Builder::spawn` in clippy.toml; clippy clean
- [ ] Replace `append_on_panic` with `server_log::append_and_exit_on_panic` and use it in commands/ui.rs
- [ ] Add `cli::context::{Context, Opened}` with unit tests for `open`
- [ ] Move every command to `run(args, &Context)` and make `cli::run` one match; tests/cli green
- [ ] CLAUDE.md (threads.rs, cli/context.rs, server spawn note) and the CHANGELOG Fixed line
- [ ] End to end: the PTY script with an injected reader panic exits in a second with 'ui server' in server.log; without it `saaio --stop` still ends the UI; a few commands (status, start, remove, hook) on a scratch store
