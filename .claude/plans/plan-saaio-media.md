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

## Review

Reviewed `flow/media-saaio` against `main` (commits 69dbde1..1225991). I viewed the PNG at full size and every distinct GIF frame (14 frames after `mpdecimate`, plus samples every 1.5 s).

**Fixed** - nothing. The renames, `record.sh`, `README.md` and `docs/ui.md` are correct. Outside the old plans and design pages, which keep the old names on purpose, nothing still points to `flow-ui.png`/`flow-ui.gif`. The wordmark reads `saaio` in the sidebar and on the board in every frame, and the master's answer and the prompts never call the product `flow`.

**Open**

- The GIF shows Jan's own Claude Code setup and his home path. At about 17-19 s, the shop-198 pane shows a `plan-progress - report (MCP)` call with `plan: "/Users/jansuthacheeva/projects/shop/.saaio/worktrees/shop-198/PLAN.md"`. Everything else in the media shows `~/projects/shop`. This comes from `demo-setup.sh`, which isolates only `SAAIO_HOME`/`XDG_STATE_HOME`: the agents run with the recorder's own plugins and MCP servers. Recommendation: retake the GIF with `plan-progress` (and any other personal plugin) disabled for the recording before the README goes public. Have `demo-setup.sh`/`record.sh` say that the agents use your own config. The PNG is clean.
- The title row and the board show `flow/shop-198`. That is the built-in branch prefix (`src/branching.rs`), a flow being saaio's word for a unit of work, not the old product name. It is acceptable under the plan's rule. Change it only if the default prefix itself changes.

**Where to look**

1. `docs/media/saaio-ui.gif` from about 17 s to 19 s: the personal MCP call and absolute path.
2. `docs/media/saaio-ui.png`: the README's first impression. It looks clean.
3. The rest (`record.sh` names, the three doc links, the plan) is routine and can be skimmed.

**Checks** - no Rust changed on the branch. `cargo test` and `cargo clippy -- -D warnings` last passed on `main` at 763453d and were not rerun.

## Decided after review

The user chose to retake both with the recorder's own plugins off and to say so in the scripts.

- [x] `demo-setup.sh` starts the Claude Code agents without user-level settings and MCP servers
  (`--setting-sources project,local --strict-mcp-config`), so personal plugins, MCP servers and
  hooks stay out of the recording; `demo-setup.sh` and `record.sh` say the agents otherwise run
  with your own config
- [x] Retake the picture and GIF, check every frame for the old name and for personal setup
- [x] Clean up the scratch session, store and repo again
