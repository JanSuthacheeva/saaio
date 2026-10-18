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
