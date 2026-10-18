# Board fixes: terminal Markdown and agent display investigation

Implementation review: [Lavish page](../../.lavish/board-fixes-implementation.html)

## Decision

Use one shared terminal Markdown renderer for the sidebar peek and board's last answer. Parse CommonMark with `pulldown-cmark`, convert it to styled ratatui lines, and fit those lines using terminal display columns. Keep each surface's existing prefix and row budget.

A scratch `SAAIO_HOME` flow started with `--agent codex` reported `workspace.agent = codex` in `status --json`; the 80-column PTY board showed `codex · age` on its worker card. The empty board has no worker card. No agent display code change is needed; keep this as an end-to-end regression check.

## Units

### `src/ui/markdown.rs` — new

Converts worker Markdown to styled, width-bounded lines. Uses `pulldown-cmark` events. Private renderer state tracks block prefixes, list numbering, inline styles, and terminal columns.

```rust
pub fn lines(text: &str, width: usize, theme: &Theme) -> Vec<Line<'static>>;
```

### `src/ui/sidebar/rows.rs` — modified

`Sidebar::peek_rows` calls `markdown::lines` for `PeekLine::Text` after shortening paths. It adds the existing transcript glyph to the first rendered row, blank lead to subsequent rows, and applies the existing peek row budget.

### `src/ui/board/detail.rs` — modified

`BoardView::render_answer` calls `markdown::lines` for `FlowView.answer`. It preserves the decisions, heading, overflow glyph and tail-first height behavior.

### `Cargo.toml`, `src/ui.rs`, `README.md`, `CHANGELOG.md` — modified

Add `pulldown-cmark`, declare the module, and document the user-visible change.

## Data and rendering

No persisted data or JSON view changes. The renderer returns `Vec<Line<'static>>` with terminal styles only. Strong and emphasis use modifiers; inline code uses muted text; a link displays its label and its URL in parentheses when they differ. Headings use strong style; unordered and numbered lists use visible plain text markers with hanging indentation; fenced code preserves line breaks and uses muted style; blockquotes use `│`. Paragraph gaps collapse to at most one blank row. Raw HTML is rendered as text. Wrapping counts Unicode terminal columns; oversized words use the existing ellipsis clipping behavior. Every row fits its supplied width.

## Call chains

1. `PeekCache::refresh(flow)` -> transcript parser -> `PeekLine::Text`.
2. `Sidebar::peek_rows(flow, budget)` -> path shortening -> `markdown::lines(text, WIDTH - 4, theme)` -> glyph/lead spans -> budgeted rows -> buffer.
3. `fold::fold(events)` -> `FlowView.answer` -> `BoardView::render_answer(flow, area, buf)` -> `markdown::lines(answer, width, theme)` -> tail selection -> buffer.
4. Malformed Markdown is treated as text by the parser; width zero yields no visible text. Empty answers continue to omit the heading.

## Test seams

Unit tests in `src/ui/markdown.rs` cover inline and block formatting, Unicode width, long words, and line breaks. Sidebar tests cover peek glyph and wrapping. Board tests cover answer styling and retained tail. Golden tests cover the intended screen change. The end-to-end check uses a scratch store, real flow, PTY board and `status --json`, plus the full test suite and required clippy command.

## Assumptions

Only worker text in `PeekLine::Text` and `FlowView.answer` gets Markdown styling. Tool calls, tool results, decisions and timeline keep their current presentation. The agent display code stays unchanged because the requested reproduction shows it already works when a flow exists.

## Tasks

- [x] Add `pulldown-cmark`, `ui::markdown::lines`, and focused tests for Markdown styling and width.
- [x] Use `markdown::lines` in `Sidebar::peek_rows`; add peek behavior tests and refresh intended golden output with `SAAIO_UPDATE_GOLDEN=1`.
- [x] Use `markdown::lines` in `BoardView::render_answer`; add answer behavior tests and refresh intended golden output with `SAAIO_UPDATE_GOLDEN=1`.
- [ ] Update README and CHANGELOG, then run a scratch PTY worker flow to verify the agent label, full tests, formatting, and `cargo clippy --all-targets -- -D warnings`.
