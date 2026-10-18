# Plan: sidebar agent markers

Review page: `.lavish/harness-marker-approaches.html` (Lavish returned HTTP 500; Jan chose in chat).

## Decision

Use the configured agent name from each flow's existing `workspace.agent`. On the first sidebar row, show the faint agent name before the elapsed time when the full flow name, agent and time fit in the 40-column row. If they do not, keep the full agent name on the first row and move time to the existing second row. Fit by terminal display columns, never a character threshold. Plain sessions follow the same rule. Show the configured master agent at the right of its first row if it fits. No new row, glyph, persistence or protocol field.

Pane failure text (`failed` / `exited`) continues to take priority as a status; put it on the second row when it cannot fit beside the agent. Clip a custom agent name only if it alone cannot fit beside the left side. If a second-row suffix competes with phase/task text, use the existing `fit::split` clipping behaviour.

## Units

`src/ui/sidebar/rows.rs` (modified):

```rust
impl Sidebar<'_> {
    pub(super) fn master_rows(&self) -> Vec<Row>;
    pub(super) fn flow_rows(&self, flow: &FlowView, index: usize) -> Vec<Row>;
}
```

`src/ui/sidebar.rs` (modified):

```rust
pub struct Sidebar<'a> {
    pub master_agent: Option<&'a str>,
    // existing fields
}
```

`src/ui/app.rs` and `src/ui.rs` (modified):

```rust
impl App {
    pub fn set_master_agent(&mut self, agent: Option<String>);
    fn sidebar<'a>(&'a self, theme: &'a Theme, peek: &'a [PeekLine]) -> Sidebar<'a>;
}
```

`reload_config` passes `config.master_agent().map(|(name, _)| name.to_string())` to `App::set_master_agent`; broken config keeps the last good name.

## Data and call chains

No stored or protocol data changes. `Workspace.agent` is already the name used to launch the worker.

1. Server board -> `App::on_server(ServerMsg::Board)` -> `App::sidebar` -> `Sidebar::flow_rows` reads `flow.workspace.agent` -> render first or second row.
2. UI config load/reload -> `Config::master_agent` -> `App::set_master_agent` -> `App::sidebar` -> `Sidebar::master_rows` -> render marker if it fits.
3. Missing workspace/agent -> existing row content; config error -> last good master name.

## Test seams and assumptions

- `src/ui/app/tests/sidebar.rs`: exact first and second row assertions for both width outcomes, custom agent, plain session and master.
- `src/ui/app/tests/golden.rs`: update intended snapshots with `SAAIO_UPDATE_GOLDEN=1`; include a long-name case that forces variant B.
- Existing `src/ui/fit.rs::columns` and `fit::split` measure and fit terminal columns.
- Sidebar remains 40 columns wide; rail and board layouts do not gain markers.
- Agent names come from config keys; display them as-is, with no icon or alias.

## Tasks

- [x] Render width-aware worker and plain-session markers in `Sidebar::flow_rows`, with first/second-row and golden tests.
- [x] Pass the configured master agent through `reload_config`, `App::set_master_agent` and `Sidebar::master_rows`; cover it in tests and update user documentation.
- [ ] Run the UI golden test as an end-to-end rendering check, the full test suite and `cargo clippy --all-targets -- -D warnings`.
