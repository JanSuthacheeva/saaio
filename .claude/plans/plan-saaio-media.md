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

## Decided after the second review

The user chose to fix the peek in this PR, then retake the GIF.

- [x] The peek writes paths in its tool lines short: `<worktree>/` dropped, so `Write(PLAN.md)`,
  and the home folder as `~` elsewhere. `fit::shorten(text, folder, home)` beside `fit::tilde`,
  applied in `Sidebar::peek_rows` to `PeekLine::Tool` targets and `PeekLine::Now` calls with the
  flow's `workspace.path`; tests in `fit` and the sidebar
- [x] Found in the retake: the peek shortens result lines and text too (a Codex `pwd` printed the
  home path), and reads this Codex's code-mode calls: `exec_command({"cmd":...})` with a quoted key,
  and `tools.apply_patch(...)` as `Edit(<first file>)` instead of an empty `exec()`
- [x] Retake the GIF (and picture) with that build first on `PATH` for the recording only, check
  every frame for the old name, home paths and personal setup
- [x] Clean up the scratch session, store and repo

## Review

Third review of `flow/media-saaio` against `main`: the commits after the second review
(2072872..830b15c) against "Decided after the second review", and the branch's media once more. I
viewed the PNG at full size and the GIF at 1 s steps, and ran OCR over every distinct GIF frame
(`mpdecimate`, 204 frames) and the PNG for the old name, home paths, temp paths, MCP calls and
plugin names. Nothing matched: every `flow` is the concept or the `flow/` branch prefix, and every
path is `~/...` or relative (`Edit(PLAN.md)`, `Bash(sed -n '1,260p' .agents/skil...)`).

**Fixed**

- `docs/ui.md` did not say the peek now writes paths short, a change a user sees (the project's
  rule: user docs change in the same PR). One sentence under "The peek". 8ee5628.

**Open**

- The GIF shows Codex's footer `2 warnings · f2 to view`. Their text is not in any frame, so
  nothing leaks, but they may come from the recorder's own `~/.codex/config.toml`.
  Recommendation: leave it; look at `f2` in Codex once if you want a clean footer next take.
- The Claude Code header shows the plan (`Opus 5.5 · Claude Team`), no name or email. Fine to keep.
- A Codex `apply_patch` shows as `Edit(PLAN.md)` with the result `⎿ {}`, the script's empty
  return value. Cosmetic; could be dropped in `peek/codex.rs` later.

**Where to look**

1. `src/ui/fit.rs` `shorten`: plain substring replace of `<worktree>/` then `<home>/`; a bare
   worktree path (no trailing slash) shows as `~/...`. `$HOME` empty is filtered in `cli/env.rs`.
2. `src/ui/peek/codex.rs` `patched`: the first `*** Add/Update/Delete File:` header, cut at `\`,
   `"` or a newline, only when the script calls `tools.apply_patch(`.
3. `src/ui/sidebar/rows.rs` `peek_rows`: Tool, Result, Text and Now lines all go through
   `shorten`; routine. `record.sh` (raise first, refuse a GIF if the window moved) and
   `demo-setup.sh` (`"tui": "fullscreen"`): routine.

**Checks** - `cargo test` and `cargo clippy --all-targets -- -D warnings` last passed at 7b7d905,
the last commit touching Rust; later commits change only docs, media and the plan, and were not
rerun.
