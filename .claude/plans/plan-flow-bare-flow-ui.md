# Plan: bare `flow` opens flow ui for a person

Approaches page: https://claude.ai/artifact/L8u8XJjsYLMXYpoa6Mc9yc (approach A chosen; the user
approved the plan without a separate implementation page).

## Decision

`flow` with no subcommand opens flow ui when a person runs it in a terminal, and prints the
overview (today's TOON home view) when an agent or a script runs it. main.rs reads whether stdin
and stdout are terminals into `Env`, beside the env vars; `Env::is_person` holds the rule;
`cli::run`'s `None` arm runs `ui::run` for a person, `home::run` otherwise. Every subcommand,
`flow ui` included, is unchanged.

The rule, decided by the user: an agent (or a script) runs bare `flow` when stdin or stdout is not
a terminal, or when any of `FLOW_ID`, `FLOW_MASTER`, `CLAUDECODE`, `CODEX_THREAD_ID` is set.
Everything else is a person.

Verified with real runs (2026-10-17): Claude Code's Bash tool gives no terminal on fds 0-2 and sets
`CLAUDECODE=1`; Codex 0.163.0's shell gives no terminal by default, but a terminal on all three fds
when it asks for a PTY, and sets `CODEX_THREAD_ID` either way. The env markers cover that PTY case.

## Units

`src/main.rs` (modified): `env()` fills the two new fields, nothing else.

```rust
use std::io::IsTerminal;

fn env() -> Env {
    Env {
        // ...existing fields...
        terminal: std::io::stdin().is_terminal() && std::io::stdout().is_terminal(),
        agent_shell: env::var_os("CLAUDECODE").is_some() || env::var_os("CODEX_THREAD_ID").is_some(),
    }
}
```

`src/cli/env.rs` (modified): the struct's doc becomes "The environment flow runs in: its variables
and its terminal, read once by main so commands get them as values."

```rust
pub struct Env {
    // ...existing fields...
    /// stdin and stdout are both terminals.
    pub terminal: bool,
    /// $CLAUDECODE or $CODEX_THREAD_ID is set: an agent's shell tool runs flow.
    pub agent_shell: bool,
}

impl Env {
    /// True when a person runs flow at a terminal: not in a flow's session, the master's, or an
    /// agent's shell.
    pub(crate) fn is_person(&self) -> bool;
}
```

`is_person` = `terminal && !agent_shell && !flow_master && flow_id.is_none()` (a set but empty
`FLOW_ID` counts as set, since the user's rule says "set").

`src/cli/commands/ui.rs` (modified): `Args` derives `Default`, so bare `flow` passes
`&ui::Args::default()` (no `--stop`).

`src/cli.rs` (modified):

```rust
match cli.command {
    None if env.is_person() => ui::run(&ui::Args::default(), store, env, clock),
    None => home::run(store, &env.bin_path(), now),
    // ...unchanged...
}
```

`Cli` gets `#[command(after_help = "...")]`: run alone in a terminal, flow opens flow ui; run by an
agent or a script, it prints every flow and what needs you. `about` stays, since the home view
prints it as `description`.

## Data

Only the two `Env` fields above. No stored data, no `--json` change, no new flag.

## Call chains

Person at a terminal, `flow`:
1. `main -> Cli::try_parse(): Cli { command: None }`
2. `main -> env(): Env { terminal: true, agent_shell: false, flow_id: None, flow_master: false, .. }`
3. `main -> cli::run(cli, &env, now_ms)`; store and registry as today
4. `cli::run -> Env::is_person(): true`
5. `cli::run -> ui::run(&Args::default(), store, env, clock): Result<Document, Failure>` - the same
   path as `flow ui`: its refusal inside its own flow's store, `ensure_server`, `ui::run`
6. `main -> answer(..)` prints the leave message, as `flow ui` does today

Agent or script, `flow`:
1-3. as above, with `terminal: false`, or `agent_shell`, `flow_id` or `flow_master` set
4. `cli::run -> Env::is_person(): false`
5. `cli::run -> home::run(store, bin, now)`: the TOON overview, unchanged

Errors: no store -> the existing "cannot find a state directory" failure before the dispatch, for
both. Bare `flow` in a person's terminal fails exactly as `flow ui` fails (config error, server not
starting) and prints the same TOON failure.

## Test seams

- `cli/env.rs` unit tests: `is_person` true only with a terminal and no marker; false for each of
  no terminal, `agent_shell`, `flow_master`, `flow_id` (including empty).
- `tests/cli.rs`: the existing `the_home_view_*` tests keep covering piped bare `flow`. New tests
  run bare `flow` in a real PTY (`portable_pty`, already a dependency) with the test's own
  `FLOW_HOME`, `CLAUDECODE` and `CODEX_THREAD_ID` removed:
  - with `FLOW_ID` set, with `CLAUDECODE=1`, with `CODEX_THREAD_ID` set: prints the overview
    (`bin: ` first line) and exits 0.
  - with no marker: opens flow ui - the server socket appears in `FLOW_HOME` and the PTY shows the
    alternate screen - using a fake master agent from config (as `Lab::agent` does) so no real
    agent starts; the test stops it with `flow ui --stop`.

## Assumptions

- "Terminal" means stdin and stdout; stderr is not checked (`flow 2>log` still opens the UI).
- `CLAUDECODE` and `CODEX_THREAD_ID` count when set to anything, empty included.
- The help text gains an `after_help` line; `about` stays as the home view's `description`.
- The help lines agents read ("Run `flow` for all flows") stay: agents get the overview.
- master.md needs no change: it only names `flow status`.
- No CHANGELOG entry: 0.1.0 is not released yet, so the 0.1.0 entry stays as it is.

## Docs

Main already has docs-voice (PR #34); rebase on main before the docs task anyway. Only the lines
about bare `flow` change:
- `docs/cli.md`: the "Commands at a glance" row for `flow`, and the `## flow` section (a person in
  a terminal gets flow ui; an agent, a script or a pipe gets the overview; the rule in one line).
- `README.md`: the command table row for `flow`.
- `docs/ui.md`: the opening "Run it in a repository" names `flow` alone as well.

## Tasks

- [ ] Add `Env::terminal`, `Env::agent_shell` and `Env::is_person` with unit tests for every marker and the terminal
- [ ] Read the terminal and the agent markers in `main.rs`'s `env()`
- [ ] Dispatch bare `flow` to `ui::run` for a person in `cli::run` (`ui::Args: Default`), with the `after_help` text
- [ ] Add PTY tests in `tests/cli.rs`: bare `flow` prints the overview with `FLOW_ID`, `CLAUDECODE` or `CODEX_THREAD_ID`, and opens flow ui (fake master agent, scratch `FLOW_HOME`) without them
- [ ] Rebase on main, then update the bare `flow` lines in `docs/cli.md`, `README.md` and `docs/ui.md`
- [ ] E2E check: in a real terminal with a scratch `FLOW_HOME`, bare `flow` opens flow ui and detaches; `flow | cat`, `flow < /dev/null` and bare `flow` from Claude Code's shell print the overview
