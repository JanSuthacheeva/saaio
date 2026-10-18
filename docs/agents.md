# Agent permissions

Reference for workers run by saaio. Verified with Codex CLI 0.163.0 and Claude Code 2.1.302 on
2026-10-18. Permission choices belong to each harness, not to saaio.

| Harness | saaio supplies on each launch and resume | Configure separately |
| --- | --- | --- |
| Codex | Eight reporting hooks and trust for those exact hooks through `-c`, plus trust for the worker's worktree. The worker also receives instructions for talking to the master. | Approval handling, command rules, sandbox network access, and reasoning effort. |
| Claude Code | Reporting and wait hooks in a `--settings` JSON argument, plus worker instructions through `--append-system-prompt`. | Permission mode and allow rules. |

saaio does not edit `~/.codex` or `~/.claude`. The Codex trust passed by saaio covers its own hooks
and the worker's worktree. Other hooks still follow Codex's normal trust review. See
[Flows](flows.md#how-a-worker-reports) for what the hooks report.

## Codex

| Setting | Location | Effect |
| --- | --- | --- |
| `--approve-for-me` | Both `command` and `resume` under `[agents.codex]` | Routes approval requests to Codex's automatic review in the workspace-write sandbox. The reviewer can still reject a request. |
| `prefix_rule` | `~/.codex/rules/default.rules` | Allows selected commands without an approval prompt. Match the executable and arguments, not a shell command containing them. |
| `[sandbox_workspace_write] network_access = true` | `~/.codex/config.toml` | Allows outbound network access from the workspace-write sandbox for commands such as `gh`, `git push`, and `npx`. Other approval checks still apply. |
| `model_reasoning_effort = "medium"` | `~/.codex/config.toml` | Uses less reasoning effort for routine work than higher settings. Available levels depend on the model. |

To enable automatic approval review for saaio workers, define both launch commands in
`~/.config/saaio/config.toml` (or your [configured location](configuration.md#file-location)). An
agent with a built-in name replaces that definition, so retain its prompt and adapter:

```toml
[agents.codex]
command = ["codex", "--approve-for-me", "{prompt}"]
prompt = "$ship-feature {request}"
resume = ["codex", "--approve-for-me", "resume", "{session}"]
adapter = "codex"
```

Example Codex user settings:

```toml
model_reasoning_effort = "medium"

[sandbox_workspace_write]
network_access = true
```

Example `~/.codex/rules/default.rules` entry:

```text
prefix_rule(pattern=["cargo", "test"], decision="allow")
```

`codex execpolicy check --rules ~/.codex/rules/default.rules cargo test` checks a single command.
The same rule does not match `/bin/zsh -lc 'cargo test && echo done'`. Give frequently used commands
their own narrow rules. A rule for `cargo test` does not approve arbitrary shell chains.

## Claude Code

| Mode | Behavior for workers |
| --- | --- |
| `manual` | Prompts for commands and edits that have no allow rule. |
| `acceptEdits` | Accepts edits and common file operations in working directories. Other commands can still prompt. |
| `auto` | A background classifier reviews actions that would normally prompt. Availability depends on the account and environment. |
| `dontAsk` | Denies actions that would prompt unless an allow rule covers them. |
| `plan` | Limits work to planning and read-only exploration. |
| `bypassPermissions` | Skips permission prompts and grants broad access. Use only in an isolated environment. |

For fewer routine prompts, use `auto` where available. Otherwise, use `acceptEdits` with narrow
allow rules for repeatable commands. Set the starting mode and rules in `~/.claude/settings.json`:

```json
{
  "permissions": {
    "defaultMode": "acceptEdits",
    "allow": [
      "Bash(cargo test)",
      "Bash(cargo fmt)"
    ]
  }
}
```

Claude Code also accepts `--permission-mode` for a session. Its rules use `Tool(specifier)` syntax,
such as `Bash(cargo test)`. Review active rules with `/permissions`. saaio's `--settings` argument
adds hooks and does not choose a permission mode or add allow rules.

Sources: [Codex configuration reference](https://developers.openai.com/codex/config-reference),
[Claude Code permissions](https://code.claude.com/docs/en/permissions), and
[Claude Code settings](https://code.claude.com/docs/en/settings).
