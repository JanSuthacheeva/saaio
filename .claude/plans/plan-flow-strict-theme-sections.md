# Plan: each theme key in its own section

Implementation page: https://claude.ai/artifact/8NmN6qXWBHtuNfZtW3qrGd (approaches: https://claude.ai/artifact/GxTVdhocnREHzYbR7iUEcz, approach A).

## Decision

`theme_file::parse` walks `toml::de::DeTable`, which keeps each key's byte span, and checks every
color key against `PaletteKey::section()`. Every theme file error reads `path:line: what is wrong`,
e.g. `theme: ~/.config/flow/theme.toml:7: review belongs in [phase], not [state]` in the title row.
Leaf values become plain `toml::Value` again, so `apply` and `color` keep working on `Value` and the
top-level match keeps its shape: phase-color-levels' planned `("phase_colors", Value::String(name))`
arm drops in; whichever branch merges second rebases that one arm. This branch does not add it.

Decided in review: `[glyphs]` is no longer accepted. flow reads no glyph keys, so a `[glyphs]` table
is `unknown key glyphs` on its line until glyphs are designed. Tests read no file from the home
folder or dotfiles: copies of `~/.config/flow/themes/noctalia.toml` and dotfiles-v2's
`noctalia/templates/flow.toml` are fixtures.

## Units

`src/ui/theme.rs` (modified)

```rust
/// A table of colors in a theme file: [ui], [state] or [phase].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Ui,
    State,
    Phase,
}

impl Section {
    pub const ALL: [Section; 3] = [Section::Ui, Section::State, Section::Phase];
    /// The table's name in a theme file: ui, state, phase.
    pub fn name(self) -> &'static str;
    /// None for a table flow does not know.
    pub fn named(name: &str) -> Option<Section>;
}

impl PaletteKey {
    /// The table a theme file sets this key in.
    pub fn section(self) -> Section;
}
```

`section`: background, panel, selection, rule, text, strong, muted, faint, accent -> Ui; working,
waiting, blocked, done, ended -> State; design, implement, review, merge-request -> Phase.

`src/ui/theme_file.rs` (modified)

```rust
struct ThemeFile {
    path: PathBuf,
    extends: String,
    colors: Vec<Entry>, // was Vec<(String, Value)>
    nerd_font: Option<bool>,
}

/// One color a theme file sets, and the line it is on.
#[derive(Debug)]
struct Entry {
    key: PaletteKey,
    value: Value,
    line: usize,
}

/// The file's keys from the top, each checked against where it belongs; errors say path:line.
fn parse(path: &Path, text: &str) -> Result<ThemeFile>;

/// The colors of one section; a key of another section, or of none, is an error on its line.
fn colors(path: &Path, text: &str, section: Section, table: DeTable) -> Result<Vec<Entry>>;

/// A value of the file as a plain toml Value (Value::deserialize(ValueDeserializer::from(value))).
fn plain(value: Spanned<DeValue>) -> Result<Value>;

/// "path:line", the context every error of a theme file carries.
fn at(path: &Path, line: usize) -> String;

/// The 1-based line `offset` is on.
fn line(text: &str, offset: usize) -> usize;
```

- `read`: no longer adds the path as context to `parse`'s errors; an I/O error keeps it.
- `load`: no longer wraps `apply` in the path.
- `apply`: takes `Entry`s, so its unknown-key branch goes; a bad value is
  `.with_context(|| at(&file.path, entry.line))` over `name: message`; the "name each other" loop
  carries the line of the first key in it.
- `color`: unchanged. The `("glyphs", ...)` arm and its comment go.

`src/ui/theme_file/fixtures/noctalia.toml`, `src/ui/theme_file/fixtures/noctalia-template.toml`
(new): verbatim copies, read with `include_str!`.

`README.md` (modified), Themes: each key goes in its own table; a broken file names file and line in
the title row; no `[glyphs]`. `docs/HANDOVER.md` row 9: `[glyphs]` is rejected until designed.

## Call chains

1. Load: `reload_theme -> ThemeWatch::changed() -> load(dir) -> chain(dir, files) -> read(path) ->
   parse(path, text): Result<ThemeFile>`
   1. `parse -> DeTable::parse(text): Result<Spanned<DeTable>, toml::de::Error>`
   2. `parse` walks top-level keys sorted by span start (the map is alphabetical).
   3. `Section::named(key)` is Some and the value a table -> `colors(path, text, section, table)`.
   4. `colors -> PaletteKey::named(name) -> key.section() == section`, `plain(value)`,
      `line(text, key span start)` -> `Entry`.
   5. Any other key -> `match (key, plain(value)?)`: `extends`, `nerd_font`, else an error.
   6. `load -> apply(file, palette)` per file, base first -> `Theme::from_palette`.
2. Errors (each `Err(anyhow!(message)).context(at(path, line))`, rising unchanged through `read`,
   `chain`, `load`; `reload_theme` keeps the last theme and shows `theme: {error:#}` with home as `~`):
   - `[state] review = ...` (colors): `review belongs in [phase], not [state]`
   - `[ui] acent = ...` (colors): `[ui] has no key acent`
   - `accent = ...` at the top (parse): `accent belongs in [ui]`
   - `colors = 1`, `[glyphs]` (parse): `unknown key colors`, `unknown key glyphs`
   - `ui = 3` (parse): `[ui] is a table of colors`
   - `extends = 1`, `nerd_font = "yes"` (parse): `extends must be a theme's name`,
     `nerd_font is true or false`
   - TOML syntax error (parse): line from `toml::de::Error::span()`, text `message()`, one line
   - `accent = "#d9825"` (apply -> color): `accent: #d9825 is no #rrggbb color`
   - `accent = "muted"`, `muted = "accent"` (apply): `accent, muted name each other`, first's line
   - `extends = "gone"` (chain, unchanged): `.../themes/gone.toml does not exist`, no line

## Test seams

- theme.rs: each key's section (9 ui, 5 state, 4 phase); `Section::named` round-trips `name`.
- theme_file.rs `says_what_is_wrong_and_where`: wrong section, top-level color key, unknown key in a
  section, section not a table, `[glyphs]`, syntax error; each asserts `theme.toml:N:` and its text.
- theme_file.rs: the existing load tests stay green (drop `[glyphs]` from
  `overrides_a_built_in_theme`); the noctalia fixture loads over `terminal` with its colors; the
  template fixture parses; dotted (`ui.accent =`) and inline-table (`ui = { ... }`) forms load.
- End to end: `flow ui` on a scratch `FLOW_HOME` and `XDG_CONFIG_HOME` with `[state] review =
  "#fff"` shows the error in the title row; fixing the file repaints.

## Assumptions

- A color key at the top level names its section (`accent belongs in [ui]`).
- With several mistakes the topmost is reported.
- Bad values, syntax errors and the name-each-other loop get a line; "does not exist" and "more than
  4 themes deep" keep the path only.
- Dotted keys and inline tables count as their section.
- The title row keeps `theme: ` and `~` for home.

## Tasks

- [ ] Section with name, named and ALL, and PaletteKey::section, with unit tests for every key's section and the name round trip
- [ ] theme_file parse walks DeTable: Entry, colors, plain, at, line; apply takes Entry; read and load drop their path context; [glyphs] rejected; says_what_is_wrong_and_where cases with path:line, dotted and inline-table tests
- [ ] Fixtures noctalia.toml and noctalia-template.toml with tests that noctalia loads and the template parses
- [ ] README Themes and HANDOVER row 9 updated
- [ ] End to end: flow ui on a scratch FLOW_HOME and XDG_CONFIG_HOME shows a wrong-section error with file and line, and repaints once fixed
