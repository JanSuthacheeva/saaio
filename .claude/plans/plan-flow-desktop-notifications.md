# Desktop notifications

Review pages: approaches https://claude.ai/artifact/LxLeDqYzh7Qq4dxU6m5VrF (`docs/design/desktop-notifications-approaches.html`),
implementation https://claude.ai/artifact/ALahCYcds96LWTUx1qCBQd (`docs/design/desktop-notifications-implementation.html`).

## Decision

- Approach A, focus-aware: saaio ui notifies when a flow comes to Waiting or Blocked while it is
  not the pane in view, or while the real terminal has lost focus (then also the flow in view).
- The App finds the transition in the board diff it already makes and returns `Effect::Notify`;
  `ui::run` hands it to `Notifier` (`ui/notify.rs`), which writes exactly one sequence chosen once
  from the environment: OSC 9 by default, OSC 777 for foot, rxvt and VTE, a plain BEL for
  Alacritty and Terminal.app. Inside tmux an OSC goes through tmux's DCS passthrough
  (`allow-passthrough on`); BEL is never wrapped.
- saaio keeps focus reporting (1004) on itself. The Router turns `ESC [ I` / `ESC [ O` into
  `Routed::Focus`, and still passes the bytes, unchanged, to a pane that asked for 1004 when no
  dialog is open, as today.
- `[ui] notify = true` by default.
- The master notifies when it comes to Waiting or Blocked out of sight, never on Idle.
- No proto, store or `--json` change.

## Units

### src/ui/notify.rs (new)

```rust
//! `Notifier`: desktop notifications through the terminal, by OSC 9, OSC 777 or its bell

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sequence {
    /// ESC ] 9 ; saaio: text BEL - iTerm2, Ghostty, WezTerm, kitty, and the default.
    Osc9,
    /// ESC ] 777 ; notify ; saaio ; text BEL - foot, urxvt, VTE.
    Osc777,
    /// BEL alone - Alacritty, Terminal.app.
    Bell,
}

impl Sequence {
    /// Bell for Alacritty ($ALACRITTY_WINDOW_ID, TERM alacritty*) and Terminal.app
    /// (TERM_PROGRAM Apple_Terminal); Osc777 for TERM foot* / rxvt* or $VTE_VERSION; else Osc9.
    pub fn pick(term: Option<&str>, program: Option<&str>, vte: bool, alacritty: bool) -> Sequence;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notifier {
    sequence: Sequence,
    tmux: bool,
}

impl Notifier {
    pub fn new(sequence: Sequence, tmux: bool) -> Notifier;
    /// Writes the notification for `text` (control characters dropped, cut to 200 chars) and
    /// flushes; Err when the terminal cannot be written.
    pub fn notify(&self, out: &mut impl Write, text: &str) -> Result<()>;
}

fn sequence(sequence: Sequence, text: &str) -> String;
/// ESC P tmux; <sequence, every ESC doubled> ESC \
fn passthrough(sequence: &str) -> String;
```

### src/ui/app/notifications.rs (new)

```rust
impl App {
    pub fn set_notify(&mut self, on: bool);
    pub(super) fn worker_notifications(&self, old: &Board, new: &Board) -> Vec<Effect>;
    pub(super) fn master_notification(&self, was: Option<State>, state: Option<State>) -> Vec<Effect>;
    /// The terminal has lost focus, or `key` is not the pane in view.
    fn out_of_sight(&self, key: &PaneKey) -> bool;
}

/// Waiting or Blocked now, and not the same a moment ago.
fn calls_for_you(was: Option<State>, now: State) -> bool;
/// "shop-230 asks: <first question>", "shop-230 waits for you",
/// "shop-230 is blocked: <blocker>", "shop-230 is blocked".
fn worker_text(flow: &FlowView) -> String;
/// "master waits for you", "master is blocked".
fn master_text(state: State) -> String;
```

### src/ui/app.rs (modified)

- `mod notifications;`
- `App` gains `notify: bool` (true) and `terminal_focused: bool` (true until the terminal says
  otherwise).

### src/ui/app/messages.rs (modified)

- `on_server(ServerMsg::Board)`: with an old board, also returns
  `self.worker_notifications(old, &board)`.
- `fn on_master_turn(&mut self, turn: Option<MasterTurn>) -> Vec<Effect>`: returns
  `master_notification(was, state)`; `ServerMsg::Master` returns it.

### src/ui/app/keys.rs (modified)

- `on_input(Routed::Focus(focused))`: first, before the notice and selection are cleared; sets
  `terminal_focused`, returns `vec![]`.

### src/ui/app/effect.rs (modified)

```rust
/// Raise a desktop notification that says this.
Notify(String),
```

### src/ui/input.rs (modified)

```rust
pub enum Routed { /* ... */ Focus(bool) }
pub struct Target { /* ... */ pub focus: bool }
/// ESC [ I is Some(true), ESC [ O Some(false), anything else None.
fn focus_event(bytes: &[u8]) -> Option<bool>;
```

`route` checks `focus_event` first in its loop, before the prefix handling (a prefix stays
pending across it). The three bytes go to `plain` only when `target.focus && !target.modal`;
then `flush` and push `Routed::Focus`.

### src/ui/outer.rs (modified)

`captured(modes)` also sets `focus: true`.

### src/ui.rs (modified)

```rust
pub struct UiEnv { /* ... */ pub notifier: Notifier }
/// Raises a desktop notification; a failure goes to server.log.
fn notify(notifier: &Notifier, log_file: &Path, text: &str, clock: fn() -> u64);
```

`Target { focus: modes.focus, .. }`; `Effect::Notify(text) => notify(...)`; `reload_config`
calls `app.set_notify(config.ui.notify)`. `pub mod notify;` beside `clipboard`.

### src/cli/env.rs, src/main.rs, src/cli/commands/ui.rs (modified)

```rust
pub struct Env {
    /* ... */
    pub term: Option<String>,        // $TERM
    pub term_program: Option<String>, // $TERM_PROGRAM
    pub vte: bool,                   // $VTE_VERSION set
    pub alacritty: bool,             // $ALACRITTY_WINDOW_ID set
    pub tmux: bool,                  // $TMUX set
}
impl Env {
    pub(crate) fn notifier(&self) -> Notifier;
}
```

`main.rs::env()` reads them; `commands/ui.rs` passes `notifier: env.notifier()`.

### src/config.rs, examples/config.toml (modified)

`UiConfig.notify: bool`, true unless set. Example under `#[ui]`:

```toml
# A flow that starts waiting or is blocked while you aren't looking at it raises a desktop
# notification through your terminal. false turns it off.
#notify = true
```

## Call chains

### A worker comes to Waiting or Blocked

1. `read_stdin`/`read_server -> Event::Server(ServerMsg::Board(board))`
2. `ui::run -> App::on_server(message): Vec<Effect>`
3. `App::on_server -> App::worker_notifications(old, &board): Vec<Effect>` - only with an old
   board; nothing when `!self.notify`; for each flow in both boards with
   `calls_for_you(Some(was.state), flow.state)` and `out_of_sight(&PaneKey::Flow(id))`:
   `Effect::Notify(worker_text(flow))`.
4. `ui::run -> ui::notify(&env.notifier, &env.log, &text, clock)`
5. `ui::notify -> Notifier::notify(&mut io::stdout(), text): Result<()>`
6. Error: a failed write logs `ui: notify failed: ...` to server.log; nothing on screen.
   A terminal that ignores the sequence, or tmux without passthrough, drops it silently.

### The terminal gains or loses focus

1. `read_stdin -> Event::Input(bytes)`
2. `ui::run -> Router::route(&bytes, Target { focus: modes.focus, .. }): Vec<Routed>`
3. `Router::route -> focus_event(rest): Option<bool>`; bytes to the pane only when
   `target.focus && !target.modal`; then `Routed::Focus(focused)`.
4. `ui::run -> App::on_input(Routed::Focus(focused)): vec![]` sets `terminal_focused`.

### The master comes to Waiting or Blocked

1. `App::on_server(ServerMsg::Master { turn, .. }) -> App::on_master_turn(turn): Vec<Effect>`
2. `App::on_master_turn -> App::master_notification(was, state): Vec<Effect>` - when `notify`,
   `calls_for_you` and `out_of_sight(&PaneKey::Master)`, and not on the first `ServerMsg::Master`
   after attach (`master.cwd` still unset), which only says where the master stood: no
   notification on startup, as for the first board. (Added in the build.)
3. Then steps 4-5 above.

### config.toml changes

1. `ui::run (tick) -> reload_config -> ConfigWatch::changed(): Some(Ok(config))`
2. `reload_config -> App::set_notify(config.ui.notify)`; a broken file keeps the last setting.

## Test seams

- notify.rs: `Sequence::pick` per environment; exact bytes of each sequence; tmux doubles every
  ESC and leaves BEL alone; control characters dropped; long text cut; writes to a `Vec<u8>`.
- input.rs: focus in/out -> `Routed::Focus`; bytes forwarded unchanged to a pane with 1004, not
  to one without, not under a dialog; a focus event after the prefix keeps the prefix pending.
- app/tests/notifications.rs (new subject): first board silent; Idle -> Waiting out of view
  notifies once; same board twice once; in view and focused silent; in view and unfocused
  notifies; Waiting -> Blocked notifies again; off silent; master Waiting out of view notifies,
  Idle does not; a focus event keeps the notice and selection.
- outer.rs: `captured` keeps focus on.
- config: the example tests cover `notify` once its line exists.

## Assumptions

- Text: "shop-230 asks: <first question>", "shop-230 waits for you", "shop-230 is blocked:
  <blocker>", cut to 200 chars; OSC 9 prefixes "saaio: ", OSC 777 has the title "saaio".
- "In view" is the focused pane in saaio, even under the board or a dialog.
- Waiting -> Blocked or back is a new transition and notifies again.
- A terminal that never reports focus counts as focused.
- Only this UI's own board notifies, not other projects' flows.
- No rate limit.
- BEL only for recognised terminals (Alacritty, Terminal.app); unknown ones get OSC 9.
- Alacritty facts for the docs (checked in its source): it sets `ALACRITTY_WINDOW_ID` and
  `TERM=alacritty`; on a bell while unfocused it requests attention (urgency hints on by default:
  macOS dock bounce, X11/Wayland urgency); `[bell] command` runs a program on a bell, with a
  cooldown.

## Tasks

- [x] Add `ui/notify.rs`: `Sequence::pick`, `Notifier::new`, `Notifier::notify`, `sequence`, `passthrough`, with unit tests for picking, each sequence's bytes, tmux wrapping, BEL unwrapped, control characters and cutting
- [x] Add `Env` fields `term`, `term_program`, `vte`, `alacritty`, `tmux`, `Env::notifier`, read them in `main.rs::env()`, and pass `UiEnv.notifier` from `commands/ui.rs`
- [x] Add `[ui] notify` to `UiConfig` and its commented line to `examples/config.toml`, with the config tests passing
- [x] Add `Routed::Focus`, `Target.focus` and `focus_event` to the Router, with tests: focus to `Routed::Focus`, forwarded unchanged only to a 1004 pane outside a dialog, prefix kept pending
- [x] Keep focus reporting on in `Outer` (`captured` sets `focus: true`), with its test updated
- [x] Add `Effect::Notify`, `App.notify`, `App.terminal_focused`, `on_input(Routed::Focus)` and `app/notifications.rs` (`set_notify`, `worker_notifications`, `master_notification`, `out_of_sight`, `calls_for_you`, `worker_text`, `master_text`), wired into `on_server(Board)` and `on_master_turn`, with `app/tests/notifications.rs`
- [x] Wire `ui::run`: `Target.focus`, `Effect::Notify` -> `ui::notify`, `reload_config` -> `set_notify`
- [ ] Docs: `docs/ui.md` Notifications section (terminal table, tmux `allow-passthrough`, Alacritty `[bell] command` example, Terminal.app), `docs/configuration.md` `notify` rows, `docs/cli.md` environment rows (TERM, TERM_PROGRAM, VTE_VERSION, ALACRITTY_WINDOW_ID, TMUX), README not-yet line, ROADMAP row dropped, CHANGELOG Unreleased Added, CLAUDE.md (`notify.rs`, `app/notifications.rs`, plan list)
- [ ] End-to-end: scratch store, UI in a PTY with captured output; report a flow waiting, then blocked, then focus out on the flow in view; assert one sequence per transition, none at startup, none with `notify = false`; then the same in a real terminal window to see the notification
