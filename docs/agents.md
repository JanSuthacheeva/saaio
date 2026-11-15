# Agent permissions

- [Introduction](#introduction)
- [Claude Code](#claude-code)
    - [Permission modes](#permission-modes)
- [Codex](#codex)
    - [Automatic approval review](#automatic-approval-review)
    - [Command rules](#command-rules)
    - [Phase tracking](#phase-tracking)

## Introduction

A worker runs while you are somewhere else, so every permission prompt it stops at is time it
spends waiting for you. How much a worker may do on its own is a choice that belongs to each
harness, not to saaio, and this page recommends settings for workers in Claude Code and Codex. It
was verified with Codex CLI 0.163.0 and Claude Code 2.1.302 on 2026-10-18.

saaio adds only what it needs to follow a worker, on each launch and resume. Everything else you
configure in the harness itself:

| Harness | saaio supplies on each launch and resume | Configure separately |
| --- | --- | --- |
| Codex | Eight reporting hooks and trust for those exact hooks through `-c`, plus trust for the worker's worktree once you [trusted](repository.md#trust) the repository, and no update check at startup. The worker also receives instructions for talking to the master and reading workflow skills only when it begins them. | Approval handling, command rules, sandbox network access, and reasoning effort. |
| Claude Code | Reporting and wait hooks in a `--settings` JSON argument, plus worker instructions through `--append-system-prompt`. | Permission mode and allow rules. |

saaio does not edit `~/.codex` or `~/.claude`. The Codex trust passed by saaio covers its own hooks
and, in a trusted repository, the worker's worktree. Other hooks still follow Codex's normal trust
review. What the hooks report is described in [Flows](flows.md#how-a-worker-reports).

## Claude Code

For fewer routine prompts, use the `auto` permission mode where it is available. Otherwise, use
`acceptEdits` with narrow allow rules for the commands your workers run again and again. Set the
starting mode and rules in `~/.claude/settings.json`:

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

Claude Code's rules use `Tool(specifier)` syntax, such as `Bash(cargo test)`, and `/permissions`
shows the active ones. It also accepts `--permission-mode` for a session.

> [!NOTE]
> saaio's `--settings` argument adds hooks only. It does not choose a permission mode or add allow
> rules.

### Permission modes

| Mode | Behavior for workers |
| --- | --- |
| `manual` | Prompts for commands and edits that have no allow rule. |
| `acceptEdits` | Accepts edits and common file operations in working directories. Other commands can still prompt. |
| `auto` | A background classifier reviews actions that would normally prompt. Availability depends on the account and environment. |
| `dontAsk` | Denies actions that would prompt unless an allow rule covers them. |
| `plan` | Limits work to planning and read-only exploration. |
| `bypassPermissions` | Skips permission prompts and grants broad access. Use only in an isolated environment. |

## Codex

These Codex settings are recommended for workers:

| Setting | Location | Effect |
| --- | --- | --- |
| `--approve-for-me` | Both `command` and `resume` under `[agents.codex]` | Routes approval requests to Codex's automatic review in the workspace-write sandbox. The reviewer can still reject a request. |
| `prefix_rule` | `~/.codex/rules/default.rules` | Allows selected commands without an approval prompt. Match the executable and arguments, not a shell command containing them. |
| `[sandbox_workspace_write] network_access = true` | `~/.codex/config.toml` | Allows outbound network access from the workspace-write sandbox for commands such as `gh`, `git push`, and `npx`. Other approval checks still apply. |
| `model_reasoning_effort = "medium"` | `~/.codex/config.toml` | Uses less reasoning effort for routine work than higher settings. Available levels depend on the model. |

Network access and reasoning effort go in your Codex user settings:

```toml
model_reasoning_effort = "medium"

[sandbox_workspace_write]
network_access = true
```

### Automatic approval review

To have Codex's automatic review answer a worker's approval requests, define both launch commands
in `~/.config/saaio/config.toml` (or your [configured location](configuration.md#file-location)):

```toml
[agents.codex]
command = ["codex", "--approve-for-me", "{prompt}"]
resume = ["codex", "--approve-for-me", "resume", "{session}"]
adapter = "codex"
```

An agent with a built-in name replaces that definition, so keep its adapter. Leave out `prompt`:
an agent's own prompt would replace every kind's
([configuration.md](configuration.md#the-first-prompt)).

### Command rules

A command rule lets one command run without an approval prompt. Add it to
`~/.codex/rules/default.rules`:

```text
prefix_rule(pattern=["cargo", "test"], decision="allow")
```

To check a single command against your rules:

```sh
codex execpolicy check --rules ~/.codex/rules/default.rules cargo test
```

> [!WARNING]
> A rule matches the executable and its arguments, not a shell line that contains them. The rule
> above does not match `/bin/zsh -lc 'cargo test && echo done'`, so a rule for `cargo test` does
> not approve arbitrary shell chains. Give frequently used commands their own narrow rules.

### Phase tracking

Codex phase tracking is experimental. Codex has no Skill tool hook, so saaio treats a shell read
of one configured phase skill's `SKILL.md` as a phase start. This depends on the worker following
the instruction saaio supplies through `developer_instructions`: read each workflow skill only
when beginning it, never ahead. A command reading skills from several phases starts none.

Sources: [Codex configuration reference](https://developers.openai.com/codex/config-reference),
[Claude Code permissions](https://code.claude.com/docs/en/permissions), and
[Claude Code settings](https://code.claude.com/docs/en/settings).
