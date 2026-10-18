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

## Decided after review

The user chose to retake both with the recorder's own plugins off and to say so in the scripts.

- [x] `demo-setup.sh` starts the Claude Code agents without user-level settings and MCP servers
  (`--setting-sources project,local --strict-mcp-config`), so personal plugins, MCP servers and
  hooks stay out of the recording; `demo-setup.sh` and `record.sh` say the agents otherwise run
  with your own config
- [x] Retake the picture and GIF, check every frame for the old name and for personal setup
- [x] Clean up the scratch session, store and repo again

## Review

Second review of `flow/media-saaio` against `main`: the commits after the first review
(467a6c7..2f8d902) against "Decided after review", and the branch's media once more. I viewed the
PNG at full size, the GIF at 1 s and 0.5 s steps, and ran OCR over every distinct GIF frame
(`mpdecimate`) for the old name, home paths, MCP calls and plugin names.

**Fixed**

- The master still ran with the recorder's own plugins, hooks and MCP servers. It is the built-in
  `master` agent, which `[agents.claude]` does not change; loading the demo config showed
  `master_agent()` as `["claude", "-n", "{name}", "--session-id", "{id}"]`. `demo-setup.sh` now
  sets `[agents.master]` with the same flags; the master is still kept out of the worker agents.
  Nothing personal from the master is visible in this take. 3f0fd85.

**Open**

- The GIF still shows the home path. At about 5 s (frames 77-80 at 15 fps) the peek over shop-198
  shows `Write(/Users/jansuthacheeva/proje...)`: Claude Code writes PLAN.md by its absolute path
  and the peek prints a tool call's argument as given. No plugin is involved, so the flags cannot
  stop it, and the next take can show it again. Recommendation: have the peek (`src/ui/peek/`)
  write `$HOME` as `~`, as the sidebar already does for the master's folder, and retake the GIF on
  that build before the README goes public. A quicker way is to keep the sample repository under
  a folder that has no user name in it, such as `/tmp/shop`, but the sidebar would then show that
  path. The PNG is clean.
- `flow/shop-198` in the title row and on the board is the default branch prefix, not the old
  name (as the first review found).

The fix was checked by me in a separate pass, not by an independent agent: the demo config loads,
the master's argv has the flags, `{prompt}` stays empty, the workers are still claude and codex.

**Where to look**

1. `docs/media/saaio-ui.gif` at about 5 s: the home path in the peek.
2. `docs/media/demo-setup.sh` config.toml: `[agents.claude]` and `[agents.master]` both carry
   `--setting-sources project,local --strict-mcp-config`; Codex still loads `~/.codex/config.toml`
   (the header says so).
3. `docs/media/record.sh` `raise()` and the comments: routine.

**Checks** - no Rust changed on the branch. `cargo test` and `cargo clippy -- -D warnings` last
passed on `main` at 763453d and were not rerun.

## Decided after the second review

The user chose to fix the peek in this PR, then retake the GIF.

- [ ] The peek writes paths in its tool lines short: `<worktree>/` dropped, so `Write(PLAN.md)`,
  and the home folder as `~` elsewhere. `fit::shorten(text, folder, home)` beside `fit::tilde`,
  applied in `Sidebar::peek_rows` to `PeekLine::Tool` targets and `PeekLine::Now` calls with the
  flow's `workspace.path`; tests in `fit` and the sidebar
- [ ] Retake the GIF (and picture) with that build first on `PATH` for the recording only, check
  every frame for the old name, home paths and personal setup
- [ ] Clean up the scratch session, store and repo
