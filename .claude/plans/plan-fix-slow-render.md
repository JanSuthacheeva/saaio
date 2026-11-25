# Fix: a pane paints top to bottom, slowly, when the machine is loaded

Report: "When the laptop is low on resources, the saaio UI visibly paints a pane from top to
bottom, very slowly, instead of showing each frame at once. Observed inside tmux 3.6b
(TERM_PROGRAM=tmux); not yet known whether it happens outside tmux." The reporter pointed at what
exists: ui.rs wraps each draw in BeginSynchronizedUpdate/EndSynchronizedUpdate,
server/terminal.rs ends a program's synchronized update past its timeout, and the server sends at
most one frame per 16 ms (FRAME_EVERY) with only changed rows.

## Report

Expected: each redraw of a pane's program shows at once, as it does in a terminal that holds
synchronized updates.
Actual: under load the pane is drawn in pieces from top to bottom, visibly slowly.
Steps and environment: saaio 0.3.1 in tmux 3.6b in Alacritty (`TERM=xterm-256color`, tmux's
`client_termfeatures` without `sync`), macOS on a MacBook Air (8 cores), Claude Code workers.

## Reproduction

The worktree's release build with temporary probes (removed again): the server logged each frame
it sent (rows, whether the pane's program was inside a synchronized update when it was sent) and
the sync markers in the focused pane's output; the UI logged each draw (write calls, bytes, time).
Setup per run: `SAAIO_HOME=$(mktemp -d)`, `XDG_STATE_HOME` inside it, `SAAIO_CONFIG` with

    [agents.master]
    command = ["claude", "--model", "haiku", "Write a numbered list from 1 to 120, each item one short sentence about the sea. No tools."]
    prompt = ""

the UI at 200x51 either in a separate tmux server (`tmux -L srui -f /dev/null new-session -d -x
200 -y 51 "<env> saaio"`, attached by a recorded tmux client in a pty) or directly in a recorded
pty; CPU load from N `yes > /dev/null` processes; 40 s per run; `saaio --stop` after each.

1. Claude Code never sends synchronized output in a saaio pane. In 533 KB of its output under
   the current build: 0 `CSI ? 2026 h`.
2. Claude Code turns it on once the terminal answers XTVERSION. Claude Code run directly in a pty
   that answers its queries (`reply with the word hi`):

       no XTVERSION answer, TERM_PROGRAM unset       2026h 0   DECRQM asked 0
       XTVERSION "saaio 0.3.1", DECRQM 2026 -> 2      2026h 25  DECRQM asked 1
       same, TERM_PROGRAM=tmux                       2026h 27  DECRQM asked 1
       no answer, TERM_PROGRAM=ghostty               2026h 21

   Claude Code sends `CSI > 0 q` (XTVERSION) first, then `CSI ? u`, an OSC, and `CSI c` (DA) last;
   saaio's emulator answers DA, DSR and DECRQM 2026 (`answers_the_queries_claude_code_sends`) but
   not XTVERSION, so Claude Code never asks DECRQM and draws without the markers.
3. What reaches the UI. With the server patched to answer XTVERSION but to drop the markers
   before the emulator (so the redraws are counted but nothing is held, as today), frames sent
   while a Claude Code redraw was half written:

       tmux,   no load:   160 redraws, 166 frames sent mid-redraw
       tmux,   48 x yes:  178 redraws, 180 frames sent mid-redraw
       direct, 48 x yes:  274 redraws, 164 frames sent mid-redraw
       tmux,  200 x yes:    8 redraws,   8 frames sent mid-redraw

   A redraw arrives in about 3 reads (the macOS pty passes 1024 bytes at a time). Nearly every one
   is shown half drawn: its first read comes after the pane was quiet for more than 16 ms, so
   `flush` sends it at once. Without load the rest follows within one frame and nobody sees it;
   under load the program and the server wait for the CPU between reads, and the pane shows its
   redraw growing from top to bottom.

Fails on every Claude Code redraw, in tmux and directly in a terminal; visible only under load.

Measured on the side and ruled out as the cause:

- Frame size and full frames. With Claude Code the server sends only damaged rows (avg 15.7 rows
  of 47) and full frames only on attach, focus and resize (2 in 40 s). The UI writes p50 1.1 KB,
  p90 1.7 KB per draw; ratatui's diff keeps it small.
- UI draw time. 0.3 to 1 ms of work; the rest is the write.
- tmux and the UI's own markers. tmux 3.6b does not hold a pane program's synchronized update: a
  program's `CSI ? 2026 h`, top line, 2 s pause, bottom line, `CSI ? 2026 l` reached the attached
  client as two pieces 2 s apart, with and without `terminal-features '*:sync'`. With a 1024-byte
  pty, any UI draw over 1 KB therefore reaches the outer terminal in pieces inside tmux. A
  synthetic 12.6 KB draw (every row recoloured) took p90 20 ms, max 509 ms under 200 x yes in tmux;
  directly in a terminal, max 149 ms, held by the terminal's own sync. With Claude Code the draws
  are mostly one piece, so this is not what the report shows; it remains for big draws (a focus
  switch) in tmux, see Also affected.

## Root cause

`src/server/terminal.rs:117` `Terminal::feed` hands a program's output to alacritty_terminal,
whose parser (vte 0.15) has no handler for XTVERSION (`CSI > q`, `CSI > 0 q`), so the query goes
unanswered. Claude Code asks XTVERSION and only then DECRQM 2026, so it decides the terminal
cannot hold frames and writes each redraw unsynchronized, although the emulator honours
`CSI ? 2026 h/l` (vte buffers until the end, `expire_sync` ends it past 150 ms). The server then
flushes whatever part of a redraw has arrived, and under load the parts are far apart.

Proven by the experiment above: answering XTVERSION alone makes Claude Code send the markers (25
to 27 synchronized redraws where there were 0), the emulator then holds each redraw until it is
whole, and the frames sent mid-redraw in step 3 are exactly the ones the markers would hold.
`TERM_PROGRAM=tmux`, which panes inherit from a server started inside tmux, does not stop it.
The query was never answered: `answers_the_queries_claude_code_sends` came with the emulator in
bc008f5 and covers DA, DECRQM 2026 and DSR.

## Fix

Built on flow/osc-7501 (merges first), whose `program_status::Scanner` already runs over each
chunk `Terminal::feed` gets, keeps a sequence split across reads, and has `feed` answer the OSC
7501 query in output order (it advances the parser up to each found sequence's end, then appends
the answer to the replies). XTVERSION goes into that scanner instead of a scanner of its own:

    // src/program_status.rs
    /// What saaio answers to XTVERSION (`CSI > q`, `CSI > 0 q`): its name and version.
    pub const VERSION_ANSWER: &[u8] =
        concat!("\x1bP>|saaio ", env!("CARGO_PKG_VERSION"), "\x1b\\").as_bytes();

    pub enum Sequence {
        Query,
        /// XTVERSION: the program asks the terminal's name and version.
        Version,
        Status(Option<Report>),
    }

    enum Scan {
        ..., // as now
        /// After `ESC [`: the bytes of a CSI that may be XTVERSION so far (`>`, then `0` or none).
        Csi,
    }

`Scanner::escape` opens `Scan::Csi` on `[`; in `Csi`, `>` and then at most one `0` keep it open, a
`q` right after `>` or `>0` ends it as `Found { end, sequence: Sequence::Version }`, ESC goes to
`Escape`, and any other byte ends it unfound (`Ground`). The scanner keeps what it has seen of the
CSI across reads as it does for an OSC. `Terminal::feed`'s match gets
`Sequence::Version => replies.extend_from_slice(VERSION_ANSWER)`, so the answer leaves before the
DA answer that follows the query, as the OSC 7501 answer does. The module doc and CLAUDE.md's
program_status line name XTVERSION beside OSC 7501.

`fuzzing::program_status` asserts every found sequence ends on a terminator (`BEL` or `\`); it
gets `q` for a `Version`, and its split property then covers the CSI state too.

Why there: the terminal is what the program asks. The server's flush, its 16 ms frame and the UI
are right to send what the screen shows; holding frames by guessing where a program's redraw ends
would be a longer interval by another name.

## Also affected

- Codex and other programs that ask XTVERSION before using synchronized output get the same
  answer; programs that do not ask are unaffected.
- tmux does not hold the UI's own synchronized updates (proven above), and a macOS pty passes
  1024 bytes at a time, so a UI draw over 1 KB (a focus switch, a resize, scrolling history) can
  still show in pieces inside tmux under load. Not fixable in saaio short of drawing fewer bytes
  (scroll regions); directly in a terminal that holds synchronized updates it does not happen.
- Panes inherit `TERM_PROGRAM=tmux`, `TERM_PROGRAM_VERSION` and `TMUX` from a server started in
  tmux, so a pane's program believes it runs in tmux when its terminal is saaio. Not needed for
  this fix (step 2); decision 1 puts it on the roadmap.
- No data is written wrong.

## Assumptions

- The answer names `saaio` and the package version (`saaio 0.3.1`), the form tmux, ghostty and
  WezTerm use; Claude Code accepts it.
- XTVERSION lives in `program_status.rs` although the module is named for OSC 7501: it is the
  scanner for a program's queries to its terminal, and the master decided against a file of its
  own.
- 8-bit CSI (0x9b) is not looked for; Claude Code and Codex send the 7-bit form.
- CHANGELOG.md gets a Fixed line under Unreleased.

## Decisions

1. Panes keep the `TERM_PROGRAM`, `TERM_PROGRAM_VERSION`, `TMUX` and `TMUX_PANE` they inherit in
   this fix; giving them `TERM_PROGRAM=saaio` goes into ROADMAP.md (Planned soon) as its own item.
2. No docs note that large redraws can still show in pieces inside tmux.
3. XTVERSION is answered by flow/osc-7501's `program_status::Scanner`, not a separate
   `xtversion.rs`. This branch stands on flow/osc-7501; its pull request opens after osc-7501
   merges, rebased onto main.

## Tasks

- [x] Add `\x1b[>0q` first, as Claude Code sends it, to `answers_the_queries_claude_code_sends` in
  `src/server/terminal.rs` (fails today: the replies start with `\x1b[?6c` where
  `\x1bP>|saaio 0.3.1\x1b\\` is expected), then add `Sequence::Version`, `Scan::Csi` and
  `VERSION_ANSWER` in `src/program_status.rs` and answer it in `Terminal::feed`;
  `fuzzing::program_status` accepts `q` as the end of a `Version` (moved here from the next task,
  since its property test would fail on a random `ESC [ > q` without it)
- [x] Scanner tests in `src/program_status.rs`: `CSI > q` and `CSI > 0 q` found with their end,
  one split anywhere found once, `CSI > 1 q`, `CSI > 0 c` (DA2), `CSI ? u` and `>0q` without
  `ESC [` not found, an OSC 7501 query right after an XTVERSION query both found in order
- [x] A test in `src/server/terminal.rs` that a redraw inside `CSI ? 2026 h ... l` fed in two parts
  changes no row until its end, so the emulator holds what Claude Code now marks
- [x] CLAUDE.md (program_status line), CHANGELOG.md (Fixed, under Unreleased), ROADMAP.md
  (Planned soon: panes say their terminal is saaio, not the tmux the server started in)
- [ ] End-to-end: rerun the reproduction (tmux and direct, 48 x yes, the haiku list, with the
  frame probe) and see Claude Code's output carry `CSI ? 2026 h` (over 100 in 40 s, today 0), and
  no frame whose rows show a half-written redraw: every frame sent while the program is inside a
  synchronized update carries no changed content; remove the probe again
