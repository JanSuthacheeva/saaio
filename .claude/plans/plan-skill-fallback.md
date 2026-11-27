# Fix: a Claude Code worker whose kind's skill is missing never starts

Report: finding 1 of the onboarding research (https://claude.ai/artifact/LmNbWC6mM23kkrTSPKduCe,
`saaio status onboarding --full`).

## Report

Expected: a flow whose kind calls a skill the agent does not have still works. The research
assumed this, and docs/configuration.md:106 promises it ("A first prompt that names a skill you
have not installed is harmless, too").
Actual: the first prompt is `/ship-feature <request>`. Claude Code 2.1.332 answers
`Unknown command: /ship-feature`, so no turn runs. saaio shows the flow as `working` /
`starting` with a spinner forever, and the master is never woken.
Steps and environment: saaio 0.3.1, a new user without the personal skills, `saaio start
"<request>"` with the default feature kind.

## Reproduction

A kind whose skill exists nowhere stands in for a missing personal skill. Run it in a scratch
store, in a repository Claude Code already trusts: an untrusted folder stalls on the trust
question first, which is a different finding.

    H=$(mktemp -d); mkdir -p $H/state
    cat > $H/config.toml <<'EOF'
    [kinds.ghost]
    skill = "no-such-skill-xyz"
    phases = [{ name = "build", skills = ["no-such-skill-xyz"] }]
    EOF
    # E: the environment without the session's CLAUDE*/SAAIO*/CODEX* markers, plus
    #    SAAIO_HOME=$H/store XDG_STATE_HOME=$H/state SAAIO_CONFIG=$H/config.toml
    cd <trusted scratch repo>      # git init, one commit, Claude Code's trust accepted once
    E saaio start --kind ghost --name ghost2 "say hi in one word"
    E saaio server &
    E saaio status                 # 2 min later
    #   ghost2,ghost,working,"","",2m,"",""
    cat $H/store/flows/<id>.jsonl  # only started + two workspace events: no hook event at all
    E saaio --stop

The pane, seen through the same command in a PTY in that worktree:

    claude --session-id <uuid> "/no-such-skill-xyz say hi"
    #   ❯ /no-such-skill-xyz say hi
    #   ⏺ Unknown command: /no-such-skill-xyz
    #   ⏺ Args from unknown skill: say hi
    #   ❯

Fails every run. Codex under the same prompt (`codex exec "$no-such-skill-xyz say hi in one
word"`) answers `Hi`: it passes an unknown `$skill` to the model as text, so Codex is not
affected.

## Root cause

`src/config/kind.rs:88` `Kind::prompt_for` always writes the kind's skill as a slash command at
the start of the first prompt ("{skill} {request}" -> `/ship-feature ...`), and
`src/cli/commands/start.rs:106` records it in the `started` event without asking whether Claude
Code has that skill. Claude Code reads a prompt that starts with `/` as a command. When it knows
no command or skill of that name, it prints `Unknown command` locally and runs no turn, so no
`UserPromptSubmit` or `Stop` hook ever reaches saaio. The flow keeps the `working` state a fresh
start folds to, and since it never stops, `deliver` has nothing to wake the master with.

Proven by the reproduction above: no hook line in the flow's log after 2 minutes, and the pane's
`Unknown command`. A plugin skill called without its namespace runs (`/brainstorming` ran
`superpowers:brainstorming` and answered `ok`), so plugins count as having the skill.

The slash call came in with kinds (452b682 "plans: kinds-of-flows" and the build after it; the
feature kind's `/ship-feature` dates back to the first worker prompt). The docs' "harmless" line
came in with 35d8170. Both assumed Claude Code passes an unknown command to the model, which
2.1.332 does not.

## Fix

Ask before the prompt is recorded. `saaio start` checks whether the agent has the skill, and if
not, records a built-in prompt that names no skill.

- New `src/skills.rs`: which skills Claude Code can call by name. It looks where Claude Code loads
  them, and counts a skill as present when any of these exists:
  - `<claude home>/skills/<name>/SKILL.md` and `<claude home>/commands/<name>.md`
    (`Env::claude_home`: `$CLAUDE_CONFIG_DIR`, else `~/.claude`)
  - the same two under `<repo>/.claude/`
  - `<claude home>/plugins/cache/*/*/*/skills/<name>/SKILL.md` and `.../commands/<name>.md`,
    any installed version. This leans to "present": a disabled plugin's skill only leaves the
    old stall, which the later stall flow catches.

  ```rust
  /// Where Claude Code finds skills by name: its home and the repository.
  pub struct ClaudeSkills { home: PathBuf, repo: PathBuf }
  impl ClaudeSkills {
      pub fn new(home: PathBuf, repo: PathBuf) -> ClaudeSkills;
      /// True when `/name` calls a skill or command Claude Code has.
      pub fn has(&self, name: &str) -> bool;
  }
  ```

- `src/config/kind.rs`: `Kind` gets the built-in prompt that needs no skill, and `prompt_for`
  takes whether the skill is there:

  ```rust
  /// The first prompt when the agent lacks the kind's skill: names no skill.
  pub fn fallback_prompt(&self) -> String;   // by skill: ship-feature, ship-fix: their own; any other: "{request}"
  /// prompt_for as today when `has_skill`; else, when the prompt would start with the skill's
  /// call, fallback_prompt().
  pub fn prompt_for(&self, agent: &Agent, has_skill: bool) -> String;
  ```

  The built-in texts (open question 1):
  - feature: "Build this: plan it, implement it with tests, commit as you go, then ask before
    you push and open a merge request. {request}"
  - fix: "Fix this bug: reproduce it, find its cause, fix it with a regression test, commit,
    then ask before you push and open a merge request. {request}"

  Only a template whose filled text starts with the skill's call falls back. An agent's own
  `prompt`, or a kind prompt with `{skill}` later in the text, is sent as written, because
  Claude Code reads a slash only at the start.

- `src/cli/commands/start.rs` `run`: for an agent with the Claude Code adapter and a kind with a
  skill, build `ClaudeSkills` from `ctx.env.claude_home()` and the repo root, and pass
  `has(skill)` to `prompt_for`. Codex and agents without an adapter always pass `true`.
- `src/cli/output/start.rs` `started`: takes `missing_skill: Option<&str>` (set only when the prompt fell back) and then prints
  `skill: ship-feature not found, the worker got saaio's built-in prompt`. It adds a help line:
  `Install the ship-feature skill for Claude Code, or set [kinds.feature] prompt
  (docs/configuration.md#without-matching-skills)`.
- `docs/configuration.md` "Without matching skills": replace the "harmless" paragraph. Claude Code
  answers `Unknown command` to a slash command it lacks and runs no turn, so `saaio start` checks
  for the skill and sends a built-in prompt instead, and says so. Codex passes an unknown `$skill`
  to the model as text. A kind's own `prompt` is sent as written.
- CHANGELOG.md `## [Unreleased]` Fixed: one line.

## Also affected

- The new-flow dialog runs `saaio start` as a job (`src/ui/app/effect.rs:53`), so it gets the
  fallback too. It does not show the start output, so the person sees no note there.
- Flows already started with a missing skill stay stuck. Nothing is stored wrong: `saaio remove`
  and a new start fix them.
- A Claude Code bundled command used as a kind's skill (e.g. `simplify`) is not on disk, so it
  falls back. No built-in kind does this.
- `src/launch/pane.rs:120` gives flows from before kinds the feature prompt with the skill, unchecked.
  Those flows were recorded before 0.2 and are resumes in practice. Left as it is.

## Assumptions

- The skill lookup is a new top-level module `skills.rs`. It sits beside `config` and is read by
  `cli/commands/start.rs`, not by `adapter` (which stays pure).
- The check runs at `saaio start` on the machine that starts the flow. The server runs the pane
  with the same home, so the answer holds for the pane.
- The output key is `skill`, printed only when the skill was missing.
- Built: tasks 2 and 3 share one commit, since `skills.rs` is dead code (a lint error) until
  `saaio start` calls it. The CLI tests' `Lab::install_skill` puts a skill in their Claude home, so
  the tests of the ordinary start keep calling the skill.
- End-to-end after the fix: `saaio start` printed the `skill:` line and its help line, the log
  recorded `"prompt":"{request}"`, and 6 s after `saaio server` it had `answer "Hi"` and `idle`
  from the hooks; `saaio status` showed `ghost2,ghost,idle`. No separate `working` line: a fresh
  start already folds to working, so the hook adds none.
- The regression test for `prompt_for` goes in `src/config/kind.rs`'s tests. The lookup's tests
  go in `skills.rs` with a temp folder. A CLI test in `tests/cli/` checks the start output with
  `CLAUDE_CONFIG_DIR` pointing at an empty folder.

## Open questions

1. The built-in fallback texts for feature and fix (above): should they end with "ask before you
   push and open a merge request", or push and open it unasked? - recommended: ask first. The
   personal skills ask before the push too, and a new user should not get an unasked push.
   Answer (the user, through the saaio master, 2026-11-27): yes, ask first.
2. Should a kind's fallback be configurable (a new `[kinds.*]` key)? - recommended: no. A kind's
   existing `prompt` key already lets the user write a prompt without a skill, and the help line
   points there. Answer (the user, through the saaio master, 2026-11-27): no new key.

## Tasks

- [x] Add `falls_back_without_the_skill` in `src/config/kind.rs` tests (fails today: does not
  compile, since `prompt_for` takes no `has_skill`; with the argument added and ignored, fails
  with `left: "/ship-feature {request}"`). Then add `Kind::fallback_prompt` and the
  `has_skill` argument, and update the callers to pass `true`.
- [x] Add `src/skills.rs` (`ClaudeSkills`) with tests for the home, repo, commands and plugin
  cache places and a missing name, and register it in `lib.rs` and CLAUDE.md's architecture list.
- [x] Wire it into `saaio start` and its output (`skill:` and the help line), with a CLI test in
  `tests/cli/` that starts a flow under an empty `CLAUDE_CONFIG_DIR` and sees the fallback prompt
  in the log and `skill:` in the output.
- [x] Correct docs/configuration.md "Without matching skills", and add the CHANGELOG line.
- [x] End-to-end: run the reproduction again. Expected: `saaio start` prints `skill:
  no-such-skill-xyz not found ...`, the pane gets the built-in prompt and answers, the flow's
  log shows `working` then `idle` from the hooks, and `saaio status` shows `ghost2` `idle`, not
  `working`.

## Review

Isolated review of main...flow/skill-fallback (c091a6a, then the fix below).

**Fixed**

- `saaio start` printed `skill: <skill> not found, the worker got saaio's built-in prompt` and the
  `[kinds.*] prompt` help line whenever Claude Code lacked the kind's skill, even when the prompt
  was sent as written (a kind prompt with `{skill}` later in the text, or an agent's own prompt).
  Shown by `start_says_nothing_of_the_skill_when_the_kinds_prompt_is_sent_as_written`
  (tests/cli/kinds.rs). `missing_skill` now reports the skill only when the prompt actually fell
  back; docs/cli.md says so. e3b1caf.

**Open**

- Skill lookup by folder name: `ClaudeSkills::has` looks for `skills/<name>/SKILL.md`. If Claude
  Code names a skill by its frontmatter `name` when that differs from the folder, the check can
  be wrong either way. Recommend: verify against Claude Code once; if it uses frontmatter, read
  the `name:` line too. Not proven either way.
- Project skills are looked up under the main checkout's `.claude`, while the worker runs in the
  worktree (`.saaio/worktrees/<name>`). An untracked `.claude/skills` exists only in the main
  checkout; whether Claude Code in the worktree still finds it (by walking up) decides whether the
  check matches. Recommend: check once in the E2E setup; low impact, since a wrong "present" only
  leaves the old stall.
- Known and accepted by the plan: a disabled plugin's skill counts as present; the new-flow dialog
  shows no note; flows started before the fix stay stuck until removed.

**Where to look**

1. `src/config/kind.rs` `prompt_for` -> `fallback_prompt`: when a prompt falls back (only when
   it starts with the skill's call) and the two built-in texts.
2. `src/cli/commands/start.rs` `missing_skill`: the Claude Code-only gate and the
   fell-back condition that drives the output.
3. `src/skills.rs` `has`: which places count as "Claude Code has it".

Routine, skim: docs, CHANGELOG, the `true` added at existing call sites, the test-only
`Lab::install_skill`, and the server test that now waits for both idle marks.

**Checks**

fmt, clippy (`--all-targets --all-features -D warnings`) and `cargo test --all-features` pass at
e3b1caf. The fix was reviewed by this same agent in a separate pass, not independently. No security
review tool ran: the diff takes no untrusted input (only file-existence checks on the user's own
config's skill name).
