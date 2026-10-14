# flow ui: a session server that owns the terminals, and a client that draws them

Implementation page (review surface): `docs/design/ui-implementation.html`.
Approaches: `docs/design/ui-approaches.html`. Screens: https://claude.ai/artifact/68Ff2wJP6BcYX6qy4hppXp
System context: `docs/HANDOVER.md`.

## Decision

- `flow server` (hidden subcommand, started by `flow ui` when none runs) owns every PTY, its terminal
  emulator and the process inside it. `flow ui` is a client over a Unix socket: it sends raw key bytes
  and sizes, and draws the focused pane, the sidebar and the views. Agents survive a UI crash or a closed window.
- The log is the only way a pane opens: `flow start` records the flow and its workspace; the server's
  reconciler opens every unfinished flow with a workspace it has not opened this run. Start and restore
  are one code path. Once opened, the server writes `Workspace.window = "flow-ui"` (HOSTED); a HOSTED
  worker is resumed (`claude --resume {id}`) instead of started fresh.
- `flow ui` is the command; bare `flow` stays the agent home. One UI shows the flows of all repos.
- The tmux path goes (no tmux check, no window, no kill-window).
- The server stops only with `C-space Q` or `flow ui --stop`. `C-space Q` lists the unfinished workers
  (working, delegated, waiting, blocked, not responding) with what each is doing:
  - none unfinished: "Stop flow? y stops it, any other key keeps it";
  - some unfinished: `k` keeps them for later (they resume on the next `flow ui`), `e` ends them
    (each gets an Ended event, so it never reopens; worktrees, branches and transcripts stay until
    `flow clear`), `esc` cancels.
  Either way every pane gets SIGHUP and up to 3 s to exit before SIGKILL. `flow ui --stop` keeps them.
- An ended flow's code is never lost and always findable: commits on `flow/<name>`, uncommitted work in
  `<repo>/.flow/worktrees/<name>` (`flow clear` keeps a dirty worktree), the conversation in Claude's
  transcript for the flow id. `flow status <name>` shows worktree, branch and whether the worktree still
  exists; after `e` the stop dialog names each flow's branch and worktree; in flow ui ended flows stay in
  the sidebar under DONE until `flow clear`, peek shows their branch and worktree, and `C-space r` on one
  resumes its Claude session. A stop also forgets the master
  (deletes master.json), so the next `flow ui` starts a new master where it runs; only a crash or a
  reboot resumes the master. Workers are resumed either way: they are unfinished work.
- The master runs the default agent, name `master`, no prompt, in the directory of the first `flow ui`,
  remembered with its session id in `<store root>/master.json`.
- Once usable, `flow ui` is home and the Claude Code board mod is retired; the CLI and `--json` stay.
- Reliable and lightweight first: one thread owns all server state, only the focused pane streams
  (changed rows, at most 60 frames/s), no async runtime.
- Extensions and the theme file come later; colors are named tokens (`Theme`) now.

## Commands and keys

```
flow ui            attach (starts the server when none runs)
flow ui --stop     stop the server and every session in it
flow server        hidden; what flow ui starts
```

Prefix `C-space` (NUL or CSI 32;5u), then: `0`-`9` focus, `j`/`k` peek cursor, `enter` jump to the cursor,
`r` reopen an exited pane, `n` new flow, `o` board, `b` rail, `[` history, `?` help, `d` detach,
`Q` stop server (confirm). `C-space C-space` sends a literal C-space.

## Units

### src/proto.rs (new)

```rust
pub const PROTOCOL_VERSION: u32 = 1;
pub const MAX_MESSAGE: usize = 8 << 20;

pub enum PaneKey { Master, Flow(String) }
pub struct Size { pub cols: u16, pub rows: u16 }
pub enum Role { Attach { size: Size }, Control }

pub enum ClientMsg {
    Hello { version: u32, role: Role },
    Focus(PaneKey),
    Input(Vec<u8>),
    Resize(Size),
    History(Option<u32>),   // lines back; None = live
    Reopen(PaneKey),
    CloseFlow(String),      // Control role
    /// end_flows: append Ended to every unfinished flow first, so none reopens.
    StopServer { end_flows: bool },
}
pub enum ServerMsg {
    Welcome { version: u32 },
    Refused { reason: String },
    Board(Board),
    Panes(Vec<PaneInfo>),
    Frame(Frame),
    Closed(String),
    Detached,               // another flow ui attached
}
pub struct PaneInfo { pub key: PaneKey, pub status: PaneStatus }
pub enum PaneStatus { Running, Exited { code: Option<u32> }, Failed { reason: String } }
pub struct Frame {
    pub key: PaneKey,
    pub full: bool,
    pub rows: Vec<(u16, Vec<Cell>)>,
    pub cursor: Option<(u16, u16)>,
    pub modes: Modes,
    pub history: Option<u32>,
}
pub struct Cell { pub text: String, pub fg: Color, pub bg: Color, pub flags: Flags }
pub enum Color { Default, Indexed(u8), Rgb(u8, u8, u8) }
pub struct Flags(u8);   // bold dim italic underline reverse strike wide spacer
pub struct Modes { pub paste: bool, pub mouse: Mouse, pub sgr_mouse: bool, pub app_cursor: bool, pub focus: bool }
pub enum Mouse { Off, Press, Drag, Motion }

/// Each message as a u32 length followed by its postcard bytes.
pub fn write_msg<W: Write, T: Serialize>(writer: &mut W, message: &T) -> Result<()>;
/// The next message; None on a clean end of stream; Err on a message over MAX_MESSAGE or bytes that do not decode.
pub fn read_msg<R: Read, T: DeserializeOwned>(reader: &mut R) -> Result<Option<T>>;
```

### src/client.rs (new)

```rust
pub struct Client { stream: UnixStream }
pub enum Closing { Closed, NoServer, NoPane, TimedOut }

impl Client {
    /// Connects and says hello; None when no server listens; Err when it refuses (another protocol version).
    pub fn connect(socket: &Path, role: Role) -> Result<Option<Client>>;
    pub fn send(&mut self, message: &ClientMsg) -> Result<()>;
    /// The next server message; None when the server closed the connection.
    pub fn recv(&mut self) -> Result<Option<ServerMsg>>;
    /// A second handle on the same socket, so one thread reads while another writes.
    pub fn try_clone(&self) -> Result<Client>;
}
/// <store root>/server.sock; when that is too long for a socket (macOS: 104 bytes),
/// /tmp/flow-<uid>/<fnv1a of the root>.sock in a 0700 directory. The log sits beside it (.log).
pub fn socket_path(store: &Store) -> PathBuf;
/// Starts `<exe> server` in its own session with output to server.log unless one listens; waits up to 2 s for the socket.
pub fn ensure_server(socket: &Path, exe: &Path, env: &[(String, String)]) -> Result<()>;
/// Asks a running server to end a flow's pane and waits for it; the outcome when there is no server or no pane.
pub fn close_flow(socket: &Path, id: &str, timeout: Duration) -> Result<Closing>;
```

### src/launch.rs (modified, pure)

```rust
/// Workspace.window of a worker flow ui has opened once; its next open resumes it.
pub const HOSTED: &str = "flow-ui";
pub struct LaunchEnv { pub flow_home: Option<PathBuf> }
pub struct PaneSpec { pub key: PaneKey, pub argv: Vec<String>, pub cwd: PathBuf, pub env: Vec<(String, String)> }

/// The agent's command the first time, its resume command once window is HOSTED;
/// None when it must resume and the agent has no resume template.
pub fn worker_spec(flow: &FlowView, agent: &Agent, env: &LaunchEnv) -> Option<PaneSpec>;
/// The agent's resume command with {id} and {name} filled in; None when it has none.
pub fn resume_argv(agent: &Agent, id: &str, name: &str) -> Option<Vec<String>>;
/// FLOW_ID (workers only), FLOW_HOME, TERM=xterm-256color, COLORTERM=truecolor.
pub fn pane_env(id: Option<&str>, env: &LaunchEnv) -> Vec<(String, String)>;
// agent_argv: an argument that fills to "" is dropped (the master has no prompt).
// removed: window_argv, kill_window_argv
```

### src/config.rs, store.rs, fold.rs, view.rs, output.rs, workspace.rs (modified)

```rust
// config.rs
pub struct Agent { pub command: Vec<String>, pub prompt: String, #[serde(default)] pub resume: Option<Vec<String>> }
// builtin claude: resume = ["claude", "-n", "{name}", "--resume", "{id}"]

// store.rs
pub struct FlowLog { pub id: String, pub events: Vec<Event>, pub beat: Option<u64> }
pub struct Stamp { pub modified: SystemTime, pub len: u64 }
impl Store {
    /// The state directory itself, for server.sock, server.log and master.json.
    pub fn root(&self) -> &Path;
    /// Every readable flow log, and a warning per log that could not be read (was main::views).
    pub fn logs(&self) -> (Vec<FlowLog>, Vec<String>);
    /// Size and modification time of each log and heartbeat file, by flow id.
    pub fn stamps(&self) -> Result<Vec<(String, Stamp)>>;
}

// fold.rs (pure)
pub fn views(logs: Vec<FlowLog>, now: u64) -> Vec<FlowView>;

// view.rs: Board and everything in it also derive Deserialize

// output.rs
pub enum Opening { Running, WhenUiRuns }
pub fn started(flow: &FlowView, setup: &str, opening: Opening) -> Document;   // window field and tmux help go, pane added
pub fn removed(name: &str, pane: &str, worktree: &str) -> Document;            // pane: closed / none / timed out

// workspace.rs: removed open_window, close_window
```

### src/main.rs (modified)

```rust
enum Command {
    // New, Report, Status, Start, Remove, Clear as today
    /// Open the terminal UI: the master, every worker and the board
    Ui {
        /// Stop the server and every session in it
        #[arg(long)]
        stop: bool,
    },
    /// The session server flow ui starts on its own
    #[command(hide = true)]
    Server,
}
fn ui(store: &Store, stop: bool) -> Result<Document, Failure>;   // ensure_server + ui::run, or StopServer
fn server(store: Store, config: Config) -> Result<Document, Failure>;
// start: no tmux check, no window; Opening from Client::connect(Control)
// remove / clear: client::close_flow before retire_worktree
```

### src/server/ (new)

```rust
// mod.rs
pub struct ServerPaths { pub socket: PathBuf, pub master: PathBuf }
/// Binds the socket (removing a stale one), opens the master, then loops until StopServer.
pub fn run(paths: &ServerPaths, store: Store, config: Config, env: LaunchEnv, clock: fn() -> u64) -> Result<()>;
enum Event { Accepted(UnixStream), Message(ConnId, ClientMsg), Gone(ConnId), Host(HostEvent), Tick }
impl Server {
    /// One event; Ok(false) once the server should stop.
    fn handle(&mut self, event: Event) -> Result<bool>;
    /// Re-folds when the store changed, reconciles, sends Board and Panes to the attached UI.
    fn refresh(&mut self) -> Result<()>;
    /// Opens and closes what reconcile asked for; marks new workers HOSTED in their log; Host::fail for failed.
    fn apply(&mut self, wanted: Wanted) -> Result<()>;
    /// Sends the focused pane's changed rows, at most 60 times a second.
    fn flush(&mut self) -> Result<()>;
}

// host.rs
pub enum HostEvent { Output(PaneKey, Vec<u8>), Exited(PaneKey, Option<u32>) }
impl Host {
    pub fn new(size: Size, events: Sender<HostEvent>) -> Host;
    /// Spawns the child in a new PTY with a reader and a waiter thread; on Err the pane is kept as Failed.
    pub fn open(&mut self, spec: PaneSpec) -> Result<()>;
    /// Keeps a pane that could not be opened, with the reason shown in it.
    pub fn fail(&mut self, key: PaneKey, reason: String);
    /// Feeds output into the pane's Terminal and writes the Terminal's replies (DA, DSR) back to the PTY.
    pub fn feed(&mut self, key: &PaneKey, bytes: &[u8]);
    /// Writes input to a running pane; Ok without writing to an exited one.
    pub fn write(&mut self, key: &PaneKey, bytes: &[u8]) -> Result<()>;
    /// Every pane to the same size, PTY and Terminal.
    pub fn resize(&mut self, size: Size);
    pub fn exited(&mut self, key: &PaneKey, code: Option<u32>);
    /// SIGHUP to the child's process group, SIGKILL after 3 s; the pane is gone once it exits.
    pub fn close(&mut self, key: &PaneKey);
    /// The pane's rows: all when full, else the damaged ones; scrolled back by history lines.
    pub fn frame(&mut self, key: &PaneKey, full: bool, history: Option<u32>) -> Option<Frame>;
    pub fn panes(&self) -> Vec<PaneInfo>;
    /// Ends synchronized updates their programs left open too long (Terminal::expire_sync).
    pub fn expire_sync(&mut self);
}

// terminal.rs - the only file that knows the emulator crate
impl Terminal {
    pub fn new(size: Size, history: usize) -> Terminal;
    pub fn feed(&mut self, bytes: &[u8]);
    /// Ends a synchronized update the program left open past its timeout, so its screen shows.
    pub fn expire_sync(&mut self);
    /// Answers the program asked for (device attributes, cursor position), drained.
    pub fn replies(&mut self) -> Vec<u8>;
    pub fn resize(&mut self, size: Size);
    /// Screen rows changed since the last call; the cursor's row is always among them,
    /// so the server asks only after new output.
    pub fn damaged(&mut self) -> Vec<u16>;
    pub fn row(&self, row: u16, history: u32) -> Vec<Cell>;
    pub fn cursor(&self) -> Option<(u16, u16)>;
    pub fn modes(&self) -> Modes;
    pub fn history_len(&self) -> u32;
}

// reconcile.rs (pure)
pub struct Wanted { pub open: Vec<PaneSpec>, pub close: Vec<PaneKey>, pub failed: Vec<(PaneKey, String)> }
/// Opens every unfinished flow with a workspace not in known; closes worker panes whose flow is gone;
/// failed: flows it cannot build a spec for (unknown agent, no resume template), with the reason.
pub fn reconcile(flows: &[FlowView], open: &[PaneKey], known: &HashSet<String>, config: &Config, env: &LaunchEnv) -> Wanted;

// watch.rs
pub struct StoreWatch { stamps: HashMap<String, Stamp>, folded_at: u64 }
impl StoreWatch {
    /// True when a log or heartbeat changed, a flow came or went, or 5 s passed since the last fold.
    pub fn changed(&mut self, store: &Store, now: u64) -> Result<bool>;
}

// master.rs
pub struct MasterRecord { pub session: String, pub cwd: PathBuf }
impl MasterRecord {
    /// The record in master.json, or a new one (fresh session id, cwd) written there; true when new.
    pub fn load_or_create(file: &Path, cwd: &Path) -> Result<(MasterRecord, bool)>;
    /// The agent's command (name master, no prompt) when new, its resume command after;
    /// None when it must resume and the agent has no resume template.
    pub fn spec(&self, fresh: bool, agent: &Agent, env: &LaunchEnv) -> Option<PaneSpec>;
}
```

### src/ui/ (new)

```rust
// mod.rs
/// Attaches (sizing the pane before the hello), then loops on stdin, server, resize, started-flow
/// and tick events until detach; what to tell the user.
pub fn run(socket: &Path, theme: Theme) -> Result<String>;

// app.rs
pub enum View { Panes, Board(BoardState), NewFlow(NewFlowForm), History(u32), Help, Stop { unfinished: Vec<String> } }
pub enum Effect { Send(ClientMsg), StartFlow(NewFlowForm), Mirror(Modes), Quit(String) }
impl App {
    pub fn new(size: Size) -> App;
    pub fn areas(&self) -> Areas;
    /// True while a dialog takes the keys instead of the pane.
    pub fn is_modal(&self) -> bool;
    pub fn on_server(&mut self, message: ServerMsg) -> Vec<Effect>;
    pub fn on_input(&mut self, routed: Routed) -> Vec<Effect>;
    pub fn on_resize(&mut self, size: Size) -> Vec<Effect>;
    /// The `flow start` the dialog ran: the flow's name, or the error it printed.
    pub fn on_started(&mut self, result: Result<String, String>);
    /// The flow under the sidebar cursor, for the peek.
    pub fn peeked(&self) -> Option<&FlowView>;
    pub fn draw(&self, frame: &mut ratatui::Frame, theme: &Theme, peek: &PeekCache);
}

// input.rs (pure)
pub enum Routed { Pane(Vec<u8>), Command(Command), View(Vec<Key>), Click { col: u16, row: u16 } }
pub enum Command { Focus(u8), Cursor(i8), Jump, Reopen, NewFlow, Board, Rail, History, Help, Detach, StopServer }
impl Router {
    /// Splits bytes at C-space (NUL or CSI 32;5u): the next key is a Command, C-space twice sends one.
    /// In a modal view every key is decoded for the view instead. SGR mouse events inside `pane`
    /// are rewritten to pane coordinates; outside it they become Routed::Click for the UI.
    pub fn route(&mut self, bytes: &[u8], modal: bool, pane: Rect) -> Vec<Routed>;
}
pub fn decode(bytes: &[u8]) -> Vec<Key>;

// outer.rs
impl Outer {
    /// Raw mode and the alternate screen; restored on Drop, also on panic.
    pub fn enter() -> Result<Outer>;
    /// Turns paste, mouse, cursor-key and focus modes on or off to match the focused pane.
    pub fn mirror(&mut self, modes: &Modes) -> Result<()>;
    pub fn size(&self) -> Result<Size>;
}

// screen.rs
impl Screen { pub fn apply(&mut self, frame: Frame); }
impl Widget for &Screen;

// layout.rs (pure)
pub enum Chrome { Solo, Sidebar, Rail }
/// frame: the rounded border around the pane, its top edge names the session.
pub struct Areas { pub frame: Rect, pub pane: Rect, pub side: Option<Rect>, pub bar: Option<Rect> }
/// Solo: the framed pane, a blank row, the bottom bar (dark panel background); Sidebar: 40 columns
/// on the right; Rail: 6.
pub fn areas(size: Size, chrome: Chrome) -> Areas;

// sidebar.rs, board.rs, rail.rs, help.rs - widgets over the Board
pub struct Sidebar<'a> { pub board: &'a Board, pub panes: &'a [PaneInfo], pub focus: &'a PaneKey, pub cursor: Option<&'a str>, pub peek: &'a [PeekLine], pub theme: &'a Theme }
pub struct BoardView<'a> { pub board: &'a Board, pub state: &'a BoardState, pub theme: &'a Theme }
pub struct Rail<'a> { pub board: &'a Board, pub focus: &'a PaneKey, pub theme: &'a Theme }

// newflow.rs
pub struct NewFlowForm { pub request: String, pub mode: Mode, pub repo: PathBuf, field: Field }
pub enum FormAction { Edit, Submit, Cancel }
impl NewFlowForm {
    /// Prefilled with the focused pane's repo.
    pub fn new(repo: PathBuf) -> NewFlowForm;
    pub fn on_key(&mut self, key: Key) -> FormAction;
}

// peek.rs - reads Claude Code's transcript, never the PTY
pub enum PeekLine { Tool { name: String, target: String }, Result(String), Text(String), Now(String) }
/// ~/.claude/projects/*/<id>.jsonl
pub fn transcript(claude_home: &Path, id: &str) -> Option<PathBuf>;
/// The last `max` lines worth showing, read from the file's last 64 KiB.
pub fn tail(file: &Path, max: usize) -> Result<Vec<PeekLine>>;
impl PeekCache {
    /// Re-reads only when the flow changed or its file grew.
    pub fn refresh(&mut self, flow: Option<&FlowView>);
    pub fn lines(&self) -> &[PeekLine];
}

// theme.rs
pub struct Theme { pub ui: UiTokens, pub state: StateTokens, pub glyphs: Glyphs }
impl Theme {
    /// ANSI colors 0-15, so flow takes on the terminal's own palette.
    pub fn builtin() -> Theme;
    pub fn state(&self, state: State) -> Style;
}
```

## Emulator spike result (task 1.2)

A real `claude --model haiku` session (120x40, trust prompt, one answer with 界面, the board mod band,
Nerd Font glyphs) was recorded in a private tmux and fed to both crates:

- Fidelity: vt100 0.16 and alacritty_terminal 0.26 produce identical screens; both match tmux on 36 of 40
  rows, the other 4 are the live board band that changed between the last recorded byte and the capture.
- Speed: both parse about 7 MB in 45 ms (release).
- What Claude Code sends: the alternate screen (1049), mouse 1000/1002/1003 + SGR 1006, focus 1004,
  bracketed paste 2004, kitty keyboard push/pop (`>5u`/`<u`) and query (`?u`), DA1 (`[c`, 4 times),
  DECRQM for synchronized output (`?2026$p`), XTVERSION (`>0q`), OSC 8 hyperlinks.
- alacritty_terminal answers DA1 and DECRQM itself (`PtyWrite`), buffers synchronized updates, tracks
  damaged rows and lets the kitty keyboard protocol be switched off; vt100 answers nothing and has no damage.

Chosen: **alacritty_terminal**, with `kitty_keyboard: false`, so Claude falls back to legacy key
encoding and C-space stays a NUL byte. Because Claude turns mouse reporting on, `Router::route` also
takes the pane's area and rewrites SGR mouse coordinates into it; clicks outside the pane go to the UI.

## Stored data

- `<store root>/master.json` `{"session": "<uuid>", "cwd": "..."}`; `server.sock`, `server.log` beside it.
- `Workspace.window` is `"flow-ui"` once the server opened the worker. Old tmux ids still fold; such a flow opens fresh once.
- `--json` unchanged (`VIEW_VERSION` 1). The `flow start` TOON loses `window`, gains `pane`.

## Call chains

1. Open flow ui
   1. `main::ui -> client::ensure_server(socket, exe, env): Result<()>`
   2. `ensure_server -> spawn <exe> server` in a new session, output to server.log
   3. `server::run -> MasterRecord::load_or_create(master, cwd) -> Host::open(record.spec(fresh, agent, env)) -> refresh()`
   4. `main::ui -> Client::connect(socket, Role::Attach { size }): Result<Option<Client>>` -> `Hello`
   5. `Server::handle -> Welcome, Board, Panes, Frame { full: true }`
   6. `ui::run -> App::on_server(message): Vec<Effect> -> Outer::mirror(modes) -> App::draw`
   - Error: no socket after 2 s -> `Err("flow server did not start, see <root>/server.log")` -> Failure, exit 1
   - Error: `Refused` (version) -> `Err("flow server runs protocol N; run flow ui --stop and start again")` -> exit 1
2. A key reaches the focused pane, output comes back
   1. stdin thread -> `Router::route(bytes, false, pane): Vec<Routed::Pane(bytes)>`
   2. `App::on_input(Routed::Pane) -> Effect::Send(Input(bytes))` -> `Client::send`
   3. `Server::handle(Message(Input)) -> Host::write(&focus, bytes)`
   4. reader thread -> `HostEvent::Output(key, bytes) -> Host::feed(&key, bytes) -> Terminal::feed`; `Terminal::replies()` written back to the PTY
   5. `Server::flush -> Host::frame(&focus, false, history) -> ServerMsg::Frame` (at most 60/s)
   6. `App::on_server(Frame) -> Screen::apply`; `Effect::Mirror(modes)` when they changed -> draw
   - Error: write to an exited pane -> Ok, nothing written; sidebar shows it exited
3. The master runs flow start; the server opens the worker
   1. `main::start -> create_worktree, Store::create(Started), Store::append(Workspace { window: None }), run_hook(Setup)`
   2. `main::start -> Client::connect(Control)`: Some -> `Opening::Running`, None -> `Opening::WhenUiRuns` -> `output::started`
   3. Tick -> `StoreWatch::changed(&store, now) -> Store::logs() -> fold::views -> fold::board`
   4. `Server::refresh -> reconcile(&flows, &open, &known, &config, &env): Wanted`
   5. `Server::apply -> Host::open(worker_spec(...))`
   6. `Server::apply -> Store::append(id, Workspace { window: Some(HOSTED), .. }); known.insert(id)`
   7. `Server::refresh -> ServerMsg::Board, ServerMsg::Panes`
   - Error: `Host::open` Err -> pane Failed; `Store::append(Blocked { reason: "could not start the agent: ..." })`
   - Error: unknown agent -> `Wanted.failed` -> `Host::fail(key, "no agent named X")`
4. The server starts again after a crash or reboot
   1. `client::ensure_server`: stale socket -> spawn `flow server`
   2. `server::run`: bind fails and nobody answers -> remove server.sock -> bind
   3. `MasterRecord::load_or_create: (record, false) -> Host::open(record.spec(false, agent, env))` (resume)
   4. `reconcile` with `known` empty -> every unfinished flow with a workspace
   5. `worker_spec`: window == HOSTED -> agent.resume argv -> `Host::open`
   - Error: no resume template -> `Wanted.failed` -> `Host::fail(key, "agent X cannot resume")`
   - Error: the resumed process exits non-zero within 5 s (Claude saved no session, it saves one only
     after the first message) -> the server opens it once more fresh, same session id
5. A worker exits; you resume it
   1. waiter thread -> `HostEvent::Exited(key, code) -> Host::exited -> ServerMsg::Panes`
   2. `App::on_server(Panes)`: row shows exited, pane keeps its last screen
   3. `C-space r -> Command::Reopen -> Effect::Send(Reopen(focus))`
   4. `Server::handle(Reopen) -> worker_spec -> Host::open`
6. flow remove / flow clear
   1. `main::remove -> client::close_flow(socket, id, 5 s): Result<Closing>`
   2. `Client::connect(Control) -> CloseFlow(id) -> Server::handle -> Host::close(&Flow(id))`
   3. `HostEvent::Exited -> pane dropped -> ServerMsg::Closed(id)`
   4. `retire_worktree -> Store::remove(id) -> output::removed(name, "closed", worktree)`
   - `NoServer` -> pane "none"; `TimedOut` -> pane "timed out", worktree retired anyway
7. Peek
   1. `C-space j -> Command::Cursor(1) -> App::on_input`: cursor moves
   2. `ui::run -> PeekCache::refresh(app.peeked())` after each event and every second
   3. `peek::transcript(claude_home, id) -> peek::tail(file, max)`
   4. `App::draw -> Sidebar { cursor, peek: peek.lines(), .. }`
   - No transcript -> no peek lines
8. New flow dialog
   1. `C-space n -> View::NewFlow(NewFlowForm::new(repo of focused pane))`
   2. `Router::route(bytes, true, pane) -> Routed::View(keys) -> NewFlowForm::on_key: FormAction`
   3. `Submit -> Effect::StartFlow(form)` -> thread runs `<exe> start <request> --mode <mode>` in form.repo -> `Event::Started`
   4. the pane appears through chain 3
   - `flow start` exits 1 -> `App::on_started(Err(line))`: toast, form reopens with its text
9. Detach, take over, stop
   1. `C-space d -> Effect::Quit("detached")` -> Outer restores the terminal; server sees `Gone`, panes keep running
   2. second `flow ui` attaches -> `Detached` to the old one -> `Quit("attached elsewhere")`
   3. `C-space Q -> View::Stop { unfinished }` (names of the unfinished flows on the board)
      - none: `y -> StopServer { end_flows: false }`
      - some: `k -> StopServer { end_flows: false }`; `e -> StopServer { end_flows: true }`; `esc` back
      - `flow ui --stop` over Control sends `StopServer { end_flows: false }`
   4. `Server::handle(StopServer { end_flows })`: when end_flows, `Store::append(id, Ended)` for each unfinished
      flow; delete master.json; `Host::close` every pane (SIGHUP, SIGKILL after 3 s), wait, remove socket, exit
   - Panic in the UI: Outer's Drop/panic hook restores the terminal; server and panes keep running

## Test seams

- `proto`: round trip of every message; split reads; oversize and garbage are errors, clean EOF is None.
- `Terminal`: byte sequences in, rows/cursor/modes/replies out (golden), incl. wide chars, scroll regions, DA/DSR.
- `Host`: real `sh` in a PTY: output arrives, `stty size` follows resize, exit code arrives, close kills a group that ignores SIGHUP.
- `reconcile`, `worker_spec`, `MasterRecord::spec`, `layout::areas`, `Router::route`: pure table tests.
- `StoreWatch`: temp store; append, beat, remove, the 5 s refold.
- `App`: messages and inputs in, effects out; drawing with ratatui `TestBackend` against the canvas screens.
- `peek::tail`: fixture transcripts, incl. a truncated last line.
- Server integration: temp `FLOW_HOME`, `cat` as the claude agent via config.toml, real socket: attach, type, see frames; `flow start` opens a pane; kill and restart resumes.

## Assumptions

1. Plain threads and channels; one thread owns the Host. No async runtime.
2. Wire format: u32 length + postcard.
3. Only the focused pane streams: changed rows, at most 60 frames/s; every pane has the size of the pane area.
4. The server checks the store every 500 ms by size and mtime; refolds at least every 5 s.
5. An exited pane keeps its last screen under a notice and is not reopened on its own; `C-space r` resumes it.
6. Keys reach the pane as raw bytes; the outer terminal mirrors the pane's paste, mouse and cursor-key modes.
7. `flow start` without a running server records the flow and says it opens when flow ui runs.
8. One UI attached at a time; a new one takes over.
9. `flow remove` waits up to 5 s for the pane to close, then tears down anyway and reports "timed out".

## Tasks

### Milestone 1: one pane through the server

- [x] Add proto.rs (all message types, write_msg, read_msg) and derive Deserialize on view.rs types, with tests for round trips (incl. Board), split reads, oversize, garbage and clean EOF
- [x] Emulator spike: run a real `claude` session through vt100 and alacritty_terminal behind Terminal, compare fidelity (redraws, wide chars, colors, DA/DSR replies, scrollback, resize) and speed; record the result in the plan and keep the winner
- [x] Add server/terminal.rs Terminal on the chosen crate with golden tests for feed, replies, damaged, row, cursor, modes and history
- [x] Add launch::PaneSpec, LaunchEnv, pane_env and the empty-argument rule in agent_argv, with tests
- [x] Add server/host.rs Host (open, fail, feed, write, resize, exited, close, frame, panes) with PTY tests on real `sh`
- [x] Add config Agent.resume (builtin claude `--resume {id}`) and server/master.rs MasterRecord (load_or_create, spec) with tests
- [x] Add server/mod.rs run and Server (handle, flush; master pane only, Hello/Welcome/Refused, Input, Resize, Focus, StopServer, stale socket) with an integration test over a real socket and a `cat` master
- [x] Add client.rs (Client::connect, send, recv, try_clone, socket_path, ensure_server) with tests against a test server
- [x] Add ui outer.rs, input.rs Router, screen.rs Screen, layout.rs (Solo), app.rs App (pane + bottom bar) and ui::run, with Router, layout and App tests
- [x] Add `flow ui [--stop]` and hidden `flow server` to main.rs
- [x] End-to-end check of milestone 1: real claude master in flow ui, type and scroll, detach and reattach, kill the ui and see claude still running

### Milestone 2: workers and the sidebar

- [x] Add Store::root, Store::logs, Store::stamps and fold::views; replace main::views; tests
- [x] Add server/watch.rs StoreWatch with tests for change, beat, removal and the 5 s refold
- [ ] Add launch::worker_spec and HOSTED with tests (fresh, resume, no resume template)
- [ ] Add server/reconcile.rs reconcile with table tests (open, known, close removed, failed)
- [ ] Server::refresh and Server::apply: board broadcast, worker panes, HOSTED append, Host::fail and Blocked on failure; integration test with a `cat` agent
- [ ] Add ui theme.rs Theme::builtin, sidebar.rs Sidebar, layout Sidebar chrome, App focus switching (0-9) and pane status; TestBackend tests against the fleet and worker screens

### Milestone 3: flow start, remove and clear through the server

- [ ] main::start without tmux: no window, Opening via Client::connect(Control), output::started with pane; update tests/cli.rs
- [ ] client::close_flow; main::remove and main::clear close panes first; output::removed with pane; tests
- [ ] Remove window_argv, kill_window_argv, open_window, close_window and their tests
- [ ] Reopen exited panes and ended flows (C-space r, ClientMsg::Reopen: resume the flow's agent even when its flow is ended) with an integration test
- [ ] `flow status <name>` shows the worktree path, the branch and whether the worktree still exists, for every flow with a workspace; tests/cli.rs

### Milestone 4: views

- [ ] Add ui/peek.rs (transcript, tail, PeekCache) with fixture tests; sidebar peek, C-space j/k and enter; ended flows stay under DONE until `flow clear` and their peek shows branch and worktree
- [ ] Add ui/board.rs BoardView and C-space o, with TestBackend tests against the board screen
- [ ] Add ui/newflow.rs NewFlowForm, C-space n and the `flow start` thread with App::on_started, with tests
- [ ] Add ui/rail.rs Rail and C-space b
- [ ] History (C-space [, ClientMsg::History) through Host::frame
- [ ] Help (C-space ?) and the stop dialog (C-space Q): unfinished workers listed, `k` keeps them, `e` ends them (StopServer { end_flows }) and the UI prints each ended flow's branch and worktree as it quits, `y` when none; tests for each answer

### Milestone 5: restore and hardening

- [ ] Restore integration test: start flows, kill the server, start flow ui again, workers and master resume
- [ ] Measure idle CPU and memory of server and ui with five panes; fix anything above near zero idle
- [ ] Remove the `#[allow(dead_code)]` the new modules carry in main.rs until they are wired
- [ ] Update CLAUDE.md architecture and docs/HANDOVER.md (flow ui, board mod retired, tmux path gone)
- [ ] End-to-end check: a real Claude master starts two workers from flow ui, peek, answer a question in a worker, detach, reattach, kill the server, restart, everything resumes
