# Plan: tests that pass from any shell

## Decision

Every CLI test spawns the binary through `flow_command` in `tests/cli.rs`, which sets FLOW_HOME,
XDG_STATE_HOME and FLOW_CONFIG and removes FLOW_ID, but inherits the rest of the shell. Inside a
master pane FLOW_MASTER=1 leaks in and `flow hook` treats every test's hook as the master's: 7
tests fail (`FLOW_MASTER=1 cargo test`). Inside a worker FLOW_NAME, FLOW_SLOT, FLOW_REPO and
FLOW_WORKTREE leak in too, harmless today only by luck.

`flow_command` removes every inherited variable named `FLOW_*`, whatever flow reads now or later,
and points the remaining homes flow reads (XDG_CONFIG_HOME, CLAUDE_CONFIG_DIR) into the test's
own folder, so no test reads the user's config or Claude Code home. HOME stays inherited: the
git that flow runs reads the user's git config through it, and the tests that depend on HOME set
it themselves. Tests that need a FLOW_* variable set it after `flow_command`, as they do now.

## Units

`tests/cli.rs` (modified):

```rust
/// The flow binary with a test's own state: `home` as FLOW_HOME, the shared folder inside it, a
/// config file that does not exist (so the built-in agents), no Claude Code home of the user's
/// and no FLOW_* variable of the shell it runs in.
fn flow_command(home: &Path) -> Command;
```

Iterates `std::env::vars_os()`, `env_remove`s each key starting with `FLOW_`, then sets
FLOW_HOME, XDG_STATE_HOME, FLOW_CONFIG, XDG_CONFIG_HOME (`home/no-config-home`) and
CLAUDE_CONFIG_DIR (`home/no-claude-home`).

## Data

None. No production code, flag or output changes.

## Tasks

- [x] `flow_command` clears every inherited FLOW_* variable and points XDG_CONFIG_HOME and
      CLAUDE_CONFIG_DIR into the test's folder; `FLOW_MASTER=1 cargo test` and a run with every
      FLOW_* of a worker pane set both pass

## Review

Reviewed `flow/test-env-isolation` against `main` (commits 1418ce1, e44b8c6) in a context that did
not write it.

- **Fixed** - nothing; no finding could be shown.
- **Open** - none. Checked and let go: `main.rs` `env()` reads exactly FLOW_HOME, FLOW_CONFIG,
  FLOW_ID, FLOW_MASTER, HOME, XDG_STATE_HOME, XDG_CONFIG_HOME and CLAUDE_CONFIG_DIR, so after
  `flow_command` only HOME still comes from the shell, as the plan intends. `env_remove` followed
  by `env` for FLOW_HOME and FLOW_CONFIG sets them (the later call wins). Tests that set HOME
  (`the_home_view_shortens_the_binary_only_inside_home`, `start_plain_takes_a_name_and_a_folder_under_home`)
  still work since FLOW_CONFIG and CLAUDE_CONFIG_DIR take precedence over HOME. The direct `git`
  and `python3` spawns in `tests/cli.rs` inherit the shell, but neither reads FLOW_*.
- **Where to look** - `tests/cli.rs` `flow_command`: the one changed function; the rest is routine.
- **Checks** - `cargo test` (also with FLOW_MASTER=1 and worker FLOW_* set), `cargo fmt --check`
  and `cargo clippy --all-targets -- -D warnings` passed at e44b8c6; only the plan changed since.
