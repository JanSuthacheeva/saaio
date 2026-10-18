# Plan: docs read-through fixes (2026-10-18)

Approaches page: https://claude.ai/artifact/32nXz8s5FNgutSCVzmtvtz (Jan approved all four
recommendations and the plan through the master; no separate implementation page).

## Decision

- `saaio ui` is gone. Bare `saaio` opens the UI for a person, `saaio --stop` stops it. `saaio server`
  stays hidden and internal.
- `commands/ui.rs` keeps one `run` that opens the UI. A new `commands/stop.rs` holds `run` for root
  `--stop`, with the worker-store guard moved there: bare `saaio` inside a worker prints the
  overview (`Env::is_person`), so only stop can still reach the guard.
- `saaio ui` afterwards gets clap's normal usage error (`unrecognized subcommand 'ui'`, exit 2). No
  hidden hint command.
- Agents run the UI for an E2E check in a PTY with the agent markers unset, documented in CLAUDE.md:
  `SAAIO_HOME=$(mktemp -d) env -u CLAUDECODE -u CODEX_THREAD_ID -u SAAIO_FLOW_ID -u SAAIO_MASTER saaio`.
- Board cards show the worker's agent faint on the third row before the age (`claude · 29m`),
  dropped when it does not fit. The detail header lists it after the mode.
- The sidebar loses the master's agent marker.
- Docs, README, ROADMAP, examples, CHANGELOG 0.1.0 entry, master.md and user-visible strings name
  the program `saaio` and the screen "the UI" (docs/ui.md title: "The terminal UI").

## Units

```rust
// src/cli.rs (modified): Command::Ui removed
use commands::{clear, home, hook, new, remove, report, send, server, start, status, stop, ui, wait};

match cli.command {
    None if cli.stop => stop::run(store, env, clock),
    None if env.is_person() => ui::run(store, env, clock),
    None => home::run(store, &env.bin_path(), now),
    // ...
}

// src/cli/commands/ui.rs (modified): no Args, no guard, no stop
/// Opens the terminal UI, starting the server first.
pub fn run(store: &Store, env: &Env, clock: fn() -> u64) -> Result<Document, Failure>;

// src/cli/commands/stop.rs (new)
/// Stops the server of `store` and every session in it; refused on a worker's own store.
pub fn run(store: &Store, env: &Env, clock: fn() -> u64) -> Result<Document, Failure>;
fn refuse_inside_own_flow(store: &Store, env: &Env) -> Result<(), Failure>;

// src/ui/sidebar.rs, src/ui/app.rs, src/ui.rs (modified)
// removed: Sidebar::master_agent, App::master_agent, App::set_master_agent, the reload_config call

// src/ui/board/cards.rs (modified)
impl BoardView<'_> {
    fn flow_card(&self, flow: &FlowView, number: Option<usize>, width: usize) -> [Line<'static>; 4];
    // third row right side: [agent faint, " · ", age] when left + agent + age fit, else [age]
}

// src/ui/board/detail.rs (modified)
// render_detail facts: request, mode, agent (workspace.agent), branch or folder, slot, MR, started
```

## Strings

| Where | Before | After |
| --- | --- | --- |
| `cli.rs` after_help | Run alone in a terminal, saaio opens saaio ui. | Run alone in a terminal, saaio opens the terminal UI. |
| `cli.rs` Start doc | ... and an agent saaio ui opens | ... and an agent the UI opens |
| `cli.rs` Server doc | The session server saaio ui starts on its own | The session server saaio starts on its own |
| `stop.rs` refusal | saaio ui runs inside a flow's worker ... | `saaio --stop` runs inside a flow's worker, on the store that flow lives in: it would end the user's sessions. Help: `Set SAAIO_HOME to a scratch folder to stop a server of your own` |
| `client.rs:64` | run `saaio ui --stop` and start saaio ui again | run `saaio --stop` and start saaio again |
| `client.rs:138` log | saaio ui --stop (saaio {BUILD}) sends ... | saaio --stop (saaio {BUILD}) sends ... |
| `keys.rs:36` | run `saaio ui` to attach again | run `saaio` to attach again |
| `keys.rs:152` | resume the next time you run `saaio ui` | resume the next time you run `saaio` |
| `draw.rs:244` | the next time saaio ui starts | the next time saaio starts |
| `dialog.rs:234` | saaio ui has no session for this flow. | saaio has no session for this flow. |
| `messages.rs:66` | another saaio ui took over: pid ... | another terminal took over: pid ... |
| `output/start.rs` | pane `when saaio ui runs`, `saaio ui opens X in its sidebar`, ``Run `saaio ui` to open the worker`` | `when saaio opens`, `saaio opens X in its sidebar`, ``Run `saaio` in a terminal to open the worker`` |
| `output/status.rs:322` | C-space r in saaio ui resumes it | C-space r in saaio resumes it |
| `adapter.rs` MASTER_OPENING | saaio ui just started you. | saaio just started you. |
| `server/master.md` | You run inside `saaio ui`. ... a saaio ui key | You run inside saaio's terminal UI. ... a saaio key; old text kept as `master-v10.md` |

Doc comments that name the client process stay as they are; only text a user or an agent reads
changes.

## Docs

- README: Usage shows only `saaio` (and the key table); features bullet without the master's agent;
  "How it works" opens with what a flow is: a unit of work, one feature or ticket, with its own
  worktree and branch, a worker (the agent session doing it) and phases it walks through; CLI table
  without `saaio ui`; Documentation link "The terminal UI"; setup-saaio recommended for every
  project (Workflow section).
- docs/cli.md: reworked for the current set: `saaio` section covers the UI, `--stop` and the
  overview; no `saaio ui` section; start/send/remove/env texts updated.
- docs/flows.md: a lead paragraph saying what a flow is before the terms table.
- docs/repository.md: recommends installing setup-saaio and running it once per project, at the top
  and in its section.
- docs/ui.md, install.md, files.md, troubleshooting.md, themes.md, configuration.md, json.md,
  docs/README.md, examples/*.toml, ROADMAP, CHANGELOG 0.1.0: no `saaio ui`.
- ROADMAP: drop "Codex commits without asking".
- CLAUDE.md: E2E line with the PTY workaround; `saaio ui` mentions replaced.
- New: CONTRIBUTING.md, .github/pull_request_template.md, .github/CODEOWNERS (`* @JanSuthacheeva`).
  Plain reference style, no semicolons, no first person.

## Call chains

1. Person types `saaio` -> `main` -> `cli::run(Cli { stop: false, command: None })` ->
   `env.is_person()` -> `ui::run(store, env, clock)` -> `env.config()?` -> `client::ensure_server` ->
   `ui::run(&socket, ui_env, clock)` -> `Document::Text(message)`. Broken config or bad prefix: the
   config Failure, exit 1, no server.
2. `saaio --stop` -> `cli::run` -> `stop::run(store, env, clock)` -> `refuse_inside_own_flow` (Err:
   Failure exit 1 on a worker's own store) -> `client::stop_server(socket, clock)` -> "stopped the
   saaio server and every session in it" | "no saaio server runs".
3. `saaio ui` -> `Cli::try_parse` Err -> `Failure::usage` -> `error: unrecognized subcommand 'ui'`,
   exit 2.
4. Board draw -> `BoardView::flow_card` -> third row `split(progress, [agent, " · ", age])` ->
   `render_detail` facts with the agent.

## Test seams

- `tests/cli.rs`: every `["ui", ...]` call becomes bare `--stop` or a PTY `Terminal::open`; the
  prefix-refusal test runs bare `saaio` in a PTY; a test that `saaio ui` exits 2 with
  `unrecognized subcommand 'ui'`; help no longer lists `ui`; start/status string assertions updated.
- `src/ui/board/cards.rs` tests: agent before the age on a wide card, dropped on a narrow one.
- `src/ui/board/detail.rs` tests (or golden): agent in the facts.
- `src/ui/app/tests/sidebar.rs`: the master test asserts no agent on row 0.
- Goldens via `SAAIO_UPDATE_GOLDEN=1`: board, board-detail, master rows, no-session dialog, stop.
- `src/server/master.rs`: the master-v10 history test pattern as for v9.

## Assumptions

- `pane:` value for a not yet running UI reads `when saaio opens`.
- "another terminal took over" replaces "another saaio ui took over".
- Media file names (`docs/media/saaio-ui.*`) stay; only alt texts change.
- The CHANGELOG 0.1.0 entry is rewritten, not added to (no new entries before 0.1.0).
- No CHANGELOG line for this PR.

## Tasks

- [x] Remove `Command::Ui`: `ui::run` without Args, new `stop.rs` with the guard, dispatch in `cli.rs`, help texts; rework `tests/cli.rs` (bare `--stop`, PTY opens, `saaio ui` usage error, help) - `cargo test` green
- [x] Replace "saaio ui" in user-visible strings (client, keys, draw, dialog, messages, start and status output, MASTER_OPENING) with tests and goldens updated
- [x] master.md without `saaio ui`, old text kept as `master-v10.md` in the history list, with its upgrade test
- [x] Remove the sidebar master agent marker (`Sidebar::master_agent`, `App::set_master_agent`, reload_config call), tests and goldens
- [x] Board: agent on the card's third row and in the detail facts, with card tests and goldens
- [x] docs/cli.md reworked for the current command set
- [x] README (usage, how it works with what a flow is, CLI table, features, setup-saaio), docs/flows.md lead, docs/repository.md setup-saaio recommendation
- [x] Every other `saaio ui` in docs, examples, ROADMAP, CHANGELOG 0.1.0, CLAUDE.md (with the PTY E2E line); ROADMAP drops "Codex commits without asking"
- [x] CONTRIBUTING.md, .github/pull_request_template.md, .github/CODEOWNERS
- [x] End to end: in a PTY with a scratch store run `saaio` (UI opens), `saaio --stop`, `saaio ui` (usage error), look at the board cards and the detail; `cargo test`, `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`
