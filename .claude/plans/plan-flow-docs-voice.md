# flow docs in a human voice

Review page: https://claude.ai/artifact/Ui9ZUEo42rQFK1Cw6hYZnr

## Decision

Rewrite the user docs in place, same files, so they read like a person wrote them. Facts, settings,
keys, defaults, messages and real command output stay exactly as they are; the sentences change.
Make "use your own skills" obvious in the README quick start and at the top of configuration.md.

In scope: `README.md`, `docs/*.md` except `HANDOVER.md` and `docs/design/`, `ROADMAP.md`,
`CHANGELOG.md`, the comments in `examples/config.toml` and `examples/theme.toml`. Not CLAUDE.md,
not code.

Decided in review:

- flow is an orchestration hub, not a multiplexer. You talk mostly to the master, the master
  instructs the workers, and you can see and understand every agent's work. The picture: you are
  the CTO, the master is the team lead, the workers are the developers. You talk mostly to the team
  lead, who instructs the developers, but as CTO you still know the details and stay responsible
  for things working. The README opens with this in plain, humble words; "What it does" leads with
  the master; flows.md introduces the master the same way.
- No first person anywhere ("I", "my"), not in the intro, not in Similar tools. Write to the reader
  ("you") throughout, humble without first person.
- Generic skill names in the "use your own" example (`my-feature`, `my-plan`, `my-build`, `my-pr`).
- Similar tools adds firstmate (https://github.com/kunchenguid/firstmate), neutrally: a lead agent
  (the first mate) that starts crewmates in their own git worktrees, shipped as a repo of
  instructions, skills and scripts you start your agent in; crewmates run in tmux by default (also
  herdr, cmux, Zellij, Orca); no dashboard of its own.
- Supporting fewer agents is never presented as a shortcoming of flow, on any page. Drop "herdr and
  ccmanager recognise many more agents ... will suit you better" and the like. ROADMAP.md gets
  **More agents** under Planned soon.

## Style rules

Models: ripgrep's GUIDE.md (introduce an example in a line, show it, explain it), zoxide (command
blocks with a `# comment` per line), fd (README shape: pitch, picture, small runnable examples),
lazygit (a person talking, here without first person and much calmer).

- Every page opens with something to run or look at, then explains it.
- "You" for the reader. Contractions are fine.
- Tables only for lookups: settings keys, placeholders, colors, exit codes, JSON fields, key
  bindings, hook events. Everything else is prose.
- At most one colon-then-list per section. A list that repeats across pages is said once and linked.
- No runs of `**Label.** text` bullets. No stacked possessives ("the worker's last turn's whole
  closing message"). No over-qualifying.
- Guide pages (README, install, ui, flows, configuration, themes, repository, troubleshooting) get
  headings that say what you want to do ("Start a flow", "Use your own workflow"). Reference pages
  (cli, json, files) keep noun headings.
- Never an em dash, never an emoji.

## Facts the new text must say

What happens without matching skills (verified):

- A flow still works. States come from the harness hooks, not from skills: working, waiting (with
  the question), idle, done. An opened merge request still finishes the flow (`src/fold.rs:260`).
- The steps stay `○───○───○───○`; where the phase name would be, the sidebar shows the state:
  `starting` while it works, `idle`, `waiting` (`src/ui/sidebar/labels.rs:69`). Task counts only
  with a plan-progress tool, as before.
- A first prompt that names a skill you don't have: Claude Code passes the line to the model as an
  ordinary message; tried with `claude -p "/zzz-no-such-skill say hi"`, it said the skill is not
  installed and did the request. So a worker without `ship-feature` still gets the request.

When a change applies: `flow start` reads `[worker] agent` and `[[phases]]` every run; everything
else (`[agents]`, any prompt, `[master]`, `[ui]`, the new-flow dialog's preselected agent) after
`flow ui --stop` and `flow ui`.

## Samples to build from

README opening:

> If you run several coding agents at once, each on its own feature, you know the routine of going
> from one terminal to the next to see which one needs you. flow works more like a small team. You
> talk to one agent, the master. It starts the workers, tells them what to do, and comes back to you
> when one of them needs a decision. Think of yourself as the CTO and the master as your team lead:
> you talk mostly to the lead, and the lead runs the developers.
>
> A CTO still knows the details, and it's still on you that things work. So you can watch every
> worker as it goes, read what it did and why, and step in whenever you want. Each one runs in its
> own git worktree, and the sidebar shows what it's doing and whether it's waiting on you.

README "Pick a workflow" (right after install, before first run): every worker starts with a first
prompt, by default `/ship-feature <request>`. Two paths one after the other under one heading, each
a short paragraph and one block:

- Use the agent-skills set: the existing `npx skills add ...` line, unchanged.
- Use your own, in `~/.config/flow/config.toml`:

  ```toml
  [worker]
  prompt = "/my-feature {request}"   # $my-feature in Codex

  [[phases]]
  name = "plan"
  skills = ["my-plan"]

  [[phases]]
  name = "build"
  skills = ["my-build"]

  [[phases]]
  name = "ship"
  skills = ["my-pr"]
  ```

  A phase starts when the worker runs one of its skills; link to configuration.md "Use your own
  workflow".
- Then, plainly: without either, flows still work (the facts above).

configuration.md "When a change applies":

> Most changes need a restart of flow ui:
>
> ```sh
> flow ui --stop && flow ui
> ```
>
> Two settings don't. `flow start` reads `[worker] agent` and `[[phases]]` every time it runs, so
> the next flow you start already uses them. The new-flow dialog's preselected agent still waits
> for the restart.

## Pages

- **README.md**: the opening above; "What it does" as four short paragraphs, master first
  (worktrees and setup/slots, hooks not screen, sessions outlive the UI); install; "Pick a
  workflow"; first run; "How it works", the TOON sample and the command table stay; "Similar tools"
  in prose with firstmate, no "fewer agents" shortcoming; Documentation; License unchanged.
- **docs/README.md**: one plain line per page.
- **docs/install.md**: example first; the skills step offers both paths and links to configuration.md.
- **docs/configuration.md**: first section "Use your own workflow" (the example, prompt and phases
  are separate settings, `/skill` vs `$skill` and a shared prompt, a running flow keeps its phases,
  what needs a restart, what happens without matching skills). Then the settings reference, every key
  and table kept; the `[[phases]]` reference keeps the superpowers example.
- **docs/flows.md**: opens with `flow start`; the master introduced as the one you talk to; states and
  hook tables kept, prose rewritten.
- **docs/ui.md**: key tables kept; the screen as a short tour.
- **docs/themes.md**, **docs/repository.md**: tables kept, examples first, prose rewritten.
- **docs/cli.md**, **docs/json.md**, **docs/files.md**: structure kept, sentences rewritten, output
  byte for byte.
- **docs/troubleshooting.md**: light pass.
- **ROADMAP.md**: plainer; add More agents under Planned soon. **CHANGELOG.md**: plainer, still only
  the 0.1.0 entry.
- **examples/config.toml**, **examples/theme.toml**: comments only; every `#key = value` and
  `#[section]` line stays byte for byte. No `earlier` entry in `DefaultFile` (pre-release, as in
  c029a72).

## Checks

- Before each page, list its facts (keys, defaults, numbers, messages, links) and tick them off
  against the new text.
- A script in the scratchpad checks every relative link and `#anchor` in the scope.
- grep the scope for `—`, emoji, and " I ", " my ", "I'm", "I've" outside code.
- `cargo test` after the example files change.

## Tasks

- [x] Write the link and anchor checker and the voice grep (scratchpad); run them on the current docs as a baseline
- [x] Rewrite README.md: orchestration opening, What it does, Pick a workflow, Similar tools with firstmate
- [x] Rewrite docs/configuration.md with "Use your own workflow" first and the no-skills facts
- [x] Rewrite docs/install.md and docs/README.md
- [x] Rewrite docs/flows.md
- [ ] Rewrite docs/ui.md
- [ ] Rewrite docs/themes.md and docs/repository.md
- [ ] Rewrite docs/cli.md, docs/json.md and docs/files.md
- [ ] Light pass on docs/troubleshooting.md
- [ ] Rewrite ROADMAP.md (add More agents) and CHANGELOG.md
- [ ] Rewrite the comments in examples/config.toml and examples/theme.toml; cargo test
- [ ] End to end: run the link checker and voice grep clean; read README and configuration.md top to bottom as a new user would, rendered as GitHub renders them; put the "use your own" example into a scratch `FLOW_CONFIG` and run `flow start` in a scratch repo with a scratch `FLOW_HOME`, confirming the flow records the three phases
