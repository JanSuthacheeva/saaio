# flow ui themes: the design's colors, and Noctalia, live

Implementation page (review surface): `docs/design/theme-implementation.html`; approaches: `docs/design/theme-approaches.html`.

## Decision

- Approach A: `~/.config/flow/theme.toml` (the design canvas's Theme artboard) picks a base with `extends` and
  overrides keys. flow ui checks it and the file it extends once a second and repaints when either changes.
- Built-in bases: `terminal` (today's look, the default without theme.toml), `flow` (the design's dark palette),
  `flow-light` (the design's light one). A file base is `~/.config/flow/themes/<name>.toml`.
- A Noctalia user template in dotfiles-v2 writes `themes/noctalia.toml` on every theme change; `extends = "noctalia"`
  follows Noctalia live.
- Review answers: `[glyphs]` later (an additive section), default stays `terminal`.

## Units

```rust
// src/ui/theme.rs (modified)
/// The colors a theme file names, by its keys; Color::Reset is "default", the terminal's own.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    pub background: Color, pub panel: Color, pub selection: Color, pub rule: Color,
    pub text: Color, pub strong: Color, pub muted: Color, pub faint: Color, pub accent: Color,
    pub working: Color, pub waiting: Color, pub blocked: Color, pub done: Color, pub ended: Color,
}
impl Palette {
    /// "terminal", "flow", "flow-light"; None for another name.
    pub fn named(name: &str) -> Option<Palette>;
    pub fn get(&self, key: &str) -> Option<Color>;
    /// false for a key flow does not know.
    pub fn set(&mut self, key: &str, color: Color) -> bool;
}
pub struct Theme { pub ui: UiTokens, pub state: StateTokens, pub glyphs: Glyphs, pub palette: Palette }
pub struct UiTokens { /* as now */ pub background: Style }
impl Theme {
    pub fn builtin() -> Theme; // from_palette(terminal)
    /// strong and key = bold in `strong`, bar = `text` on `panel`, selection = `selection` behind.
    pub fn from_palette(palette: Palette) -> Theme;
    /// Text on a solid pill: the background, or black where the background is the terminal's.
    pub fn on_solid(&self) -> Color;
}

// src/ui/theme_file.rs (new)
pub const THEME_FILE: &str = "theme.toml";
pub const THEMES_DIR: &str = "themes";
const EXTENDS_LIMIT: usize = 4;
/// theme.toml over what it extends; terminal without theme.toml; plus every file read or that would be read.
pub fn load(dir: &Path) -> (Result<Theme>, Vec<PathBuf>);
fn apply(file: &ThemeFile, palette: &mut Palette) -> Result<()>;
/// "#rrggbb", an ANSI index 0-255, "default", or another key of the palette.
fn color(value: &toml::Value, palette: &Palette) -> Result<Color>;
pub struct ThemeWatch { dir: PathBuf, stamps: Vec<(PathBuf, Option<Stamp>)> }
impl ThemeWatch {
    pub fn new(dir: PathBuf) -> ThemeWatch;
    /// The theme again when one of its files appeared, changed or went away; always on the first call.
    pub fn changed(&mut self) -> Option<Result<Theme>>;
}

// src/ui/mod.rs
pub struct UiEnv { /* as now */ pub theme_dir: PathBuf }
pub fn run(socket: &Path, env: UiEnv) -> Result<String>; // owns the theme now
const TICKS_PER_THEME: u32 = 10;

// src/ui/app.rs
pub fn say(&mut self, notice: String);
// draw(): paints the whole frame in theme.ui.background first.

// src/ui/screen.rs
pub struct ScreenView<'a> { pub screen: &'a Screen, pub text: Color, pub background: Color }
impl Widget for ScreenView<'_>; // replaces impl Widget for &Screen

// src/ui/board.rs: solid pills use theme.on_solid() instead of Color::Black.
// src/main.rs ui(): theme_dir = config::config_file(..).parent(); ui::run(&socket, env).
```

dotfiles-v2 (`~/projects/dotfiles-v2/noctalia`): `templates/flow.toml` and in `theme.toml`:

```toml
[theme.templates.user.flow]
input_path  = '~/.config/noctalia/templates/flow.toml'
output_path = '~/.config/flow/themes/noctalia.toml'
```

```toml
# templates/flow.toml
extends = "terminal"
[ui]
background = "{{colors.surface.default.hex}}"
panel      = "{{colors.surface_container.default.hex}}"
selection  = "{{colors.surface_container_high.default.hex}}"
rule       = "{{colors.outline_variant.default.hex}}"
text       = "{{colors.on_surface.default.hex}}"
strong     = "{{colors.on_surface.default.hex}}"
muted      = "{{colors.on_surface_variant.default.hex}}"
faint      = "{{colors.outline.default.hex}}"
accent     = "{{colors.primary.default.hex}}"
```

## Data

`theme.toml` beside config.toml (with FLOW_CONFIG, beside that file): `extends` (terminal | flow | flow-light | a
`themes/` file without .toml), `[ui]` background, panel, selection, rule, text, strong, muted, faint, accent;
`[state]` working, waiting, blocked, done, ended. Values: `"#rrggbb"`, an ANSI index, `"default"`, or another key.
Unknown keys are an error.

| key | terminal | flow | flow-light |
| --- | --- | --- | --- |
| background | default | #141418 | #f6f5f1 |
| panel | 235 | #1f1f26 | #ecebe5 |
| selection | 236 | #26262e | #e2e0d8 |
| rule | 8 | #34343d | #cfcdc4 |
| text | default | #c8c8d0 | #2b2b30 |
| strong | default | #f0f0f4 | #111114 |
| muted | 7 | #9a9aa6 | #5d5d66 |
| faint | 8 | #6c6c78 | #73737c |
| accent | 173 | #e07a52 | #b8532f |
| working | 4 | #7aa7ff | #2f5fd0 |
| waiting | 3 | #e5b45a | #9a6a00 |
| blocked | 1 | #f0717f | #c2283f |
| done | 2 | #93c46d | #3d7a22 |
| ended | faint | faint | faint |

## Call chains

1. Start: `main::ui -> config::config_file(..)`, parent = `UiEnv.theme_dir` -> `ui::run(socket, env) ->
   ThemeWatch::new(dir) -> ThemeWatch::changed(): Some(result) -> theme_file::load(dir)`: no theme.toml ->
   `Theme::builtin()`; else read it, follow `extends` (`Palette::named` or `themes/<name>.toml`, at most 4 deep),
   `apply` from the base up, `Theme::from_palette`.
   - Err: keep `Theme::builtin()`, `App::say("theme.toml: <error>")`.
2. Theme file changes: tick every 10 -> `ThemeWatch::changed()` (a stamp differs) -> `load` -> `Some(Ok(theme))` ->
   replace, redraw: `App::draw(frame, &theme, peek)` paints the background; `ScreenView` draws default cells in
   `text` on `background`.
   - Err (half-written, broken): keep the old theme, `App::say` names the file and error.

## Test seams

- Palette: named palettes match the table; get/set reject unknown keys.
- theme_file::load (temp dir): none is terminal; built-in and themes/ bases; overrides; key references; bad hex,
  unknown key, unknown base, extends loop each an error naming the file.
- ThemeWatch: first call loads; unchanged None; edit of the extended file, creating and deleting theme.toml Some.
- ScreenView, App::draw: TestBackend with the flow palette paints empty cells in background, default cells in text.

## Assumptions

1. Without theme.toml flow looks exactly as today.
2. A broken theme keeps the last good one and says why.
3. `background` paints every cell flow ui draws, the pane's default-colored cells too; colors Claude sets stay.
4. The Noctalia template keeps state colors ANSI (terminal-sequences sets them).
5. Only flow ui reads the theme.
6. Once a second is quick enough.
7. On the Mac (no Noctalia) use `extends = "flow"`.

## Tasks

- [ ] theme.rs: Palette (named terminal/flow/flow-light, get, set), Theme::from_palette, UiTokens.background, on_solid; builtin via from_palette; tests that terminal equals today's tokens
- [ ] theme_file.rs: load with extends, apply, color; tests for bases, overrides, references and each error
- [ ] ThemeWatch::new/changed with tests (first load, unchanged, edit, create, delete)
- [ ] screen::ScreenView and App::draw painting the background; board pills via on_solid; tests with the flow palette
- [ ] ui::run owns the theme (UiEnv.theme_dir, TICKS_PER_THEME, App::say on errors); main::ui passes the config dir
- [ ] Docs: README (themes), CLAUDE.md module map, HANDOVER
- [ ] dotfiles-v2: Noctalia template templates/flow.toml and its [theme.templates.user.flow] entry
- [ ] End-to-end check: flow ui in a private tmux server; theme.toml extends flow, then flow-light, then a themes/ file edited while running repaints within a second; a broken file shows the notice and keeps the theme
