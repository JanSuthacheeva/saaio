# Plan: flow ui polish - one focus look, one dialog frame, fitted hints

Implementation page: https://claude.ai/artifact/2bJds6izUgSyp66skUhDFt (copy: `docs/design/polish-implementation.html`).
Findings: https://claude.ai/artifact/CjhZ1YTchL5zyPfhTDE3mj. Approaches: https://claude.ai/artifact/ErKSyoguLZJq5TmPrE6osa (A with 8a chosen).

## Decision

Build all nine findings of the design review. A new `ui/focus.rs` draws the one focus look:
`theme.ui.selection` over the area and `theme.glyphs.cursor` in `theme.ui.accent` down its first
column. Colors come only from existing theme tokens, so every theme gets its own focus, and there
are no new theme keys. The sidebar, rail, board cards and the new-flow fields all use it. A shared
`Dialog` widget in `dialog.rs` draws every box: it dims the screen, puts the title in the border,
adds one row and two columns of padding, and shows the keys under a rule that joins the border.
A new `ui/hints.rs` fits a row of key hints into any width and draws keys that do nothing faint.
Solo chrome goes: the sidebar shows from the start with only the master. Below 110 columns the
sidebar folds to the rail until C-space b says otherwise. The board's detail panel puts the steps
under its header and shows decisions and the last answer beside the timeline.

Findings, by number: 1 narrow widths, 2 focus vs peek, 3 color repetition, 4 dialogs, 5 inactive
hints, 6 empty states, 7 labels and times, 8 detail panel, 9 one focus look.

Decided in review: inactive keys stay visible in faint; unselected board cards get no box; the
focus colors follow the theme (`ui.selection`, `ui.accent`).

## Units

`src/ui/focus.rs` (new)

```rust
/// Marks `area` as where the keys go: theme.ui.selection over it and theme.glyphs.cursor in
/// theme.ui.accent down its first column. Drawn after the content, which leaves that column blank.
pub fn mark(buf: &mut Buffer, area: Rect, theme: &Theme);

/// Lights `area` without claiming the keys: the fill alone, for the peek and a row that just changed.
pub fn light(buf: &mut Buffer, area: Rect, theme: &Theme);
```

`src/ui/hints.rs` (new)

```rust
/// One key and what it does; `active` false draws both faint, for a key that does nothing now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hint {
    pub key: &'static str,
    pub label: Cow<'static, str>,
    pub active: bool,
    /// Kept when the row is too narrow: esc and ? stay as the others go.
    pub kept: bool,
}

impl Hint {
    pub fn new(key: &'static str, label: impl Into<Cow<'static, str>>) -> Hint;
    pub fn inactive(self) -> Hint;
    pub fn kept(self) -> Hint;
}

/// The hints that fit `width`, three columns apart: whole hints are dropped from the right, the
/// kept ones last; never a cut word.
pub fn line(hints: &[Hint], width: usize, theme: &Theme) -> Line<'static>;
```

`src/ui/dialog.rs` (modified: `draw_dialog`, `draw_help`, `draw_not_running`, `draw_no_session` go)

```rust
/// A box over the session: the screen behind it dimmed, the title in its border, one row and two
/// columns of padding, and its keys under a rule that joins the border.
#[derive(Debug)]
pub struct Dialog<'a> {
    pub title: &'a str,
    /// Right of the title in the border, faint: where a new flow starts.
    pub aside: Option<String>,
    /// The border's color: rule for a notice, waiting for a question, blocked for a failure.
    pub border: Style,
    pub body: Vec<Line<'a>>,
    /// Rows the body takes beyond its lines, for a widget that draws into it (new flow).
    pub body_rows: u16,
    pub body_width: u16,
    pub keys: Vec<Hint>,
    pub theme: &'a Theme,
}

impl<'a> Dialog<'a> {
    /// A dialog with `body` and no keys, its border in the rule color; the rest by struct update.
    pub fn new(title: &'a str, body: Vec<Line<'a>>, theme: &'a Theme) -> Dialog<'a>;
    /// The box centered in `area`, a little above the middle, clamped to it; the row padding goes
    /// first when `area` is too short.
    pub fn area(&self, area: Rect) -> Rect;
    /// Where the body goes inside `area(area)`: inside the padding, above the key rule.
    pub fn body_area(&self, area: Rect) -> Rect;
}

/// Dims all of buf.area, clears the box, draws frame, title, aside, body, the ├─┤ rule and
/// hints::line(keys).
impl Widget for Dialog<'_> { fn render(self, area: Rect, buf: &mut Buffer); }

/// Every key behind C-space.
pub fn help(theme: &Theme) -> Dialog<'_>;
/// A pane whose process is gone: for a worker (its name) what to do with its flow, else the master.
pub fn not_running<'a>(status: &PaneStatus, worker: Option<&str>, theme: &'a Theme) -> Option<Dialog<'a>>;
/// A worker pane flow ui has no session for.
pub fn no_session(theme: &Theme) -> Dialog<'_>;
/// The body's side padding; new flow's focus mark reaches across it to the border.
pub const PAD_COLS: u16 = 2;
```

`src/ui/layout.rs` (modified)

```rust
/// Below this many columns the sidebar folds to the rail until C-space b says otherwise.
pub const RAIL_BELOW: u16 = 110;

pub enum Chrome { Sidebar, Rail }            // Solo removed
pub struct Areas { pub title: Rect, pub pane: Rect, pub rule: Rect, pub side: Rect } // bar removed, no Options
```

`src/ui/app.rs` (modified)

```rust
/// The sidebar folded to a rail: Some once C-space b chose, None follows the width (RAIL_BELOW).
rail: Option<bool>,                     // was bool

/// Whether the rail shows instead of the sidebar: the user's choice, else a narrow terminal.
fn folded(&self) -> bool;

/// The sidebar or rail, from the start: the master has its row even before there are workers.
pub fn areas(&self) -> Areas;
```

`src/ui/app/draw.rs`, `src/ui/app/chrome.rs`, `src/ui/app/keys.rs` (modified)

```rust
// draw.rs: every dialog is a Dialog rendered over areas.session(); the DIM call goes.
fn draw_view(&self, frame: &mut Frame, areas: &Areas, theme: &Theme);
/// The stop question, or the flows it would leave unfinished with keep, end and esc as keys.
fn stop_dialog<'a>(&'a self, theme: &'a Theme, unfinished: &[String]) -> Dialog<'a>;
/// The scrollback hint along the pane's last row, fitted by hints::line.
fn draw_history(&self, frame: &mut Frame, pane: Rect, back: u32, theme: &Theme);

// chrome.rs: draw_bar goes; title_line drops the needs-you bar (the state glyph says it); the
// master's title says its whole place while the rail shows (nothing else does), else its branch.
fn title_line(&self, theme: &Theme, width: u16, right: usize) -> Line<'static>;

// keys.rs: Command::Rail sets self.rail = Some(!self.folded()); every read of self.rail is folded().
```

`src/ui/sidebar.rs`, `src/ui/sidebar/rows.rs`, `src/ui/sidebar/labels.rs` (modified)

```rust
/// How a row stands out: focused (fill and bar), lit (fill: the peek, a state that just changed).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Mark { None, Lit, Focus }

pub(super) struct Row { pub line: Line<'static>, pub key: Option<PaneKey>, pub mark: Mark } // selected -> mark

/// Under the master while there are no workers: "no workers yet" and C-space n.
fn empty_rows(&self) -> Vec<Row>;
```

- rows.rs: the lead is always one blank column (focus::mark draws the bar into it); the focused
  flow and the master get Mark::Focus, the peek and a changed flow Mark::Lit; the second row's right
  column is tasks only; a finished flow says `took 2h`; the peek's hint reads `C-space enter jump in`.
- labels.rs `steps()`: done steps and reached joins in ui.text, the current one in
  `theme.step_style(flow)` (main's phase_colors level), open ones faint; a finished flow draws every
  step in ui.text.
- sidebar.rs `render`: set_line, then focus::mark / focus::light by Mark.

`src/ui/rail.rs` (modified): the focused row, the master included, gets focus::mark; the master's
`0` is no longer always in accent.

`src/ui/board.rs`, `src/ui/board/cards.rs`, `src/ui/board/detail.rs` (modified)

```rust
// board.rs
/// Its keys for the selected card: those that do nothing for it drawn inactive, fitted to `width`.
fn footer(&self, width: usize) -> Line<'static>;
fn header(&self, width: usize) -> Line<'static>;   // "1 session", "N sessions"

// cards.rs
/// The first row carries the state label through fit::split: the name is cut with … and two
/// columns stay between them.
fn flow_card(&self, flow: &FlowView, number: Option<usize>, width: usize) -> [Line<'static>; 4];
fn master_card(&self, width: usize) -> [Line<'static>; 4];       // "no workers yet" at 0
/// The selected card gets focus::mark over its rect instead of the accent border; the body keeps its place.
fn render_card(&self, flow: Option<&FlowView>, number: Option<usize>, selected: bool, rect: Rect, buf: &mut Buffer);

// detail.rs
/// Header (name and facts), the steps under it, a rule, then the timeline beside the answer.
pub(super) fn render_detail(&self, area: Rect, buf: &mut Buffer);
/// DECISIONS, then LAST ANSWER: the flow's closing message wrapped, its end kept when it is long.
fn render_answer(&self, flow: &FlowView, area: Rect, buf: &mut Buffer);   // was render_facts
```

- cards: the label says less (`waiting`, `agent`) when the name would keep fewer than 10
  columns (NAME_KEPT), as the 96-column board showed `3 ◓ e…  reviewing · agent`;
  a blocker and a waiting question are both ui.text; ` · guided` only when it fits; ages
  read `3m` as in the sidebar (no ` ago`); `set_span` of the label goes.
- detail: render_master_detail says `no workers yet  n starts a flow` when there are none; the
  needs-you entries use ui.text for blockers and questions alike.

`src/ui/newflow/view.rs`, `src/ui/newflow.rs` (modified)

```rust
impl Widget for NewFlowView<'_> {
    /// A Dialog (title "new flow" or "new session", aside "in ~/shop", keys enter, tab, esc)
    /// with its fields drawn into the body; the focused field gets focus::mark.
    fn render(self, area: Rect, buf: &mut Buffer);
}
impl NewFlowView<'_> {
    /// The NewFlowForm as a Dialog, so the view and the cursor agree on where the body is.
    fn dialog(&self) -> Dialog<'_>;
    /// "(•)" in ui.strong; in the focused row the chosen option is reversed.
    fn radio(&self, field: Field, on: bool, label: &str) -> Vec<Span<'static>>;
}
// newflow.rs: dialog_area goes; cursor() takes the body from Dialog::body_area.
pub fn cursor(&self, body: Rect) -> Option<(u16, u16)>;
```

The focused field's mark spans the dialog's inner width, border to border (the request's label
and box together), so its bar stands where the sidebar's does. Labels stay faint.

The text box keeps a rule-colored rounded border in every state; with focus its rows get
focus::mark and the cursor shows.

`src/ui/theme.rs` (modified)

```rust
impl Theme {
    /// A section's heading: its name in capitals, faint, as the sidebar's NEEDS YOU.
    pub fn heading(&self, text: &str) -> Span<'static>;
}
```

Used for the board's TIMELINE, DECISIONS, LAST ANSWER, NEEDS YOU and READY FOR YOU. No new
tokens: focus is ui.selection + ui.accent, lit rows ui.selection, dialog borders ui.rule,
state.waiting or state.blocked.

## Data

- `Hint { key, label, active, kept }`: widgets -> hints::line.
- `Dialog { title, aside, border, body, body_rows, body_width, keys, theme }`: app/draw.rs and
  newflow -> the frame buffer.
- `sidebar::Mark { None, Lit, Focus }`: rows -> Sidebar::render, Rail::render.
- `App.rail`: `bool` -> `Option<bool>`.
- `Chrome`: Solo removed; `Areas`: bar removed, rule and side no longer Option.

Nothing stored changes: the `--json` contract, theme files and key bindings stay.

## Call chains

Redraw, the focused sidebar row:
1. App::draw -> Sidebar::render(side, buf)
2. Sidebar::render -> Sidebar::rows(height): Vec<Row>; focused rows Mark::Focus, peek and changed rows Mark::Lit
3. Sidebar::render -> Buffer::set_line(x, y, &row.line, width) for each row
4. Sidebar::render -> focus::mark(buf, row_rect, theme) for Focus, focus::light(buf, row_rect, theme) for Lit
5. No workers: rows() has the header, the master and empty_rows(), never an empty sidebar.

C-space ?, a dialog (help, stop, confirm, not running, no session all take this path):
1. App::on_input(Routed::Command(Help)) -> App::on_command: view = View::Help
2. App::draw -> App::draw_view(frame, &areas, theme)
3. App::draw_view -> dialog::help(theme): Dialog
4. App::draw_view -> Frame::render_widget(dialog, areas.session())
5. Dialog::render -> Buffer::set_style(buf.area, Modifier::DIM); Clear; frame with title; body inside the padding
6. Dialog::render -> hints::line(&keys, inner_width, theme): Line, under a ├─┤ rule
7. Too small a terminal: Dialog::area clamps to the area; body lines that do not fit are left out, the keys row before the body.

The board at any width:
1. App::draw -> App::draw_board -> BoardView::render(screen, buf)
2. BoardView::render -> BoardView::header(width): "1 session" / "N sessions"
3. BoardView::render -> render_cards -> render_card(flow, number, selected, rect, buf)
4. render_card -> flow_card(flow, number, width) -> fit::split(left, [gap, state_label], width)
5. render_card -> focus::mark(buf, rect, theme) when selected
6. BoardView::render -> render_detail -> header, Sidebar::steps line, rule -> render_timeline(left), render_answer(right)
7. render_answer -> fit::wrap(answer, width): the last rows that fit, under DECISIONS when there are any
8. BoardView::render -> footer(width) -> hints::line(&hints, width, theme); `x clear finished flow` inactive unless the selected card is a finished flow of this project; `enter jump in` inactive on another project's card; `0-9 select` and `h j k l move` inactive with one card
9. An inactive key pressed anyway: on_board_key keeps today's notice ("only a finished flow can be cleared").
10. No answer yet: render_answer shows only the decisions, or nothing.

Tab in new flow:
1. App::on_input(Routed::View(keys)) -> NewFlowForm::on_key(Key::Tab): field = Agent
2. App::draw_view -> Frame::render_widget(NewFlowView, areas.session())
3. NewFlowView::render -> NewFlowView::dialog(): Dialog -> Widget::render
4. NewFlowView::render -> Dialog::body_area(area) -> request box, agent_row, mode_row, creates
5. NewFlowView::render -> focus::mark(buf, agent_row_rect, theme); radio() reverses the chosen agent
6. App::draw_view -> NewFlowForm::cursor(body): None while the agent row has focus

Resize to 96 columns, then C-space b:
1. ui loop -> App::on_resize(size): self.size = size
2. App::on_resize -> App::areas() -> App::folded(): rail None and 96 < RAIL_BELOW -> Chrome::Rail
3. App::on_resize -> Effect::Send(ClientMsg::Resize(areas.pane_size()))
4. C-space b -> App::on_command(Rail): rail = Some(!folded()); the sidebar stays until the next C-space b

The first worker starts:
1. Server -> App board update with one worker
2. App::areas(): still Chrome::Sidebar, the pane keeps its size, no Resize sent
3. Sidebar::rows: empty_rows() gives way to the group blocks

## Test seams

- focus: Buffer tests - fill on every cell, bar only in column 0, symbols kept, colors equal the
  palette's selection and accent for terminal, flow and flow-light.
- hints: all fit; drops from the right; kept last; inactive faint; never a cut word; zero width.
- Dialog: area and body_area (padding, clamping); the rule joins the border with ├ ┤.
- layout: existing tests without Solo; App::folded by width and with the C-space b override (app/tests/panes.rs).
- cards: name cut before its label at 30 columns; mode dropped when it does not fit; blocker in text style.
- detail: the answer's end kept; no answer, no heading; steps under the header.
- sidebar: Mark per row (focus, peek, changed); `took 2h`; tasks-only column; empty rows.
- golden: every view rewritten with FLOW_UPDATE_GOLDEN=1 and read by eye; solo becomes master-alone; new narrow-board (96
  columns), empty-board, new-flow-agent (focus on agent), stop-unfinished.
- E2E: scratch FLOW_HOME in a separate tmux server (`tmux -L`) at 150x44 and 96x30, the review's screens.

## Assumptions

1. The rail threshold is 110 columns.
2. C-space b overrides the width until the next C-space b; a resize keeps the choice.
3. The peek and a just-changed row get the fill without the bar; only one row ever has the bar.
4. The title row's needs-you bar goes; the state glyph says it.
5. Ages read `3m` on cards as in the sidebar; finished flows read `took 2h`.
6. Dialog borders are rule-colored except a question (waiting) and a failure (blocked); the title is in the border.
7. A long last answer shows its end.
8. The rail marks the master like any focused row; its `0` is no longer always accent.

## Tasks

### Shared drawing

- [x] Add `ui/focus.rs` with `mark` and `light`, and unit tests for fill, bar column, kept symbols and each built-in palette's colors
- [x] Add `ui/hints.rs` with `Hint` and `hints::line`, and unit tests for fitting, dropping from the right, kept hints, inactive style and zero width
- [x] Add `Theme::heading` with a test that it is uppercase and faint

### Dialogs (items 4, 9)

- [x] Add `Dialog` with `area`, `body_area` and `Widget::render` (dim, padding, title, aside, ├─┤ rule, keys), and unit tests for geometry and the joined rule
- [x] Move help, not running, no session, confirm and stop onto `Dialog` (`dialog::help`, `dialog::not_running`, `dialog::no_session`, `App::stop_dialog`), remove `draw_dialog`; update the help and stop goldens
- [x] Draw `NewFlowView` inside a `Dialog` (`NewFlowView::dialog`, `NewFlowForm::cursor(body)`), mark the focused field with `focus::mark`, draw `(•)` strong and the focused row's choice reversed; remove `dialog_area` and the DIM call in draw.rs; update new-flow goldens and add new-flow-agent
- [x] Fit the scrollback hint with `hints::line` (`App::draw_history`)

### Focus and color (items 2, 3, 9)

- [x] Replace `Row::selected` with `Mark`; sidebar rows draw the bar through `focus::mark` and the peek and changed rows through `focus::light`; the peek hint reads `C-space enter jump in`; sidebar tests per Mark
- [x] Mark the rail's focused row, master included, with `focus::mark`
- [x] Board cards: `focus::mark` instead of the accent border; the label through `fit::split` with a two-column gap; mode only when it fits; blocker and question in ui.text; cards tests at 30 columns
- [x] Calmer steps in `Sidebar::steps` (done in text, current in the flow's color, open faint); drop the title row's needs-you bar; update goldens

### Layout and empty states (items 1, 6)

- [x] Remove `Chrome::Solo`, `Areas::bar` and `draw_bar`; the sidebar shows with only the master and `Sidebar::empty_rows`; update layout tests and the solo golden
- [x] Add `RAIL_BELOW`, `App.rail: Option<bool>` and `App::folded`; C-space b sets the choice; tests for width and override in app/tests/panes.rs
- [x] Board header `1 session`, master card and master detail `no workers yet` / `n starts a flow`; add the empty-board golden

### Hints and labels (items 1, 5, 7)

- [x] Board footer through `hints::line` with inactive keys for the selected card; add the narrow-board golden at 96 columns
- [x] Labels: `took 2h`, tasks-only right column in the sidebar, `3m` ages on cards, `Theme::heading` for every section heading

### Detail panel (item 8)

- [x] `render_detail` with the steps under the header and `render_answer` (decisions, last answer's end); detail tests

### Finish

- [x] Copy the approaches and implementation pages to `docs/design/polish-approaches.html` and `docs/design/polish-implementation.html`; update CLAUDE.md's ui module list for focus.rs and hints.rs
- [x] End-to-end: build, run flow ui on a scratch FLOW_HOME in a separate tmux server at 150x44 and 96x30 with stand-in workers, capture every screen from the review and compare by eye under noctalia and the terminal theme; clippy and fmt clean

## Review

Reviewed in isolation against this plan and CLAUDE.md, every changed line of `main...flow/tui-polish`.

**Fixed**

- On every project's board, a card of another project drew `enter jump in` and (when finished) `x clear finished flow` as active, though both only say "runs in project api". Shown by `draws_enter_and_x_faint_on_a_card_of_another_project`; fixed with `BoardView::reachable` (ui: board: enter and x faint on a card of another project). The fix was re-read on its own, not by a separate agent.

**Open**

- None blocking. A finished flow whose start and last change share a second reads `took 0s` (the fleet fixture shows it); real flows will not, so left as is.
- The selected board card's focus fill covers its whole rect, the blank rows above and below the text included (narrow-board golden). That follows the plan; worth a look on the real terminal if it reads as too heavy.

**Where to look**

1. `src/ui/dialog.rs` `Dialog::area` / `body_area` / `render`, then `src/ui/newflow/view.rs` `render` and `NewFlowForm::cursor`: the body geometry the view and the text cursor must agree on, and how a short terminal drops padding, then body rows.
2. `src/ui/app.rs` `folded()` with `keys.rs` `Command::Rail` and `messages.rs` (the Board-time Resize is gone because chrome no longer depends on workers): the pane size the server is told.
3. `src/ui/board/cards.rs` `first_row`: when the state label shortens and how the name is cut.
4. Routine, skim: `focus.rs`, `hints.rs`, theme headings, sidebar/rail `Mark`, label wording, goldens.

**Checks**

`cargo test`, `cargo clippy --all-targets -- -D warnings` and `cargo fmt --check` pass at the fix commit.
