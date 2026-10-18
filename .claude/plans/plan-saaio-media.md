# Plan: README picture and GIF after the rename

The README picture and GIF still show the old `flow` wordmark in the sidebar. Re-record them with
the installed post-rename `saaio` (built after PR #39), under the names `saaio-ui.png` and
`saaio-ui.gif`.

## Approach

- Rename with `git mv`, so the old files go and history follows; then overwrite with the new takes.
- `record.sh` writes the new names; its header says so. `demo-setup.sh` and `alacritty.toml` have no
  other leftover flow names (`SAAIO_FLOW_ID`/`SAAIO_FLOW_NAME` are real variables).
- References: `README.md` (2), `docs/ui.md` (1). Plans and design pages under `.claude/plans/` and
  `docs/design/` are records of past work and keep the old names.
- Recording: `demo-setup.sh` into a scratch folder in the scratchpad (repo + state, so
  `SAAIO_HOME` is scratch), `record.sh open`, the master asked through the private tmux server,
  the agents' first-run questions answered in their panes, `record.sh capture` once the workers
  stand in different places. Five agents at most (master, three Claude Code, one Codex), nothing
  else running; afterwards `saaio ui --stop` on the scratch store, the tmux server killed, the
  scratch folder removed.
- A take is kept only if no visible text says `flow` as the product (wordmark, master's answer,
  prompt suggestion); otherwise retake.
- No CHANGELOG entry (nothing before 0.1.0); no ROADMAP change.

## Tasks

- [x] Rename the media files and update `record.sh`, `README.md`, `docs/ui.md`
- [x] Record the picture and GIF on a scratch store and check every frame for the old name
- [x] Clean up the scratch session, store and repo
