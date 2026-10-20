# Copy text from the UI

Implementation page: https://claude.ai/artifact/MNXznsovacXRwbNcy8uRw3
(approaches: https://claude.ai/artifact/G8iMNgcALPerziiD8kWTvP)

## Decision

Reproduced: Claude Code 2.1.304 and Codex 0.163 turn on `?1003h` + `?1006h`; saaio mirrors that
onto the real terminal, then drops every mouse event outside the pane but a left press, so no drag
selects anything. A pane's own OSC 52 copy (Claude Code over SSH) is dropped by
`server/terminal.rs` (`Event::ClipboardStore` ignored).

Approach A: saaio owns the mouse, like tmux `mouse on`. While the UI runs the real terminal always
reports buttons and drags (at least `?1002h`, always SGR). A drag on a saaio surface (open dialog,
board, title row, sidebar or rail, history view, a pane whose program does not use the mouse)
selects inside that surface, is highlighted, and is copied **on release** via OSC 52 plus the system
clipboard command when not over SSH. Double-click selects a word, triple-click a line. A pane whose
program uses the mouse gets every event it asked for, encoded as it asked (SGR or X10); its OSC 52
is passed on when the pane is in view. The **wheel** over a pane that does not use the mouse scrolls
saaio's history (opens it; down past the newest line leaves it). Shift-drag (Option-drag in iTerm2
and Terminal.app) still gives the terminal's own selection. docs/ui.md "Mouse" is rewritten. No
CHANGELOG entry (none before 0.1.0).

## Units

### `src/ui/selection.rs` (new, pure)

```rust
/// How far a selection reaches past the cells under it: a drag, a double-click, a triple-click.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit { Cell, Word, Line }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection { bounds: Rect, anchor: Position, head: Position, unit: Unit }

impl Selection {
    pub fn new(bounds: Rect, at: Position, unit: Unit) -> Selection;
    /// Moves the free end to `to`, clamped into bounds.
    pub fn extend(&mut self, to: Position);
    /// A press and release on one cell with Unit::Cell: a click, nothing selected.
    pub fn is_click(&self) -> bool;
    /// Rows in reading order, each with trailing blanks trimmed, joined by newlines; Word and Line
    /// widen the ends in `buf`. Wide-char spacer cells are skipped.
    pub fn text(&self, buf: &Buffer) -> String;
    /// Reverses the selected cells.
    pub fn render(&self, buf: &mut Buffer);
}
```

A word is a run of cells that are neither blank nor box-drawing. A Word selection on a blank cell
is empty and copies nothing.

### `src/ui/clipboard.rs` (new)

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Clipboard { command: Option<&'static [&'static str]> }

impl Clipboard {
    /// pbcopy on macOS; wl-copy under Wayland, else xclip -selection clipboard under X11; none over SSH.
    pub fn new(ssh: bool, wayland: bool, x11: bool) -> Clipboard;
    /// Writes osc52(text) to `out`, then pipes `text` to the command (waits for it); Err names what failed.
    pub fn copy(&self, out: &mut impl Write, text: &str) -> Result<()>;
}

/// ESC ] 52 ; c ; base64 BEL.
fn osc52(text: &str) -> String;
```

New dependency `base64 = "0.22"`.

### `src/ui/input.rs` (modified)

```rust
pub enum Routed {
    Pane(Vec<u8>),
    Command(Command),
    View(Vec<Key>),
    /// A mouse event for saaio. Replaces Click.
    Pointer(Pointer),
}

pub struct Pointer { pub kind: PointerKind, pub at: Position }
pub enum PointerKind { Press, Drag, Release, WheelUp, WheelDown }

pub struct Target { pub pane: Rect, pub modal: bool, pub mouse: Mouse, pub sgr: bool }

pub struct Router { /* existing */ held: Option<Holder> }
enum Holder { Pane, Saaio }

impl Router {
    pub fn route(&mut self, bytes: &[u8], target: Target) -> Vec<Routed>;
}

/// The report in pane coordinates, SGR or X10 as the pane asked; None past X10's 223 columns.
fn encode(event: MouseEvent, pane: Rect, sgr: bool) -> Option<Vec<u8>>;
/// Whether a program in `mouse` mode asked for `event`: Press press/release/wheel, Drag also
/// motion with a button, Motion all motion.
fn wanted(mouse: Mouse, event: MouseEvent) -> bool;
```

Private `MouseEvent` gains `motion: bool` (button bit 32). Routing: modal -> every left
press/drag/release and wheel is a Pointer. Not modal: a press inside the pane with `mouse != Off`
sets `held = Pane`; while held, motion and release go to the pane (coordinates clamped to the pane)
when `wanted`. Any other left press sets `held = Saaio`; while held, drags and the release are
Pointers wherever they are. Wheel inside a mouse pane goes to the pane, elsewhere it is a Pointer.
Release clears `held`. Motion without a button outside a mouse pane, and other buttons outside the
pane, are dropped.

### `src/ui/outer.rs` (modified)

```rust
/// The pane's modes with saaio's own mouse needs on top: buttons and drags at least, always SGR.
fn captured(modes: Modes) -> Modes;
```

`enter` writes the capture of `Modes::default()` before the first frame; `mirror` writes
`mode_changes(captured(old), captured(new))`.

### App (`src/ui/app.rs`, `app/pointer.rs` new, `app/keys.rs`, `app/messages.rs`, `app/draw.rs`, `app/effect.rs`)

```rust
// app.rs
selection: Option<Selection>,
clicks: Option<Clicks>,
struct Clicks { at: Position, time: u64, count: u8 }

impl App {
    /// The open dialog, the board (whole screen), the title row, the pane, the sidebar or rail.
    fn surface_at(&self, at: Position) -> Option<Rect>;
    /// The dialog the view shows over the session, shared by draw_view and surface_at.
    fn dialog(&self, theme: &Theme) -> Option<Dialog<'_>>;
    /// The focused pane's modes, for the router's Target.
    pub fn pane_modes(&self) -> Modes;
}

// app/pointer.rs
const WHEEL_LINES: u32 = 3;
const MULTI_CLICK_MS: u64 = 500;
impl App {
    pub(super) fn on_pointer(&mut self, pointer: Pointer) -> Vec<Effect>;
    fn on_wheel(&mut self, kind: PointerKind) -> Vec<Effect>;
    /// Moved from keys.rs.
    fn on_click(&mut self, at: Position) -> Vec<Effect>;
}

// app/effect.rs
pub enum Effect {
    // existing
    CopySelection(Selection),
    Copy(String),
}
```

- `on_input`: `Routed::Pointer` -> `on_pointer`; any other input clears `selection`. Focus change,
  resize and opening a view clear it too.
- `on_server(ServerMsg::Clipboard(text))` -> `vec![Effect::Copy(text)]`.
- `draw` ends with `selection.render(frame.buffer_mut())` (board view too).
- `draw_view` uses `dialog()` for Stop, Confirm, Help; NewFlow's box from `NewFlowView::dialog()`.

### `src/ui.rs` (modified)

`UiEnv` gains `pub clipboard: Clipboard`. The loop routes with
`Target { pane: app.areas().pane, modal: app.is_modal(), mouse: modes.mouse, sgr: modes.sgr_mouse }`
from `app.pane_modes()`, keeps `drawn: Buffer` cloned from each `terminal.draw` result, and runs
`Effect::CopySelection(sel)` as `clipboard.copy(&mut stdout, &sel.text(&drawn))` and
`Effect::Copy(text)` as `clipboard.copy(&mut stdout, &text)`; an Err is logged as
`ui: copy failed: {error:#}`.

### Env (`src/main.rs`, `src/cli/env.rs`, `src/cli/commands/ui.rs`)

```rust
pub struct Env {
    // existing
    /// $SSH_CONNECTION or $SSH_TTY is set.
    pub ssh: bool,
    /// $WAYLAND_DISPLAY / $DISPLAY are set.
    pub wayland: bool,
    pub x11: bool,
}
impl Env {
    pub fn clipboard(&self) -> Clipboard;
}
```

### Server (`src/server/terminal.rs`, `host.rs`, `server.rs`, `conn.rs`, `src/proto.rs`)

```rust
// terminal.rs
struct Listener { /* existing */ copied: Arc<Mutex<Option<String>>> }
impl Terminal {
    /// The text the program last put on the clipboard (OSC 52), drained.
    pub fn copied(&mut self) -> Option<String>;
}

// host.rs
pub fn feed(&mut self, key: &PaneKey, bytes: &[u8]) -> Option<String>;

// conn.rs
/// Sends the text to the attached UI; over MAX_MESSAGE it is logged and not sent.
pub(super) fn send_clipboard(&mut self, text: String);

// proto.rs
pub const PROTOCOL_VERSION: u32 = 7;
#[derive(..., PartialOrd, Ord)] pub enum Mouse { Off, Press, Drag, Motion }
pub enum ServerMsg { /* existing */ Clipboard(String) }
```

Clipboard (`c`) and primary (`p`) stores both pass on; OSC 52 reads stay refused (`Osc52::OnlyCopy`).

## Data

| Type | Fields | Crosses |
| --- | --- | --- |
| `Selection`, `Unit` | bounds, anchor, head (screen cells), unit | App -> Effect -> ui.rs |
| `Pointer`, `PointerKind` | kind, at | Router -> App |
| `Target` | pane, modal, mouse, sgr | ui.rs -> Router |
| `ServerMsg::Clipboard(String)` | decoded text | server -> UI (protocol 7) |
| `Clipboard` | command argv | main -> Env -> UiEnv |

## Call chains

### Drag over a saaio surface

1. `ui.rs -> Router::route(bytes, target): Vec<Routed>` - press outside a mouse pane: `held = Saaio`, `Routed::Pointer(Press)`.
2. `ui.rs -> App::on_input(Pointer) -> on_pointer` - `surface_at(at): Some(bounds)`; click count from `clicks`; `selection = Some(Selection::new(bounds, at, unit))`.
3. Drag: `Router` (held Saaio) -> `Pointer(Drag)` even over the pane -> `Selection::extend(at)`; redraw.
4. `App::draw -> Selection::render(buf)`; loop keeps `drawn`.
5. Release: `held = None`; `is_click()` -> `on_click(at)` (sidebar or rail focus), `selection = None`; else `Effect::CopySelection(sel)`, highlight stays until the next input.
6. `ui.rs -> Clipboard::copy(&mut stdout, &sel.text(&drawn)): Result<()>`.
7. Error: command missing or failing -> Err -> server.log `ui: copy failed: ...`; nothing shown. Press outside every surface -> `surface_at: None`, nothing.

### Drag in a pane whose program uses the mouse

1. `Router::route` - press in pane, `mouse != Off`: `held = Pane`; `Routed::Pane(encode(event, pane, sgr))` when `wanted`.
2. `App::on_input -> Effect::Send(ClientMsg::Input(bytes))`.
3. Motion leaving the pane keeps going to it while held, clamped to its edge.

### A pane's program copies with OSC 52

1. `Server::handle(Host(Output(key, bytes))) -> Host::feed(&key, &bytes): Option<String>`.
2. `Host::feed -> Terminal::feed`; `Listener` stores `ClipboardStore` text; `Terminal::copied()` drains it.
3. `key == focus` -> `Server::send_clipboard(text) -> send_to(attached, ServerMsg::Clipboard(text))`.
4. `App::on_server(Clipboard(text)) -> Effect::Copy(text)` -> `Clipboard::copy`.
5. Error: no UI attached or pane not in view -> dropped; over MAX_MESSAGE -> logged, not sent.

### Wheel

1. Over a pane with mouse Off: `Router -> Pointer(WheelUp)` -> `on_wheel`: `View::Panes` -> `view = History(3)`, `Send(History(Some(3)))`.
2. In `History(n)` (modal): up -> `n + 3`; down -> `n - 3`, at 0 -> `view = Panes`, `Send(History(None))`.
3. Over the sidebar, board or dialogs: nothing.

### Double and triple click

1. Press on the same cell within `MULTI_CLICK_MS` of the last: count 2 -> `Unit::Word`, 3 -> `Unit::Line`.
2. Release copies (never `is_click`); an empty word copies nothing.

## Test seams

- `selection.rs`: hand-built `Buffer`; forward and backward drags over rows, trailing blanks, clamping, wide chars, word and line widening, `render` reverses exactly the selected cells.
- `clipboard.rs`: `osc52` bytes; `Clipboard::new` choices; `copy` without a command writes only OSC 52.
- `input.rs`: Pointer outside the pane; drag held by saaio across the pane; drag held by the pane out of it; mode filtering; X10 and SGR encoding; modal sends all to saaio.
- `outer.rs`: `captured` for Off, Press, Motion.
- `terminal.rs`: `ESC]52;c;aGk=BEL` -> `copied() == Some("hi")` once; a query writes no reply.
- `server/tests.rs`: focused pane printing OSC 52 sends `ServerMsg::Clipboard`; a pane not in view does not.
- `app/tests/pointer.rs` (new): sidebar drag -> CopySelection with the right text from the rendered buffer; click focuses; key clears; wheel opens, scrolls and leaves history; double-click selects a word; dialog drag stays inside the dialog.

## Assumptions

- Highlight is reversed video, not the theme's selection colour.
- No "copied" notice.
- Highlight stays after the copy until the next key, click or focus change.
- OSC 52 from a pane passes on only while it is in view.
- Clipboard and primary OSC 52 writes both go to the clipboard; reads stay refused.
- Inside tmux, saaio's OSC 52 needs `set-clipboard on` (documented); locally the system command copies anyway.
- Linux: wl-copy, else xclip; a failure is logged per copy.

## Tasks

- [x] Add `base64` and `ui/clipboard.rs`: `osc52`, `Clipboard::new`, `Clipboard::copy`, with unit tests for the bytes, command choice and copy without a command
- [x] Add `ui/selection.rs`: `Selection`, `Unit`, `new`, `extend`, `is_click`, `text`, `render`, with unit tests on a hand-built Buffer (rows, trimming, clamping, wide chars, word, line, render)
- [x] Make `Mouse` ordered and add `outer::captured`; `Outer::enter` turns capture on and `mirror` writes captured modes, with tests for Off, Press and Motion panes
- [ ] Router: `Target`, `Pointer`, `PointerKind`, `held`, `encode`, `wanted`; `Routed::Pointer` replaces `Click`; tests for ownership of drags, mode filtering, X10/SGR encoding and modal routing (callers adapted, clicks still focus)
- [ ] App: `selection`, `clicks`, `surface_at`, `dialog`, `pane_modes`, `app/pointer.rs` (`on_pointer`, `on_wheel`, `on_click` moved), `Effect::CopySelection`, selection drawn and cleared; tests in `app/tests/pointer.rs`
- [ ] Env `ssh`, `wayland`, `x11` and `Env::clipboard`; `UiEnv.clipboard`; ui.rs builds `Target`, keeps `drawn`, runs `Effect::CopySelection` and logs a failed copy
- [ ] Pass a pane's OSC 52 on: `Terminal::copied`, `Host::feed -> Option<String>`, `Server::send_clipboard`, `ServerMsg::Clipboard`, protocol 7, `Effect::Copy`; tests in terminal.rs and server/tests.rs
- [ ] Rewrite docs/ui.md "Mouse": drag, double and triple click, wheel, Shift or Option for the terminal's own selection, terminal table, iTerm2 setting, tmux `set-clipboard on`
- [ ] End to end in tmux with a scratch store: drag on sidebar, rail, title, board, a dialog, history and a shell pane, checked with pbpaste and a tmux buffer; sidebar click and wheel; Claude Code pane with `SSH_CONNECTION` set passes its OSC 52 on
