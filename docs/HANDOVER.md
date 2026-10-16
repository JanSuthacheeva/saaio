# Handover: the flows system

Written 2026-10-14 at the end of the session that designed the system and built `flow`
core. Everything needed to continue is here; the conversation itself is not.

## Goal

Run several features in parallel, each as its own ship-feature flow in its own Claude
session and worktree, and keep track of all of them from one master session: phase, state
(exploring, delegated, implementing, reviewing, waiting, blocked, done), open blockers and
progress history. Small features should later run without stops and only be reviewed at the
end (auto mode).

The flow being tracked is the agent-skills chain in `~/projects/agent-skills`:
design-code -> implement-plan -> review-changes -> write-merge-request, run in order by a
ship-feature orchestrator skill (planned there, not written yet; see its plan page
`~/projects/agent-skills/.lavish/ship-feature-plan.html`). The skills stay agent-agnostic.

## The six pieces

| # | Piece | Where | Status |
| --- | --- | --- | --- |
| 1+2 | Event format + `flow` core (new, report, status, clear, home) | this repo | **done**, plan `.claude/plans/plan-flow-core.md` |
| 3 | Board viewer mod: tabbed board above the prompt | `~/projects/claude-code-mods` (new mod `flows`) | **retired** by flow ui (its sidebar and board replace it); was built (claude-code-mods main, marked experimental), plan `~/projects/claude-code-mods/.claude/plans/plan-flows-board.md` (local, gitignored there); design page `docs/design/board-implementation.html` |
| 4 | Adapter: a Claude Code worker reports into flow | this repo, `src/adapter/` | **rebuilt in flow** (plan `.claude/plans/plan-flow-adapters.md`): Claude Code's own hooks run `flow hook claude-code`; the flows mod and the skills' report lines are gone. Was the flows mod in claude-code-mods, plan `~/projects/claude-code-mods/.claude/plans/plan-flows-adapter.md` (local); page `docs/design/adapter-implementation.html`. Typed skills raise no skill.prompt, so phases also come from the submitted `/skill` prompt |
| 5 | `flow start`: worktree, slot, setup hook, worker; `flow clear` tears down | this repo | **built**, plan `.claude/plans/plan-flow-start.md`, page `docs/design/start-implementation.html`; since flow ui the worker opens there, the tmux path is gone |
| 7 | `flow ui`: session server owning every Claude session, master + workers, sidebar, peek, board, new flow, rail, restore | this repo | **built**, plan `.claude/plans/plan-flow-ui.md`, pages `docs/design/ui-approaches.html`, `docs/design/ui-implementation.html`, screens https://claude.ai/artifact/68Ff2wJP6BcYX6qy4hppXp |
| 8 | One flow per project: slots shared across every project's store, `flow status --all`, the board's `a` | this repo, `src/registry.rs`, `src/slots.rs` | **built**, plan `.claude/plans/plan-flow-per-project.md`, pages `docs/design/projects-approaches.html`, `docs/design/projects-implementation.html`. Each tmux project session sets its own `FLOW_HOME`; stores register in the default folder's `stores/`, `flow start` claims `slots/<n>` (1-20) under a flock before setup |
| 9 | Themes: theme.toml with extends (terminal, flow, flow-light, themes/<name>.toml), reloaded live; a Noctalia template in dotfiles-v2 | this repo, `src/ui/theme.rs`, `src/ui/theme_file.rs` | **built**, plan `.claude/plans/plan-flow-theme.md`, pages `docs/design/theme-*.html`. each key is checked against its own table (plan `.claude/plans/plan-flow-strict-theme-sections.md`); `[glyphs]` from the design is rejected until it is built |
| 10 | Branch names after a repo's convention: `.flow/config.toml` `[branch] name`/`types`, `flow start --type`, a type row in the new-flow dialog | this repo, `src/branching.rs` | **built**, plan `.claude/plans/plan-flow-branch-names.md`, page `docs/design/branch-implementation.html` |
| 11 | Codex as a second harness, for workers and the master | this repo, `src/adapter/codex.rs`, `src/server/news.rs` | **built**, plan `.claude/plans/plan-flow-codex.md`, pages `docs/design/codex-approaches.html`, `docs/design/codex-implementation.html` |
| 6 | Auto mode + optional report line in the skills, `flow setup` hook | `~/projects/agent-skills` (+ this repo for setup) | not designed |

Each piece gets its own design-code round (approaches page, implementation page, plan)
before building.

## Decisions and why

Round 1 (first approaches, a mod-only design, superseded):
- Workers are separate Claude sessions, one per worktree, not subagents of the master.
- The master starts the workers (later `flow start`).
- You answer a blocker in the worker itself; the board shows it and jumps you there. No
  answering from the master - one-way data, nothing lost when the master is closed.
- Status in Claude Code comes from observing engine events, not from the model remembering
  to report (we saw Claude skip the plan-progress tool once).

Round 2 (system structure), chosen **A: a `flow` CLI owns event files, the mod adapts and views**:
- `flow` is a Rust CLI so it works across harnesses; any harness reports through a shell
  command. Rejected: a `flow serve` daemon (process lifecycle, sockets, not needed for a
  2 s board) and `flow` driving the phases itself instead of ship-feature (cold context per
  phase, judgement moves out of the agent, duplicates the approved ship-feature plan; kept
  as the fallback if instructions prove unreliable at keeping order and stops).
- Append-only event log per flow: progress and history come for free.
- Auto mode lives in the skills, not in `flow` (see piece 6).

Round 3 (core structure), chosen **A: the CLI folds; readers use `flow status`**:
- The fold (events -> state, durations, tasks, timeline) exists once, in Rust. The log is
  private; the only contracts are the command line and `flow status --json`.
- Rejected: readers folding raw logs themselves (two folds drift), and snapshot files next
  to each log (second contract, crash consistency; a possible later optimisation if process
  starts every 2 s ever matter).

AXI (asked during review):
- `flow`'s main user is an agent, so it follows https://axi.md: TOON output by default, a
  home view with no arguments (`bin`, `description`, live data), `help[]` next steps on every
  output, structured errors on stdout, exit codes 0/1/2, idempotent mutations, no prompts,
  short default fields with `--full`, aggregates (`count`, `needs_you`), explicit empty
  states. `--json` stays for programs.

Answered open questions:
- Command surface: `flow`, `flow new`, `flow report <event>`, `flow status [<name>] [--json] [--full]`, `flow clear`; flow from `--flow <name|id>` or `$FLOW_ID`.
- Logs in `$FLOW_HOME`, else `$XDG_STATE_HOME/flow`, else `~/.local/state/flow`.
- `flow report` with no flow id answers `skipped: no flow id` and exits 0, so the skills'
  optional report line never derails a normal session.
- Ship mode: Claude builds `flow` with implement-plan (despite personal Rust defaulting to
  learn mode in `~/OPINIONS.md`).
- New repo `~/projects/flow` for the CLI (not inside claude-code-mods or agent-skills).

Earlier answers from round 1 that still hold for pieces 3-5:
- Only sessions started by the master appear on the board (recommended, not re-confirmed).
- ~~`flow start` outside tmux: refuse with a hint~~ - superseded: flow ui hosts the workers, no tmux.
- Finished flows stay until `flow clear` (implemented).
- Workers run in your default permission mode (recommended, not re-confirmed).

## What `flow` core does now

See `README.md` for the commands and output, `CLAUDE.md` for the module map, and the plan
for signatures, fold rules and call chains. Built in 14 commits, 46 unit and 13 integration
tests, installed with `cargo install --path .` at `~/.cargo/bin/flow`.

Changes made while building (all in the plan):
- `Phase::name()` added for timeline text and TOON output.
- The list of open questions and block reasons is `blockers[n]{name,blocker}`.
- Detail timeline rows are `{ago,text}`; absent values are left out, not `null`.
- `Store::at` is test-only.
- After `flow new` the first hint is `flow report --flow <name> ...`: in the end-to-end run
  an agent's shell did not keep an exported `FLOW_ID` between commands, so `--flow` is what
  agents actually use.

End-to-end results: two shells writing 200 reports each into one flow lost nothing (401
lines, all parse); a fresh agent told only "use `flow` to report your progress" found its
way from the home view and hints, created a flow, reported phase and tasks, and ended it.

## Verified facts the next pieces build on

Claude Code hooks (2.1.300, recorded):
- `Stop` carries `background_tasks`: `[{"id", "type": "subagent", "status": "running",
  "description", "agent_type"}]` while background work is in flight, `[]` otherwise. flow reads a
  running or pending task as `delegated` (fixture `stop_background.json`). Publishing a claude.ai
  Artifact arms a live subscription that stays there as `{"type": "monitor", "status": "running",
  "description": "live updates for artifact <url> (auto-armed on publish)"}` (2.1.301) and never
  finishes, so flow skips it and the turn ends idle or waiting (fixture `stop_artifact_watch.json`).
  The skip matches that wording, so re-record the fixture with the others on a Claude Code upgrade.
- A subagent's hooks fire with `agent_id` set; a phase skill it starts (`review-changes` in a
  fresh agent) reaches PreToolUse Skill only when the skill exists (an unknown skill is refused
  before any hook).
- When the background agent finishes, the main agent resumes on its own and its next `Stop`
  follows.
- A command hook with `"asyncRewake": true` runs in the background; when it exits 2, its stderr
  wakes the session as a new turn, shown as "Stop hook feedback". Probed on 2.1.300: on `Stop`
  it woke an idle session at its prompt and is armed again at each turn end; on `SessionStart`
  it woke a fresh session that had no prompt yet. Its `timeout` (seconds) also bounds a
  background hook. `flow wait` is built on this.
- `claude/channel` MCP servers can push messages into a session, but 2.1.300 offers one only
  from an approved allowlist or with a development flag, and Team/Enterprise accounts have them
  off by default: not a base for flow.

Claude Code mods (2.1.296 / 2.1.299):
- A mod's `$.store` is shared live between simultaneous sessions (probe mod in two
  sessions: each read the other's writes at once; 2 x 200 concurrent writes to distinct keys,
  none lost). It is one file per plugin **and install source**
  (`~/.claude/plugins/store/<plugin>_<source>-<hash>.json`): marketplace install,
  `--plugin-dir` and a skills-dir symlink each get their own file.
- `$.session.send({ to: { sessionId } })` reaches another local session, and a
  `session.receive` hook there can consume it before the model sees it. The text arrives as
  `<cross-session-message from="uds:..." from-name="...">...</cross-session-message>`.
- `$.fs` reads and lists any absolute path; `$.process.run(argv, { cwd, env, timeoutMs })`
  inherits the host environment (so `TMUX` is visible to a master inside tmux).
- A worker's mod can observe: `skill.prompt` (which skill started; since 2.1.299 also its
  frontmatter, source and path), `tool.call` (Agent, AskUserQuestion, `mcp__plan-progress__report`,
  Bash), `turn.start` / `turn.complete` (with `agentId` for subagent loops), `agent.spawn`,
  `$.agent.list()`, `session.end`.
- `$.ui.open({ id, title, columns })`: a pane opened by something the person did is placed
  at any width; opened unasked it waits below 144 terminal columns (110 for an id the person
  opened before). In a docked pane's `ui.render`, `e.viewport.columns` is the transcript
  beside the dock, not the terminal: terminal = viewport + bodyColumns + 1.
- A width the person dragged is saved globally as `pluginPanes.dockColumns` in
  `~/.claude.json` and overrides every mod's `columns` request.
- There is no API for the dock's grey background yet (a recurring check watched for it).

Claude Code CLI:
- `claude -w <name> --session-id <uuid> -n <name> "<prompt>"` creates a worktree, fixes the
  session id (reuse the flow id) and names the session. `--tmux[=classic]` exists too.
- A new folder shows the workspace trust prompt first; a launcher that types a prompt blindly
  answers it "No, exit" (seen in the end-to-end run). `flow start` must handle that.

TOON: `serde_toon_format` 0.2 (crate name `serde_toon`) encodes tables, inline arrays and
quoting as the design expects.

## Gotchas learned

- **Old engines keep running.** Sessions run for days on the version they started with, but
  load mods from the repo folder on `/reload-plugins`. Switching plan-progress to the
  2.1.299-only `Text weight` prop blanked its pane in 2.1.296 sessions ("ui.render (Pane)
  refused: Text prop weight is not allowed"). Test mod renders under the oldest installed
  version too: `~/.local/share/claude/versions/<v> --plugin-dir <mod>` in a private tmux.
- **Never touch the default tmux server** in tests; use a private socket (`tmux -L ppshot`)
  and kill only your own session there.
- An open pane can be off screen (behind another pane's tab, or not placed); a toggle must
  check `isShown && isPlaced`, not just "is open" (fixed in plan-progress on branch
  `fix/plan-progress-toggle`).
- An agent's Bash calls do not share shell state; pass `--flow` or set `FLOW_ID` in the
  worker's environment at launch.

## Notes for the next pieces

**3 - board viewer (mod).** Designed 2026-10-14: a band above the prompt, not a pane - tab row (`All` + one tab per flow), cards with state pills on All, a hero card with timeline and decisions on a flow's tab, digits plus vim keys (h/l tabs, j/k, Enter, g, o). Off until `/flows`, remembered; stacks above skill-usage. Prototype findings: per-phase colours collapse in some themes (colour by progress instead), `Box` has no `flexBasis`, pill ends need a Nerd Font. The notes below were the pre-design starting point. Original notes: Start from plan-progress in `~/projects/claude-code-mods/plan-progress`:
store + 2 s poll + pure model (`hooks/pane.ts`) + drawing (`hooks/register.tsx`), dock at a
third of the terminal (`dockColumns`, `terminalColumnsBesideDock`). The board runs
`flow status --json` every 2 s while open (`$.process.run`), shows a clear "install flow"
state when the binary is missing, and draws one row per flow: name, state in its colour,
phase strip with durations, task bar, blocker, age; a row opens the flow's timeline. One mod
cannot import another, so shared helpers are copied. Mock of the intended look is in
`docs/design/system-approaches.html` ("The same in every approach").

**4 - adapter (mod).** In a worker, map engine events to `flow report` calls: a phase skill
starting (design-code -> design, implement-plan -> implement, review-changes -> review,
write-merge-request -> merge-request), Agent tool start/end (delegated / working), an
AskUserQuestion (waiting), a main-loop `turn.complete` (waiting), plan-progress reports
(tasks, blocked), `gh pr create` / `glab mr create` succeeding (merge-request), `session.end`
from `/exit` (ended; a hangup is not an end, so flow ui can resume the worker), and `heartbeat`
every 30 s. The flow id comes from `$FLOW_ID` set by `flow start`.
Background subagents should not count as delegated.

**5 - `flow start`.** `flow new`, then `tmux new-window -d -P -F '#{window_id}' -n <name> -c <repo> -- claude -w <name> --session-id <flow id> -n <name> "/ship-feature <request>"` with `FLOW_ID` in the
environment; record the window id (a new event or a side file) so the board can jump with
`tmux select-window`. Handle the trust prompt. Name from the ticket key (`SHOP-217` ->
`shop-217`) or the first words, `-2` when taken. Per-harness command templates come later.

**6 - auto mode (agent-skills).** ship-feature takes a mode (guided / auto). In auto,
design-code picks its recommended approach and writes the plan without review pages,
recording choices under `## Decisions`; implement-plan and review-changes run as today;
write-merge-request leads with the decisions so the one review at the end covers code and
choices. Guardrail: anything expensive to reverse (stored data, public surface,
user-visible behaviour) stops the flow even in auto. One optional, agent-agnostic line in the
skills: "if a `flow` command exists, report each phase, decision and stop". Needs evals that
prove auto picks sensibly and really stops on the expensive cases. A `flow setup` command can
install the ambient-context hook (AXI principle 7).

## flow ui (built 2026-10-14)

The vision below is built as `flow ui`, see README and `.claude/plans/plan-flow-ui.md`. Decisions:
- Our own multiplexer, not tmux or herdr: `flow server` (started by `flow ui`, a new session via
  setsid) owns every PTY and its terminal emulator (alacritty_terminal, kitty keyboard off so
  C-space stays a NUL byte); `flow ui` is a client over a Unix socket. Sessions survive the UI.
- The log is the only way a pane opens: the server reconciles every unfinished flow with a
  workspace; once opened it records `Workspace.window = "flow-ui"`, so later opens resume.
- One thread owns all server state; only the focused pane streams (changed rows, at most 60/s).
  Idle: about 0.15 % CPU and 3-5 MB each for server and UI.
- The master keeps its session across crashes (master.json), starts fresh after a stop, and runs
  the built-in `master` agent with `master.md` appended to its system prompt.
- `C-space Q` keeps or ends unfinished flows; ended flows keep branch and worktree until `flow clear`.
- Later: see Future improvements.

## The master and its workers talk (built 2026-10-15)

See README "The master and its workers" and `.claude/plans/plan-flow-talk.md`. Decisions:
- Messages are events in the flow's log (`message`), so the conversation is in `flow status`; the
  master is the only sender, a worker asks by ending its turn with the question.
- Delivery and waking use Claude Code's own hooks (`asyncRewake` `flow wait` on SessionStart and
  Stop), not typing into the PTY: it never mixes into what you are typing, works without flow ui,
  and the worker can tell the master's words from yours.
- The flow's mode decides who answers: auto -> the master when the code, plan or earlier decisions
  settle it; guided -> you, the master relays. `[master] wake = false` turns the master's off.
- One waiter per session (`waits/<key>.pid`, last claim wins); the master's `waits/master.seen`
  keeps what it was woken for, so nothing wakes it twice.

## Plain sessions (built 2026-10-16)

See README "Plain sessions" and `.claude/plans/plan-flow-plain-sessions.md`. Decisions:
- Plain is a third `Mode` (`plain`), not a `--mode` value: `flow start --plain <folder>` picks it,
  and refuses a request, `--mode` or `--type` with it. The dialog's mode row is auto, guided, plain.
- Its Workspace is the folder with `branch` and `slot` null, which made `VIEW_VERSION` and
  `PROTOCOL_VERSION` 2. The server did not change: it opens whatever the log records.
- It is the user's own: no first prompt, no worker system prompt, no `flow wait` hook; it never
  wakes the master and `flow send` refuses it. The reporting hooks stay, so its state shows.
- `repo_of` is None without a branch, so `flow remove` never tears down or checks a folder flow did
  not create, even one inside `.flow/worktrees`.

## Codex (built 2026-10-16)

See README "Agents", "How a worker reports" and `.claude/plans/plan-flow-codex.md` (its "Verified
Codex facts" list every source line). Decisions:
- Hooks per launch as `-c hooks.<Event>=[...]` (approach A); Codex's own review trusts them once and
  stores the hash in `~/.codex/config.toml`. Rejected: `--dangerously-bypass-hook-trust` (runs
  every untrusted hook, a cloned repo's too), installing into `~/.codex/hooks.json`, copying
  Codex's trust hash.
- Codex picks its own session id: a `session` event (`FlowView.session`, `PROTOCOL_VERSION` 4)
  records it and the rollout path; `{session}` resumes it. A Codex master keeps it in
  `waits/master.session`, forgotten when the master starts fresh. A session that never had a turn
  starts again instead of resuming.
- Codex cannot wake an idle session (no asyncRewake; an async hook's exit 2 is dropped), so:
  its synchronous Stop hook hands the news over (exit 2), and flow server types news into an idle
  pane as one bracketed paste after ten quiet seconds, only once the session had its first turn
  (`waits/<key>.busy` says idle). This is the one place flow writes into a PTY on its own.
- Instructions go through `Adapter::instruct`: Codex `-c developer_instructions=<toml string>`,
  Claude Code `--append-system-prompt` (the master too, instead of `-file`).
- `[master] agent` picks the master's agent; `[worker] agent` the worker's default (the top-level
  `agent` key is refused, naming it), the dialog's agent row the worker's.

Verified on Codex 0.163.0 (recorded in `src/adapter/fixtures/codex/`):
- The subagent tool is `collaborationspawn_agent` (namespace + name) with an encrypted `message`;
  delegated shows its `task_name`. It returns at once, `collaborationwait_agent` waits.
- Bash's `tool_response` is the output string; PermissionRequest's input adds `description`.
- SessionEnd's reason is always "other": no `/exit` event, so a Codex flow never ends from a hook.
- `codex <-c hooks> resume <id>` keeps the id, fires SessionStart `source: "resume"`, and runs the
  trusted hooks without a new review. Worktrees inherit the main repo's Codex trust.
- The rollout's shell calls are code-mode `exec` scripts (`tools.exec_command({cmd:"..."})`).

Later: `codex queue --thread <id> --message` (0.163.0, experimental) starts a turn on an idle
thread, but only through Codex's shared daemon, and a session with `-c` overrides runs its own
embedded server, which queue refuses. Once the daemon takes per-session hooks, queue replaces the
server's typing.

## Future improvements

- **Working across repos.** One flow ui already shows every repo's flows and the master can
  `cd` anywhere to `flow start`; three gaps remain:
  - the new-flow dialog takes the repo of the session in view; give it a repo field, prefilled
    and choosing from the repos flow has seen in its logs;
  - the sidebar and board do not say which repo a flow belongs to; show it on the flow's second
    line and on its card, grouped by repo once there are flows in more than one;
  - `flow start --repo <path>`, so the master and scripts need no `cd`.
- **The master asks a worker, the worker answers.** `flow ask <flow> "<question>"` stores the question;
  delivery needs no model decision: a busy worker gets it from its own `Stop` hook ("continue with
  this question from the master"), an idle one gets it typed into its prompt by flow server. The
  worker's next `Stop` carries its last message, which flow records as the answer. `flow ask` returns
  the answer to the master's Bash call, or "asked, no answer yet" after a timeout, the answer then
  arriving as a message in the master's session. The sidebar shows "asked by master", the timeline
  the exchange. To settle: hooks for the master too, a question arriving while you type in that pane,
  loops; start with master -> worker only.
- **A mascot.** A small character in a corner of flow ui (bottom right of the sidebar, or the bar
  when solo) that reacts to the board: idle, working while workers run, dancing when a flow opens
  its merge request, worried when one is blocked. Further on it becomes the master's face, a Jarvis
  that tells you what happened ("shop-221 needs a decision on the migration") from `flow status`,
  in a line beside it. Drawn in a few terminal rows with the theme's tokens, animated on the UI's
  existing tick; off with a setting.
- **A flow setup skill.** A skill that prepares a project for flow: it reads the repo (Docker
  Compose services, ports, env files, migrations, package managers) and writes `.flow/setup` and
  `.flow/teardown`, with ports taken from `FLOW_SLOT` so parallel worktrees never clash (the README's
  Docker example as its starting point), then runs both once in a scratch worktree to prove them.
- **Extensions:** glab and ClickUp as data sources (ticket as the request, MR and pipeline status
  per flow); the new-flow dialog's "from ClickUp" list in the design waits for this.
  Decide whether both should match.
- **Notifications (first after 0.1).** When a flow turns waiting or blocked and you are not looking at
  it, flow ui sends a terminal notification: OSC 9, OSC 777 (Ghostty, iTerm2), the bell as fallback, so
  no OS-specific code. A switch in `config.toml`. Today only a master that pushes to your phone covers
  this; without a master, or with flow ui in the background, a waiting flow goes unnoticed.
- **Subagents in the sidebar.** `delegated` shows one foreground subagent's task; background
  subagents (`run_in_background`) are not counted, so a worker running three in parallel looks merely
  working. Count every running subagent from the harness's start and stop hooks (Claude Code's
  subagent hooks, Codex's equivalents), show the count beside the state glyph (`◐ 3`) while any run,
  and list their tasks in the board's detail view.
- **No messages between workers (decided 2026-10-16).** Each flow keeps its own branch so its work
  stays independent and reviewable; workers sending each other messages would couple flows through
  conversations nobody reviews, allow loops (A asks B, B asks A) and hide who decided what. A worker
  may read other flows (`flow status --all`, their plans), like any node reading a CAN bus; whatever
  it needs from another flow it says at the end of its turn, and the master (later `flow ask`) passes
  it on.
- **No real bus for now (decided 2026-10-16).** flow is a publish/subscribe log with one supervisor:
  status works like CAN (workers report unasked into the logs, each reader filters, `fold::changes`
  drops what changes nothing), control like LIN (only the master sends, one flow per message), except
  that the master is woken instead of polling. flow server and `flow wait` poll the store every
  500 ms. A hub over the server's socket, with hooks pushing to it, would still have to write the logs
  (they survive crashes, and reporting works without flow ui), still could not wake Codex, and would
  gain at most half a second. Revisit when flow spans machines or needs sub-second reactions.

## Long-term vision: `flow` as its own TUI

Jan's picture (2026-10-14): `flow` becomes a terminal UI of its own - one big master pane
with the worker sessions as small tabs, integrated with glab and ClickUp - instead of a
board inside Claude Code plus tmux windows.

Why the current design carries over:
- The board is just another reader. Every flow's state is already in the event logs and
  `flow status --json`; a full-screen `flow` UI reads the same data the Claude Code band
  reads, so nothing in pieces 1-5 is wasted.
- Starting sessions is already planned. `flow start` (piece 5) launches workers; in the TUI
  it would start them inside `flow` instead of in tmux windows.

What it would take, in Rust:
- A TUI layer, such as `ratatui`.
- Pseudo-terminals for the Claude sessions, such as `portable-pty`.
- Terminal emulation to draw each session inside a tab, such as `vt100` or `wezterm-term`.

That is a small tmux of its own: the master pane, the other sessions as tabs, focus
switching and input routing. Embedding a full TUI like Claude Code is the hard part:
resizing, scrollback, mouse and key handling.

Alternatives to weigh before building it:
- tmux as the engine, `flow` as the brain: `flow` drives tmux in control mode and draws
  its dashboard around it. Less to build, less design control.
- Headless sessions through the Agent SDK: `flow` renders the conversations itself. Full
  design control, but the workers lose Claude Code's own UI and the mods.

Integrations as data sources:
- ClickUp tickets become the flow request ("ship SHOP-217" already plans to read the
  ticket). `~/projects/clickup-cli` is a Go CLI (a clone of `iamcommee/clickup-cli`) that
  lists assigned tasks and shows task details, with a Claude Code skill; `flow` could call
  it or talk to the ClickUp API directly.
- glab / gh give each flow its merge request and pipeline status, as the merge-status mod
  does today (`~/projects/claude-code-mods/merge-status`).

## Where things are

- This repo: `~/projects/flow`, private on GitHub at https://github.com/JanSuthacheeva/flow. Plans: `.claude/plans/plan-flow-core.md`, `plan-flow-start.md`, `plan-flow-ui.md` (branch `flow-ui`).
  Design pages: `docs/design/` (system approaches round 2, core approaches, core
  implementation) - open them in a browser or with `npx -y lavish-axi <file>`.
- Mods: `~/projects/claude-code-mods` (plan-progress, skill-usage, merge-status), installed
  from its folder marketplace. Branch `fix/plan-progress-toggle` is unpushed: the toggle fix
  and the revert of the `weight` change, which `origin/main` still has.
- Skills: `~/projects/agent-skills` (design-code, implement-plan, review-changes,
  write-merge-request; ship-feature planned).
- Claude memory for the mods project: `~/.claude/projects/-Users-jansuthacheeva-projects-claude-code-mods/memory/`
  (`flows-system.md`, `mods-old-engine-compat.md`).
