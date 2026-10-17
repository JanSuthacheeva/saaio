# Roadmap

What comes after 0.1.0. Plans change; this page says where things stand today. What already
shipped is in [CHANGELOG.md](CHANGELOG.md).

## Implementing

Nothing is in progress right now.

## Planned soon

- **Desktop notifications.** When a flow starts waiting or is blocked and you are not looking at
  it, flow ui tells your terminal to notify you, with a switch in `config.toml`. Today a waiting
  flow shows only in the sidebar and to the master.
- **Every subagent in the sidebar.** A worker running several background subagents looks merely
  working, and only the first shows, once its turn ends. The sidebar should count every running
  subagent and the board list their tasks.
- **Working across repositories.** The new-flow dialog starts flows in the repository of the
  session in view. It should let you pick the repository, the sidebar and board should say which
  repository a flow belongs to, and `flow start --repo <path>` should save a `cd`.
- **Codex phases from the skill it starts.** A Codex worker that reads a later skill's `SKILL.md`
  early seems to start that phase early.
- **Codex commits without asking.** Codex's sandbox cannot write to the main repository's `.git`,
  so a Codex worker asks permission for every commit in its worktree.
- **The first prompt set when a flow starts.** A change to `[worker] prompt` takes effect only
  after a new `flow ui`, while `[worker] agent` applies at once.
- **`flow example config` and `flow example theme`.** Print every setting with its default, for
  when your own file has drifted from the examples flow wrote on its first run.

## Planned future

- **The master asks a worker.** A `flow ask` that puts the master's question to a worker and
  brings the answer back, without waiting on the user.
- **An interrupted Claude Code turn shown as idle.** Claude Code runs no hook when you press Esc,
  so the worker keeps its spinner until its next prompt.
- **Task counts and an end event for Codex.** Codex has neither, so a Codex flow shows no task
  counts and is ended from flow ui or with `flow remove`.
