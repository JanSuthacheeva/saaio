# Changelog

All notable changes to flow are listed here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and flow uses
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

The first public release, 0.1.0. This heading becomes `## [0.1.0] - <date>` when it is tagged.

### Added

- flow ui: the master, every worker and every plain session in one terminal, with the sidebar,
  the peek, the board and the new-flow dialog.
- flow server: sessions outlive the UI, and resume after a crash or a reboot.
- Claude Code and Codex workers, followed through their own hooks; any other command as a worker
  that flow only watches run.
- A configurable workflow: 1 to 4 phases and the skills that start them, and every agent's first
  prompt.
- The master: it starts and answers workers, and is woken by their news.
- Guided and auto flows, and plain sessions.
- Per-worktree setup and teardown scripts, `FLOW_SLOT` for ports, and branch conventions per
  repository.
- Themes: the terminal's own colors, flow's dark and light themes, live reload, themes another
  tool writes.
- The command line for agents (TOON) and `flow status --json` for programs.
- Commented example `config.toml` and `theme.toml`, written on the first run.

### Fixed

- flow ui no longer crashes when its terminal shrinks, such as a window manager resizing it, while
  it draws.

### Known issues

- No desktop notifications: a waiting flow shows only in the sidebar and to the master.
- A Claude Code session interrupted with Esc keeps its spinner until its next prompt, since Claude
  Code runs no hook on an interrupt.
- Codex flows show no task counts, and are ended from flow ui or with `flow remove`, since Codex
  has no exit event.
- Codex asks once to trust flow's hooks, and again after they change.
- A Codex worker that reads a later skill's `SKILL.md` early seems to start that phase early.
- A Codex worker asks permission to commit in its worktree, since its sandbox cannot write to the
  main repository's `.git`.
- Background subagents are not counted: a worker running several shows only the first, and only
  once its turn ends.
- The new-flow dialog starts flows in the repository of the session in view.
- macOS and Linux only.
