# Plan: README picture and GIF of the finished UI

`docs/media/saaio-ui.png` and `saaio-ui.gif` were taken before the plain goose mark and the agent
names in the sidebar and on board cards. Retake both with the installed `saaio` (655e5bc; `main`
after it changed only docs), so they show bare `saaio`, the plain goose, agent names, a Claude Code
and a Codex worker, and phases moving.

## Approach

- The existing `docs/media/demo-setup.sh`, `record.sh` and `alacritty.toml`, unchanged unless the
  take shows they need it: Alacritty, a throwaway repo and a scratch `SAAIO_HOME` in the
  scratchpad, never the real store; Claude Code without user settings and MCP servers.
- Three workers, not four, to keep the load and the Codex use down: two Claude Code (one auto, one
  guided that stops to ask), one Codex with a request small enough for one short turn.
- `capture` while the Claude Code workers still walk through their phases, so the GIF shows them
  move.
- Check the PNG and every distinct GIF frame (OCR over `mpdecimate` frames): no home path, no
  username, no personal plugin or MCP call, the product called saaio everywhere, no `saaio ui`.
- No CHANGELOG entry (nothing before 0.1.0), no ROADMAP change. `.lavish` pages never committed.

## Tasks

- [x] Record the picture and GIF on a scratch store
- [x] Check every frame for home paths, the username, personal setup and the product name
- [x] Clean up the scratch session, store and repo

## Found in the take

- The workers' answers quoted the recorder's own `~/.claude/CLAUDE.md` ("commit with the /commit
  skill"): the demo env sets `CLAUDE_CODE_DISABLE_CLAUDE_MDS=1`. Claude Code's tmux scrolling hint
  showed: the env unsets `TMUX`. The workers committed `__pycache__`: the sample repo ignores it.
- Run from an agent session, `record.sh open` passed that session's markers
  (`CLAUDE_CODE_CHILD_SESSION`, its messaging socket) to the demo's agents, and Claude Code turned
  off transcript saving: the private tmux server now starts without them.
- A Claude Code worker walks all four phases in about a minute, before a capture can follow: three
  flows first, then a fourth right before `capture`, so the GIF shows it move. The scripts' printed
  steps say so.

## Review

Reviewed `flow/media-reshoot` against `main` in a context that did not write it. No Rust changed,
so there is no suite to run; both scripts pass `bash -n` and were read, not run.

- **Fixed** - `demo-setup.sh`'s header comment ran one line to 130 columns after the CLAUDE.md
  note was added; rewrapped at 100 like the rest (65154a3).
- **Media** - the PNG and all 194 distinct GIF frames (`mpdecimate`) went through OCR and a few
  were viewed: no home path but `~/projects/shop`, no username, no personal plugin, MCP call or
  instruction, the product is saaio throughout, no `saaio ui`. `flow/shop-217` is saaio's own
  default branch template, not an old name. The header shows the account's plan ("Claude Team"),
  which is harmless.
- **Open** - `record.sh open` unsets every `CLAUDE_CODE_*` variable to keep a parent session's
  markers out; a recorder who logs in through one (`CLAUDE_CODE_OAUTH_TOKEN`, `CLAUDE_CODE_USE_BEDROCK`)
  would get logged-out agents. Leave as is unless someone records that way; the failure is
  visible at once. Not this branch: the worker peek and the board's last answer show Markdown
  raw (`**plain text** or **HTML**`, `**CSV:**`), visible in the GIF; worth its own fix in the UI.
- **Where to look** - `record.sh` `open_ui()`: which variables the tmux server starts without;
  then the `env` file `demo-setup.sh` writes (`CLAUDE_CODE_DISABLE_CLAUDE_MDS`, present in Claude
  Code 2.1.302, and `unset TMUX`). The rest, the printed steps and the media, can be skimmed.
- **Checks** - no code changed since `main`; `bash -n` passes on both scripts at 65154a3.
