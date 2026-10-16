# Plan: a working flow takes the color of its phase

Approaches page: https://claude.ai/artifact/Sxdac5sRxdaWmihFDTzcq5 (approach A chosen, colors as proposed).

## Decision

While a flow works (State::Working or State::Delegated) and has a phase, every element that today
takes its state color takes its phase's color instead: the spinner, the reached step dots and joins,
the card label, the rail, the title row, the detail view's current step. Waiting, idle, blocked, not
responding, done and ended keep their state colors. A working flow without a phase yet keeps
`working`. The timeline's Tone::Progress stays `working`.

Theme files get a `[phase]` section: design, implement, review, merge-request. The terminal theme
uses 256-color indexes, since terminals repaint ANSI blue, magenta and cyan.

| key | terminal | flow | flow-light |
| --- | --- | --- | --- |
| design | 141 | #b392f0 | #7a4fc9 |
| implement | 75 | #7aa7ff | #2f5fd0 |
| review | 80 | #56c2c2 | #13807f |
| merge-request | 176 | #e07fd0 | #b0399c |

## Units

`src/ui/theme.rs` (modified)

```rust
pub struct Palette {
    // ... existing fields
    pub design: Color,
    pub implement: Color,
    pub review: Color,
    pub merge_request: Color,
}

pub enum PaletteKey {
    // ... existing variants
    Design,
    Implement,
    Review,
    MergeRequest,
}

impl PaletteKey {
    pub const ALL: [PaletteKey; 18] = [ /* ... */ ];
    // name(): "design", "implement", "review", "merge-request"
}

pub struct Theme {
    pub ui: UiTokens,
    pub state: StateTokens,
    pub phase: PhaseTokens,
    pub glyphs: Glyphs,
    pub palette: Palette,
}

#[derive(Debug, Clone)]
pub struct PhaseTokens {
    pub design: Style,
    pub implement: Style,
    pub review: Style,
    pub merge_request: Style,
}

impl Theme {
    pub fn phase_style(&self, phase: Phase) -> Style;
    /// The phase's color while the flow works in one, else its state's.
    pub fn flow_style(&self, flow: &FlowView) -> Style;
}
```

`src/ui/theme_file.rs` (modified): `parse` accepts `"ui" | "state" | "phase"` sections.

Call sites (modified), `state_style(flow.state)` -> `flow_style(flow)` wherever a flow is at hand:
`sidebar/labels.rs` (steps), `sidebar/rows.rs` (row tone, peek spinner), `rail.rs`, `app/draw.rs`,
`app/chrome.rs`, `board/cards.rs` (card tone, state label), `board/detail.rs` (glyph, current step).

`README.md` (modified): the `[phase]` keys.

## Call chain

1. Draw: `Sidebar::steps(flow)` / row / card -> `Theme::flow_style(flow)`:
   - `flow.state` is Working or Delegated and `flow.phase` is `Some(phase)` -> `phase_style(phase)`
   - otherwise -> `state_style(flow.state)`
2. Load: `theme_file::parse` collects `[phase]` with `[ui]` and `[state]`; `apply` sets them by
   `PaletteKey::named`; an unknown key stays an error naming the file.

## Test seams

- theme.rs: flow_style for working and delegated in a phase, working without a phase, waiting in a
  phase; the built-in palettes' phase colors.
- theme_file.rs: a `[phase]` section with `merge-request` overrides the base.
- app/tests/sidebar.rs: the delegated export-csv (review) draws spinner and steps in review's color.

## Assumptions

- Idle keeps `muted` even with a phase.
- `merge-request` is the key, as the CLI names the phase.
- The detail view's done steps keep `done` green with ✓.

## Tasks

- [x] Palette, PaletteKey and PhaseTokens get the four phase colors with the built-in values; Theme::phase_style and Theme::flow_style, with unit tests
- [x] theme_file reads a [phase] section, with a test that merge-request overrides the base
- [x] Every flow-colored call site uses flow_style, with a sidebar test for the delegated flow in review
- [ ] README theme keys name [phase]
- [ ] End to end: flow ui with the noctalia theme draws working flows in their phase colors
