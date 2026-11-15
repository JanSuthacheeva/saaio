# Fix: the steps stay on review or merge-request while the worker builds a follow-up fix

Report: "once a flow is at the review or merge-request phase and the user asks the worker for
another fix (more implementation work), the sidebar's phase progress does not update: it stays on
review/MR instead of showing the flow back at implement (and then review/MR again)."

## Report

Expected (the user, through the master): when the person asks for code changes after the review
finished (at review or merge-request), the flow goes back to implement; review and merge-request
show as done earlier but stale, and become done again once they run again. Not a step back: the
fixes review-changes makes to its own findings (part of the review), and arguably changes answering
merge request reviewer comments at the MR step (working, no phase change).
Actual: the phase and the steps stay where they were; after the merge request opened even an
explicit `implement-plan` call changes nothing.
Steps and environment: a Claude Code worker on a feature or fix flow, at review or merge-request,
asked in its pane for one more change.

## Reproduction

The real `saaio` binary in a scratch store, fed the Claude Code hook payloads a worker sends
(`saaio hook claude-code` with the flow's key, as its pane runs it); the merge request is reported
with `saaio report` from a scratch repo whose origin is on GitHub. Script:
`/private/tmp/claude-501/-Users-jansuthacheeva-projects-saaio--saaio-worktrees-phase-goes-back/6c7ad996-f11e-46a3-8661-84aa2724b5a8/scratchpad/repro.sh <saaio> <review|mr|mr-skill>`.
It walks a flow through `/ship-feature`, design-code, implement-plan with an Edit, review-changes in
a subagent, then (mr, mr-skill) write-merge-request and the PR; then sends the prompt "Also make the
CSV header bold, please", (mr-skill) an `implement-plan` Skill call, an Edit, and Stop, printing
`saaio status demo` before, during and after.

    review:    phase: review         steps: plan done, implement done, review current, MR open
               (the same before the prompt, while editing and after the turn)
    mr:        phase: merge-request  steps: all four done (the same throughout)
    mr-skill:  phase: merge-request  steps: all four done (the same throughout)

The log of mr-skill after the PR holds no `phase` line at all:

    {"kind":"merge-request","url":"https://github.com/acme/shop/pull/7"}
    {"kind":"answer","text":"PR open: ..."}  {"kind":"idle",...}
    {"kind":"working"}
    {"kind":"answer","text":"Done: header bold"}  {"kind":"idle",...}

Fails every run, all three scenarios.

## Root cause

Two places, both in how phases are derived:

1. Only a skill call moves the phase. `adapter/claude_code.rs:111` `translate` turns a `Skill`
   PreToolUse or a typed `/skill` prompt into `EventKind::Phase` through `Workflow::phase_of`; a
   code change (PostToolUse of Edit, Write, ...) becomes only `Working`. A worker asked for "one
   more change" edits code without calling `implement-plan`, so no event says the work went back to
   implementing (scenarios review and mr).
2. Once a merge request is open, `fold.rs:228` drops every `Phase` event ("A PR fixes the final
   workflow phase", from d36ce19 "keep PR-open workers active until session end"), and
   `finish` (`fold.rs:563`, `is_done = merge_request.is_some() || ...`) marks every step done.
   Because `fold::changes` compares the folded views before and after, `saaio hook` does not even
   append the `phase: implement` line (proven by the mr-skill log above). So even an explicit
   skill call cannot move a flow back after its PR (scenario mr-skill).

A third, shown once 1 and 2 are fixed: `finish` marks a phase after the current one `Done` when it
was entered before (`self.entered[position]`), so a flow back at implement would show review
as done, not as done-but-stale.

## Fix

**A code change is an event.** New `EventKind::Edited { path: String }` (`event.rs`, JSON kind
`edited`): the main agent changed a file. `adapter::claude_code::translate`: a main-agent (no
`agent_id`) PostToolUse of `Edit`, `Write`, `MultiEdit` or `NotebookEdit` yields
`[Working, Edited { path: tool_input.file_path or notebook_path }]`. Subagent edits stay ignored, so
a review in a fresh agent fixing its own findings never steps back. An older binary skips the
unknown kind (`event.rs` already skips unknown kinds), and `fold::changes` appends it only when it
moves the phase, so the log does not grow by a line per edit.

**The fold steps back** (`fold.rs`, `Folder`):

- New field `turn_ended: bool`: false on `enter`, true on `Idle`, on `Waiting` without
  `permission`, and on `Blocked`. It marks that the phase was handed back to the person since it
  began, so an edit in the turn that entered review (review-changes run in the main agent) is part
  of the review.
- New arm `EventKind::Edited { path }` (`Folding::build_to_go_back_to`): steps back to the build
  phase when all hold: the flow's phases have one (`Phase::is_build`), the current phase stands
  after it, `turn_ended`, the flow is not merged, no merge request is open (see open question 1),
  and `path` lies inside the flow's workspace (`view.workspace.path`), so a merge request
  description or an Artifact drafted in a scratch folder is no code change. Stepping back is `self.enter(build, at)`, state `Working`, the
  timeline note `phase: implement (back)`.
- The guard at `fold.rs:228` drops `Phase` events only once the merge request merged, not once it
  opened: an explicit skill call after the PR moves the phase.
- `finish`: `is_done` holds when merged, or a merge request is open and the current phase is the
  last, or (a flow that merges nothing) its state is `Done`. A phase after the current one that was
  entered before is `StepStatus::Stale`; before the current one it stays `Done`.

`workflow.rs`: `pub fn is_build(&self) -> bool` on `Phase`, whether it is the phase named
`implement` (see open question 3). Built as a method of `Phase` rather than
`Workflow::build_phase`, since the fold also folds logs that recorded only phase names or none.

`view.rs`: `StepStatus::Stale`, name `stale`. TOON and JSON print it; `VIEW_VERSION` becomes 4
(see open question 2).

**The UI** draws a stale step as a done step in `theme.ui.faint`: `ui/sidebar/labels.rs` `steps`
(glyph `step_done`, faint; a join reaches it only from a done step, as for open) and
`ui/board/detail.rs` `steps` (glyph `done`, faint). Goldens updated with `SAAIO_UPDATE_GOLDEN=1`
only where a stale step shows.

**Docs and changelog:** `docs/flows.md` Phases (the step back, stale steps, the fourth glyph
look), `docs/json.md` (`stale`, version 4 and its line in Versions), `docs/cli.md` where the steps'
statuses are listed, `CHANGELOG.md` Unreleased: Fixed.

## Also affected

- Codex: its hooks are seen only for Bash and agent spawns (`adapter/codex.rs:160`); an
  `apply_patch` edit is not observed, so a Codex worker steps back only through a skill read
  (which root cause 2's fix makes work after the PR too). See assumptions.
- Data: no log is wrong. Logs written so far hold no `edited` lines, so a flow that already went
  back to work keeps its old look until its next edit; a dropped post-PR `phase` line is gone and
  is not repaired.

## Assumptions

- The edit tools are Claude Code's `Edit`, `Write`, `MultiEdit`, `NotebookEdit`; a file written
  through Bash (`sed -i`, a heredoc) is not seen.
- Any file in the worktree counts, docs and plans included: the person asking for a change to the
  plan after review is sending the flow back to build too.
- A permission prompt does not end the turn for `turn_ended`; `Blocked` does.
- Codex stays without edit detection in this fix; checking whether Codex sends hooks for
  `apply_patch` costs a Codex run (see the Codex budget) and goes to ROADMAP.md Planned soon if
  not done here.
- The stale step reuses the existing glyphs in the faint color; no new theme key.
- Tests: the fold rule in `src/fold/tests.rs`, the translation in `src/adapter/claude_code/tests.rs`
  with a recorded `post_edit.json` fixture shaped like `post_write.json`, the end to end in
  `tests/cli/report.rs` or a new `tests/cli/phases.rs` driving `saaio hook claude-code`.

## Decisions

Approved by the user with the recommendations:

1. A code change once the merge request is open shows only `working`, no phase change (answering
   reviewer comments); an explicit `implement-plan` (or another phase skill) still moves it, since
   root cause 2 is fixed. Before the merge request opens, at merge-request, a change in a later
   turn steps back.
2. The new step status `stale` bumps `VIEW_VERSION` to 4; json.md lists it under Versions.
3. The build phase is the phase named `implement` (the feature and fix kinds have it); a workflow
   without one never steps back on an edit, only on its skills. Documented in configuration.md's
   kinds section.

## Tasks

- [x] Add `steps_back_to_implement_on_an_edit_after_the_review_turn` in `src/fold/tests.rs` (fails
  today: phase stays `review`, steps `[done, done, current, open]`), then add `EventKind::Edited`,
  `Phase::is_build`, `Folding::turn_ended` and the `Edited` arm, with tests that an edit in the
  turn that entered review, an edit outside the workspace and one in a workflow without `implement`
  do not step back (a subagent's edit never reaches the fold: tested in the adapter's task)
- [x] Add `a_phase_skill_after_the_merge_request_moves_the_phase` in `src/fold/tests.rs` (fails
  today: phase stays `merge-request`, every step done), then let `Phase` events through until merged
  and compute `is_done` from the current phase
- [ ] Add `StepStatus::Stale` with a test that review and merge-request show `stale` once back at
  implement and `done` again after they ran again; `VIEW_VERSION` 4 (the two UI matches get their
  stale arms here, since the variant does not compile without them)
- [ ] Translate main-agent Edit, Write, MultiEdit, NotebookEdit PostToolUse into `Edited` in
  `adapter/claude_code.rs`, with a recorded fixture and tests, subagent edits excluded
- [ ] Draw stale steps faint in the sidebar and the board detail; update the goldens it changes
- [ ] End-to-end test in `tests/cli/` driving `saaio hook claude-code` through the reproduction's
  review scenario, asserting `phase: implement` and the stale steps
- [ ] Docs: flows.md, json.md, cli.md, configuration.md; CHANGELOG Fixed; ROADMAP Planned soon for
  Codex edit detection
- [ ] End-to-end: run the reproduction again with the new binary and see, for review: while
  editing `phase: implement` with `review,stale`, after the turn `phase: implement`; for mr: phase
  `merge-request`, state working (as decided in question 1); for mr-skill: `phase: implement`,
  `review,stale`, `merge-request,stale`
