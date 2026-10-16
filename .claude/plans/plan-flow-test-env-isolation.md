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

- [ ] `flow_command` clears every inherited FLOW_* variable and points XDG_CONFIG_HOME and
      CLAUDE_CONFIG_DIR into the test's folder; `FLOW_MASTER=1 cargo test` and a run with every
      FLOW_* of a worker pane set both pass
