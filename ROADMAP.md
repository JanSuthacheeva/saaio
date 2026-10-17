# Roadmap

This is what comes after 0.1.0, as things stand today. Plans change. What has already shipped is in
[CHANGELOG.md](CHANGELOG.md).

## Implementing

Nothing is in progress right now.

## Planned soon

- More agents. Today flow follows Claude Code and Codex through their hooks; other harnesses should
  get the same treatment.
- Desktop notifications. When a flow starts waiting or gets blocked while you're looking elsewhere,
  flow ui should have your terminal notify you, with a switch in `config.toml`. For now a waiting
  flow only shows in the sidebar and to the master.
- Every subagent in the sidebar. A worker running several background subagents just looks busy,
  and only the first subagent shows, once the turn ends. The sidebar should count every running
  subagent, and the board should list their tasks.
- Working across repositories. The new-flow dialog starts flows in the repository of the session
  you're looking at. You should be able to pick the repository, the sidebar and board should say
  which repository a flow belongs to, and `flow start --repo <path>` should save you a `cd`.
- Codex phases from the skill it actually starts. A Codex worker that reads a later skill's
  `SKILL.md` early seems to start that phase early.
- Codex commits without asking. Codex's sandbox can't write to the main repository's `.git`, so a
  Codex worker asks permission for every commit in its worktree.
- The first prompt read when a flow starts. A change to `[worker] prompt` only takes effect after a
  new `flow ui`, while `[worker] agent` applies right away.
- `flow example config` and `flow example theme`, which print every setting with its default, for
  when your own file has drifted from the examples flow wrote on its first run.

## Planned future

- The master asking a worker. A `flow ask` would put the master's question to a worker and bring
  the answer back, without waiting on you.
- An interrupted Claude Code turn shown as idle. Claude Code runs no hook when you press Esc, so the
  worker keeps its spinner until its next prompt.
- Task counts and an end event for Codex. Codex has neither, so a Codex flow shows no task counts
  and has to be ended from flow ui or with `flow remove`.
