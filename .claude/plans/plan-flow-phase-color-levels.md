# Plan: phase colors as a theme setting

Implementation page: https://claude.ai/artifact/9dkPyELqg5KoLch61tvEev (approaches: https://claude.ai/artifact/NhJ4mKUKJhLbGb2sdjNb8T, approach A, option a).

## Decision

theme.toml gets a top-level `phase_colors = "off" | "steps" | "full"`, default `steps`:

- `off`: state colors only, as before PR #20.
- `steps`: only the current step dot (and the detail view's current step) takes the current
  phase's color; spinner, label, rail and title keep the state color.
- `full`: the spinner, label, rail and title take it too.

Done dots and the joins between them are plain text at every level (decided with the tui polish
merge, `plan-flow-tui-polish.md`): only the current step stands out.

As today, dots take the phase color only while the flow is Working or Delegated in a phase. The
nearest file that says wins, like `nerd_font`.

## Units

`src/ui/theme.rs` (modified)

```rust
/// How much of a working flow takes its phase's color: theme.toml's phase_colors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PhaseColors {
    Off,
    #[default]
    Steps,
    Full,
}

impl PhaseColors {
    /// "off", "steps" or "full"; None for another name.
    pub fn named(name: &str) -> Option<PhaseColors>;
}

pub struct Theme {
    // ... existing fields
    pub phase_colors: PhaseColors, // from_palette: PhaseColors::default()
}

impl Theme {
    /// The phase's color at Full while the flow works in one, else its state's.
    pub fn flow_style(&self, flow: &FlowView) -> Style;
    /// The phase's color at Steps or Full while the flow works in one, else its state's.
    pub fn step_style(&self, flow: &FlowView) -> Style;
    /// The phase's color when `colored` and the flow works in one, else its state's.
    fn tinted(&self, flow: &FlowView, colored: bool) -> Style;
}
```

`src/ui/theme_file.rs` (modified)

```rust
struct ThemeFile {
    // ... existing fields
    phase_colors: Option<PhaseColors>,
}
```

- `parse`: `("phase_colors", Value::String(name))` via `PhaseColors::named`; anything else bails
  `phase_colors is off, steps or full`.
- `load`: `theme.phase_colors` = the nearest file's value, else `PhaseColors::default()`.

Call sites (modified): `sidebar/labels.rs` `Sidebar::steps` and the current step in
`board/detail.rs` `steps` use `step_style`; the other `flow_style` callers stay.

`README.md` (modified): `phase_colors` in the Themes example and keys.

## Call chains

1. Load: `ThemeWatch::changed -> theme_file::load -> chain -> read -> parse: ThemeFile` ->
   `Theme::from_palette` -> `theme.phase_colors = nearest Some or default`.
   - Bad value -> `parse` bails with the path as context -> the UI keeps its theme and shows the
     notice, as for any theme error.
2. Draw: `Sidebar::steps -> Theme::step_style -> tinted(flow, phase_colors != Off)`;
   rows, rail, chrome, cards -> `Theme::flow_style -> tinted(flow, phase_colors == Full)`.
   `tinted`: Working or Delegated with `Some(phase)` and `colored` -> `phase_style`, else
   `state_style`.

## Test seams

- theme.rs: flow_style and step_style at Off, Steps and Full; default Steps; waiting keeps its state
  color at every level.
- theme_file.rs: phase_colors read; nearest file wins; a bad value names the allowed ones.
- app/tests/sidebar.rs: export-csv (delegated, review) spinner and dots per level.
- app/tests/board.rs: the card's spinner and label per level.
- Goldens are text only; regenerate to confirm no change.

## Build notes

- The call sites moved into the first task: the new default (Steps) changes what flow_style draws,
  so the theme change and its callers only pass together. PhaseColors::named moved to the
  theme_file task, its only caller.

## Assumptions

- The detail view's current step is a step dot; its done steps stay green with a check.
- Joins between reached dots are plain text, like the done dots.
- `full` is exactly today's look.

## Tasks

- [x] PhaseColors, Theme.phase_colors, step_style and flow_style through tinted, with unit tests per level; Sidebar::steps and the detail view's current step use step_style; sidebar and board tests per level; goldens regenerated unchanged
- [x] PhaseColors::named; theme_file reads phase_colors (nearest wins, bad value errors), with tests
- [x] README Themes documents phase_colors
- [x] End to end: flow ui on a scratch FLOW_HOME with phase_colors off, steps and full

## End to end

flow ui on a scratch FLOW_HOME, XDG_STATE_HOME and XDG_CONFIG_HOME, a worker `colors` (agent `sh`)
working in review, theme.toml `extends = "flow"` with each level, in a private tmux at 150x44;
the sidebar's colors read from `capture-pane -e`:

```
off    spinner=working  dots=●:working ●:working ●:working ○:faint
steps  spinner=working  dots=●:review ●:review ●:review ○:faint
full   spinner=review   dots=●:review ●:review ●:review ○:faint
```

After the tui polish merge the done dots read text at every level, the current one as above:

```
off    spinner=working  dots=●:text ●:text ●:working ○:faint
steps  spinner=working  dots=●:text ●:text ●:review ○:faint
full   spinner=review   dots=●:text ●:text ●:review ○:faint
```

## Review

- **Fixed** - the detail view's current step moving to `step_style` had no test: reverting it to
  `flow_style` left the suite green. Added
  `board::detail::tests::the_current_step_takes_the_color_of_its_phase_by_default`, proven to fail
  on the revert (45d9e61). Test only, so no second review round was needed.
- **Open** - none. Note for the merge request: the default changes the look for everyone, since
  `steps` replaces today's full coloring; a user who wants the old look sets `phase_colors = "full"`.
- **Where to look** - `src/ui/theme.rs` `flow_style` / `step_style` / `tinted`: which level colors
  what; then `src/ui/theme_file.rs` `parse` and `load`: the bad-value error and the nearest file
  winning. The call-site swaps in `sidebar/labels.rs` and `board/detail.rs` and the test and README
  changes are routine.
- **Checks** - `cargo test`, `cargo clippy --all-targets -- -D warnings` and `cargo fmt --check`
  pass at 45d9e61.
