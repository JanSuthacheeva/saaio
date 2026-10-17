# flow

![flow ui with Claude Code and Codex workers](docs/media/flow-ui.png)

If you run several coding agents at once, each on its own feature, you know the routine of going
from one terminal to the next to see which one needs you. flow works more like a small team. You
talk to one agent, the master. It starts the workers, tells them what to do, and comes back to you
when one of them needs a decision. Think of yourself as the CTO and the master as your team lead:
you talk mostly to the lead, and the lead runs the developers.

A CTO still knows the details, and it's still on you that things work. So you can watch every
worker as it goes, read what it did and why, and step in whenever you want. Each one runs in its
own git worktree, and the sidebar shows what it's doing and whether it's waiting on you.

flow works with Claude Code and Codex. It's a terminal UI, `flow ui`, plus a command line written
for agents to read. It started as a tool for one person's daily work and is shared in case it
helps you too.

## Install

You need macOS or Linux, git, Rust 1.95 or newer, and Claude Code or Codex.

```sh
cargo install --git https://github.com/JanSuthacheeva/flow --tag v0.1.0
```

## Pick a workflow

Every worker starts with a first prompt. Out of the box that's `/ship-feature <your request>`
(`$ship-feature` in Codex), a skill that plans the feature, builds it, reviews it and opens the
merge request. You can install that skill and the ones it uses, or point flow at skills you already
have.

**Use the agent-skills set.** It lives in
[agent-skills](https://github.com/JanSuthacheeva/agent-skills):

```sh
npx skills add JanSuthacheeva/agent-skills -g --skill ship-feature design-code implement-plan review-changes write-merge-request commit setup-flow
```

**Use your own skills.** Tell flow which prompt starts a worker and which skills start each phase,
in `~/.config/flow/config.toml`:

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

A phase starts when the worker runs one of its skills, and your phases replace the built-in four.
[Use your own workflow](docs/configuration.md#use-your-own-workflow) explains the rest.

You can also skip both. A flow works without any matching skills: the worker gets your request,
and the sidebar still shows whether it's working, waiting for you or done. Its steps just stay
empty (`○───○───○───○`), since no skill ever starts a phase.

## First run

In a repository:

```sh
flow ui
```

The big pane is the master. Ask it to "start a flow for SHOP-217 add CSV export", or press
`C-space n` to start one yourself. Each worker shows up in the sidebar on the right, grouped by
what it needs from you, and `C-space ?` lists every key.

![Moving through flow ui: the peek, a worker, the board](docs/media/flow-ui.gif)

The first run also writes `config.toml`, `theme.toml` and `master.md` to `~/.config/flow`, with
every setting commented out at its default.

## What it does

The master is a regular agent session that knows how to use flow. It starts workers, sends them
answers with `flow send`, and is woken whenever one waits, gets blocked or finishes. In a guided
flow it brings every question to you. In an auto flow it answers what the code and the plan already
settle, and only asks you the rest.

Each worker gets its own branch and worktree, so nothing it does touches your checkout. If your
project needs services, `.flow/setup` and `.flow/teardown` run in every worktree, and `FLOW_SLOT`
(1 to 20, never shared between two of your projects) gives each worktree's Docker stack its own
ports. The `setup-flow` skill can write both scripts for your repo and try them in a scratch
worktree.

flow knows what a worker is doing because Claude Code and Codex tell it through their own hooks.
It never reads the screen. Any other command can run as a worker too; flow then shows whether it's
running. The phases, the skills that start them and every agent's first prompt are all settings.

Close the terminal and everything keeps running. After a reboot, `flow ui` resumes every
conversation where it stopped.

Next to your workers you can open plain sessions: an agent in any folder, without a worktree or a
flow. And flow takes on your terminal's colors by default, with a dark and a light theme of its
own. Themes reload while you edit them, and another tool can write one for flow to follow (the docs
have a Noctalia template).

## How it works

Every flow is an append-only log of events. The agents' hooks run `flow hook`, which turns what
happened into an event: a skill started a phase, a question is waiting, a merge request was opened.
flow folds those logs into where each flow stands. `flow server` runs each agent in its own
terminal and keeps it running, and `flow ui` attaches to it.

## For agents and scripts

Every command answers in [TOON](https://toonformat.dev), a compact format agents read well, and
suggests the next commands to run:

```
$ flow status
count: 3
needs_you: 2
flows[3]{name,state,phase,tasks,age}:
  shop-221,waiting,plan,"",2s
  shop-217,blocked,implement,4/9,2s
  scratch,idle,"","",11s
blockers[2]{name,blocker}:
  shop-221,Should refunds count against the original order's totals or the refund date's?
  shop-217,"Task 5: the migration needs a default value"
help[2]: "Run `flow status <name>` for one flow's phases, timeline and decisions",Run `flow new --name <name> --request <text>` to track a new flow
```

Programs read `flow status --json`, which is versioned. The command line and the JSON are the only
public interface; the logs behind them are private.

| Command | Does |
| --- | --- |
| `flow` | every flow and what needs you |
| `flow ui [--stop]` | the terminal UI |
| `flow start <request>` | a worker in its own worktree |
| `flow start --plain <folder>` | an agent in a folder |
| `flow status [<flow>] [--json] [--all]` | where flows stand |
| `flow send <flow> <text>` | tell a worker something |
| `flow report <event>` | report into a flow by hand |
| `flow remove <flow>`, `flow clear` | tear down and forget |

[docs/cli.md](docs/cli.md) has every option.

## Similar tools

Plenty of tools run coding agents side by side in worktrees, among them Claude Squad, ccmanager,
dmux, amux, herdr, parallel-code and Conductor. Several of them do things flow doesn't. Conductor,
parallel-code and dmux have diff review and pull requests built in, and parallel-code reaches your
phone. If you need one of those, they'll suit you better.

[firstmate](https://github.com/kunchenguid/firstmate) is closest in spirit. A lead agent, the first
mate, starts crewmates in their own git worktrees. It ships as a repository of instructions, skills
and scripts that you start your agent in. Crewmates run in tmux by default (or herdr, cmux, Zellij
or Orca), and it has no dashboard of its own.

flow's take is to follow a feature rather than a session. Each flow has phases, task counts,
decisions, blockers and a timeline. It learns all of that from the agents' own hooks, and its
command line is made for agents, so the master can start, read and answer workers and gets woken
instead of polling. Each worktree gets its own setup, teardown and port slot.

flow has no diff viewer or pull request button (the worker's skill opens the merge request, and
flow notices), no desktop notifications yet, and no way to reach it from your phone. It runs on
macOS and Linux.

## Documentation

Start with [install](docs/install.md) and [flow ui](docs/ui.md). Then there's
[flows and the master](docs/flows.md), [configuration](docs/configuration.md),
[themes](docs/themes.md), [repository setup](docs/repository.md),
[the command line](docs/cli.md), [the JSON contract](docs/json.md), [files](docs/files.md) and
[troubleshooting](docs/troubleshooting.md). [CHANGELOG.md](CHANGELOG.md) says what changed and
[ROADMAP.md](ROADMAP.md) what comes next.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or
  <https://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or <https://opensource.org/licenses/MIT>)

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in
the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without
any additional terms or conditions.
