# Plan: a configurable prefix key

Implementation page: https://claude.ai/artifact/6VotXfeYLsM8TcEKymVVsC (approaches: https://claude.ai/artifact/48yV7UFWxBdiNANUQCfejh, approach A).

## Decision

`[ui] prefix` in config.toml names the key before every flow ui command, `C-space` unless set. It
deserialises into a `Prefix` at config load, so an unknown name or a key a pane needs fails like any
other config error, naming the file and the key. `App` owns the `Prefix` and passes it explicitly to
everything that writes a key (Sidebar field, dialog arguments, title row, master card); `Router` is
built from it; `Hint::key` becomes a `Cow`. Not on `Theme`, not a global.

Decided in review: the names are exactly `C-space` and `C-a`..`C-z` (no C-], Alt or function keys);
C-a and C-b are allowed.

## Units

`src/ui/prefix.rs` (new) - the key that leads every flow ui command: its name and the bytes a
terminal sends for it.

```rust
/// C-space or C-a..C-z, as the byte a terminal sends (C-space is NUL, C-b is 0x02).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub struct Prefix(u8);

impl Default for Prefix { fn default() -> Prefix; }   // C-space

impl TryFrom<String> for Prefix {
    type Error = String;
    /// "C-space" or "C-a".."C-z"; Err names what is accepted, or why a pane needs the key.
    fn try_from(name: String) -> Result<Prefix, String>;
}

impl Prefix {
    /// "C-space", "C-b".
    pub fn name(self) -> String;
    /// The name and the key after it: "C-b r".
    pub fn then(self, key: &str) -> String;
    /// The byte a terminal without the kitty keyboard protocol sends.
    pub fn byte(self) -> u8;
    /// The kitty keyboard protocol's form: CSI 32;5u for C-space, CSI 98;5u for C-b.
    pub fn kitty(self) -> Vec<u8>;
}

/// The keys a pane needs, each with what it does there: C-c, C-d, C-z, C-s, C-q, C-h, C-i, C-j, C-m.
const NEEDED: [(char, &str); 9];
```

`src/config.rs` (modified) - the `[ui]` section, strict like the others.

```rust
pub struct Config { ..., pub ui: UiConfig }

/// [ui]: how flow ui takes keys.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct UiConfig {
    /// The key before every flow ui command, C-space unless set.
    pub prefix: Prefix,
}
```

`src/ui.rs`, `src/cli/commands/ui.rs` (modified)

```rust
pub struct UiEnv { ..., /// The key before every flow ui command.
    pub prefix: Prefix }
// run: App::new(..).with_prefix(env.prefix); Router::new(env.prefix)
// commands/ui.rs: prefix: config.ui.prefix
```

`src/ui/input.rs` (modified) - `PREFIX` and `KITTY_PREFIX` go; the Router holds both forms.
`Default` stays (C-space) for tests.

```rust
#[derive(Debug, Default)]
pub struct Router { prefix: u8, kitty: Vec<u8>, prefixed: bool, pending: Vec<u8> }

impl Router {
    pub fn new(prefix: Prefix) -> Router;
    pub fn route(&mut self, bytes: &[u8], modal: bool, pane: Rect) -> Vec<Routed>; // unchanged
    /// Was a free fn: the command after the prefix; the prefix again is Literal.
    fn command_after_prefix(&self, rest: &[u8]) -> (AfterPrefix, usize);
}
```

A NUL with another prefix is a plain byte for the pane; the literal pushes `self.prefix`.

`src/ui/app.rs` (modified)

```rust
pub struct App { ..., prefix: Prefix }
/// The key before every command, for the hints that name it.
pub fn with_prefix(mut self, prefix: Prefix) -> App;
// sidebar(..): prefix: self.prefix
// draw.rs: dialog::help(self.prefix, theme), not_running(status, worker, self.prefix, theme),
//          no_session(self.prefix, theme)
```

`src/ui/hints.rs` (modified)

```rust
pub struct Hint { pub key: Cow<'static, str>, ... }
pub fn new(key: impl Into<Cow<'static, str>>, label: impl Into<Cow<'static, str>>) -> Hint;
```

`src/ui/dialog.rs` (modified)

```rust
/// Every key behind the prefix.
pub fn help(prefix: Prefix, theme: &Theme) -> Dialog<'_>;
pub fn not_running<'a>(status: &PaneStatus, worker: Option<&str>, prefix: Prefix, theme: &'a Theme)
    -> Option<Dialog<'a>>;
pub fn no_session(prefix: Prefix, theme: &Theme) -> Dialog<'_>;
```

help: "{name} then", and the row "{name}  send {name} itself"; the key column stays 9 wide, since
the longest name, C-space, is 7.

`src/ui/sidebar.rs`, `sidebar/rows.rs`, `app/chrome.rs`, `board/cards.rs` (modified)

```rust
pub struct Sidebar<'a> { ..., pub prefix: Prefix, pub theme: &'a Theme }
// empty_rows: prefix.then("n"); footer: prefix.name() then "?"; rows.rs peek: prefix.then("enter")
// chrome.rs draw_title: self.prefix.then("b")
// cards.rs master_card: format!("exited - {} resumes it", self.sidebar.prefix.then("r"))
```

`src/cli/commands/status.rs`, `cli/output/status.rs` (modified)

```rust
pub(crate) fn detail(flow: &FlowView, now: u64, full: bool, worktree_exists: Option<bool>,
    prefix: Prefix) -> Document;
fn detail_help(flow: &FlowView, worktree_exists: Option<bool>, prefix: Prefix) -> Vec<String>;
// commands/status.rs: run(args, store, registry, env: &Env, now) - it takes the Env now; only
// the detail path reads env.config().map(|config| config.ui.prefix).unwrap_or_default()
```

No code: README.md, docs/HANDOVER.md, src/server/master.md (current text kept as master-v7.md,
added to `PREVIOUS_DEFAULTS`).

## Data

- `Prefix(u8)`: 0x00 or 0x01..0x1a. config.toml -> Config -> UiEnv -> Router, App, Sidebar, dialogs;
  Config -> flow status.
- `UiConfig { prefix: Prefix }`: config.toml `[ui]` -> Config.
- `Hint.key`: `&'static str` -> `Cow<'static, str>`.

```toml
[ui]
prefix = "C-b"
```

## Call chains

flow ui starts:

1. main -> commands::ui::run(args, store, env): Document
2. commands::ui::run -> Env::config(): Result<Config>
3. Config::parse -> toml (serde) -> Prefix::try_from(String): Result<Prefix, String>
4. commands::ui::run -> ui::run(socket, UiEnv { prefix: config.ui.prefix, .. }, clock)
5. ui::run -> App::new(size).with_prefix(env.prefix); Router::new(env.prefix)

Error: try_from returns Err; toml wraps it with line and key; `Config::load` adds "<file> is not
valid"; flow ui exits 1 before the server starts or the terminal is touched. Messages:
`C-c interrupts the agent in a pane; pick another prefix, such as C-space or C-b`;
`unknown prefix "M-x": use C-space or C-a to C-z`. A typo in the section is serde's unknown-field
error. flow server fails the same way.

A key arrives:

1. ui::run -> Router::route(bytes, modal, pane): Vec<Routed>
2. Router::route: byte == self.prefix or starts_with(self.kitty) -> prefixed
3. Router::route -> self.command_after_prefix(rest): (AfterPrefix, usize)
4. prefix twice -> AfterPrefix::Literal -> Routed::Pane(vec![self.prefix])
5. ui::run -> App::on_input(routed): Vec<Effect> (unchanged)

A frame draws a hint:

1. App::draw -> App::sidebar(theme, peek): Sidebar { prefix: self.prefix, .. }
2. Sidebar::footer -> prefix.name() -> Span::styled(.., theme.ui.key)
3. App::draw_view -> dialog::help(self.prefix, theme): Dialog
4. dialog::not_running -> Hint::new(prefix.then("r"), "resumes it")
5. hints::line(&hints, width, theme): Line (unchanged signature)

flow status <ended flow>:

1. commands::status::run -> Env::config() -> .map(ui.prefix).unwrap_or_default()
2. -> output::status::detail(&one, now, full, worktree_exists, prefix): Document
3. detail_help -> "C-b r in flow ui resumes it"

An unreadable config does not fail flow status: the hint names C-space.

## Test seams

- prefix.rs unit: every accepted name round-trips with its byte and kitty form; each needed key
  refused with its reason; unknown names, "C-A", "ctrl-a" refused.
- config.rs unit: `[ui] prefix = "C-b"` loads; default C-space; `[ui] other = 1` and
  `prefix = "C-c"` fail naming the file.
- input.rs unit: a C-b router turns `0x02 d` and `CSI 98;5u d` into Detach, sends 0x02 for C-b
  twice, passes NUL through.
- app/tests: an App with C-b shows `C-b ? keys`, `C-b n starts a flow`, help `C-b then`, title row
  `C-b b sidebar`, master card and not-running dialog `C-b r`. Goldens unchanged (default).
- tests/cli.rs e2e: `flow ui` with FLOW_CONFIG holding `prefix = "C-c"` exits 1 with the reason;
  `flow status` of an ended flow names the configured key.

## Assumptions

- Names exactly `C-space` and `C-a`..`C-z`, lowercase.
- Refused: C-c (interrupt), C-d (end of input), C-z (suspend), C-s / C-q (flow control),
  C-h / C-i / C-j / C-m (the same bytes as backspace, tab, newline, enter).
- Allowed although some agent binds them: C-a, C-b, C-r, C-o, C-t, C-g and the rest.
- A bad prefix fails every command that loads config strictly; flow status falls back to C-space.
- The prefix applies when flow ui starts; no live reload.
- master.md: "Do not name a `C-space` digit" -> "Do not name a flow ui key or digit".
- README names the prefix once ("`C-space` unless `[ui] prefix` sets another"), writes keys as
  "prefix d" after, and the config example gains a `[ui]` block listing the refused keys.
- `Prefix` lives in `ui/prefix.rs`; config.rs imports it.

## Tasks

Regrouped during the build into vertical slices: a `Prefix` nobody reads fails the dead-code lint,
so each task brings a piece of it together with its first reader.

- [x] The configured key routes input: `Prefix` (`try_from`, `byte`, `kitty`, `NEEDED`) with unit tests for every accepted name, each refused key and unknown spellings; `UiConfig` / `Config.ui` with config tests (loads C-b, defaults to C-space, refuses an unknown `[ui]` key and `prefix = "C-c"` naming the file); `Router::new` / `command_after_prefix(&self)` with input tests for a C-b router (plain byte, kitty form, twice sends one, NUL passes through); `UiEnv.prefix` from `commands::ui`
- [x] Make `Hint::key` a `Cow<'static, str>`
- [x] Every hint names the configured key: `Prefix::name` / `then` with unit tests, `App::with_prefix`, `Sidebar.prefix`, `dialog::help` / `not_running` / `no_session`, chrome title row, master card, peek hint; app tests with a C-b App for each place; goldens unchanged
- [x] Name the configured key in `flow status`'s resume hint (`detail`, `detail_help`) with a cli test
- [ ] E2E cli test: `flow ui` with `prefix = "C-c"` exits 1 with the reason
- [ ] Reword master.md (current text to master-v7.md in `PREVIOUS_DEFAULTS`), README keys section and `[ui]` example, HANDOVER lines
- [ ] End-to-end check: flow ui on a scratch FLOW_HOME with `[ui] prefix = "C-b"`: C-b ? shows the help naming C-b, C-b d detaches, C-b C-b reaches the pane, the sidebar footer reads C-b; then `prefix = "C-c"` refuses to start
