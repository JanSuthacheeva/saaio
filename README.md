# flow

![flow ui with Claude Code and Codex workers](docs/media/flow-ui.png)

I usually have several coding agents working at once, each on its own feature. flow is the tool I
built to keep track of them: it opens each agent in its own git worktree, shows where every one
stands, and tells me when one needs me. I use it every day with Claude Code and Codex, and thought
it might help others too.

flow is a terminal UI, `flow ui`, and a command line whose main user is an agent.

## Quick start

You need macOS or Linux, git, Rust 1.95 or newer, and Claude Code or Codex.

```sh
cargo install --git https://github.com/JanSuthacheeva/flow --tag v0.1.0
```

Each worker starts with `/ship-feature <request>` (`$ship-feature` in Codex), a skill from
[agent-skills](https://github.com/JanSuthacheeva/agent-skills) that takes a feature from plan to
merge request. Install it and the skills it runs, or set your own first prompt (see
[configuration](docs/configuration.md#the-first-prompt)):

```sh
npx skills add JanSuthacheeva/agent-skills -g --skill ship-feature design-code implement-plan review-changes write-merge-request commit setup-flow
```

Then, in a repository:

```sh
flow ui
```

The big pane is the master, an agent that works with flow for you. Ask it to start a flow ("start
a flow for SHOP-217 add CSV export"), or press `C-space n`. Each worker shows up in the sidebar on
the right, grouped by what it needs from you. `C-space ?` lists every key.

![Moving through flow ui: the peek, a worker, the board](docs/media/flow-ui.gif)

On its first run flow writes `config.toml`, `theme.toml` and `master.md` to `~/.config/flow`, every
setting commented out at its default.

## What it does

- **Works with different agents.** Claude Code and Codex are built in, each followed through its
  own hooks. Any other command can run as a worker; flow then shows only whether it runs. The
  phases a flow walks through, the skills that start each phase and every agent's first prompt are
  settings.
- **Gives each worktree its own environment.** `.flow/setup` and `.flow/teardown` run in every
  worktree. `FLOW_SLOT` (1 to 20) is unique across all your projects, so each worktree's Docker
  stack can get its own ports. The `setup-flow` skill writes both scripts for your repo and tries
  them in a scratch worktree.
- **Has a master that is woken.** The master starts workers, answers them with `flow send` and is
  woken when one waits, is blocked or finishes. In a guided flow the questions come to you; in an
  auto flow the master answers what the code and the plan already settle.
- **Plain sessions.** An agent in any folder, without a worktree or a flow, next to your workers.
- **Themes.** Your terminal's colors by default, flow's own dark and light themes built in,
  reloaded while you edit. Another tool can write a theme for flow to follow (a Noctalia template
  is in the docs).
- **Sessions outlive the UI.** Closing the terminal leaves every agent running; after a reboot
  flow resumes each conversation.

## How it works

Every flow is an append-only log of events. The agents' own hooks run `flow hook`, which turns what
happens into events: a skill started a phase, a question is waiting, a merge request was opened.
flow folds the logs into where each flow stands: its phase and how long each took, tasks,
blockers, decisions and a timeline. `flow server` runs each agent in its own terminal and keeps it
running; `flow ui` attaches to it.

## For agents and scripts

Every command answers in [TOON](https://toonformat.dev), a compact format agents read well, with
the next steps beside it:

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

Programs read `flow status --json`, a versioned contract. The command line and the JSON are the
only public interface; the logs are private.

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

Every option is in [docs/cli.md](docs/cli.md).

## Similar tools

Many tools run coding agents side by side in worktrees: Claude Squad, ccmanager, dmux, amux, herdr,
parallel-code and Conductor among them, and several do things flow does not. Conductor,
parallel-code and dmux have diff review and pull requests built in; parallel-code reaches your
phone; herdr and ccmanager recognise many more agents. If you need one of those, they will suit you
better.

What flow does differently:

- It follows a feature, not only a session: phases, task counts, decisions, blockers and a
  timeline per flow.
- It learns what an agent does from the agent's own hooks instead of reading its screen.
- Its command line is made for agents, so a master agent can start, read and answer workers, and
  it is woken instead of polling.
- Each worktree gets its own setup, teardown and port slot.

Where flow falls short: it has no diff viewer or pull request button (the worker's skill opens the
merge request and flow notices it), no desktop notifications, no phone access, and it follows only
Claude Code and Codex. It runs on macOS and Linux only.

## Documentation

[Install](docs/install.md), [flow ui](docs/ui.md), [flows and the master](docs/flows.md),
[configuration](docs/configuration.md), [themes](docs/themes.md),
[repository setup](docs/repository.md), [the command line](docs/cli.md),
[the JSON contract](docs/json.md), [files](docs/files.md),
[troubleshooting](docs/troubleshooting.md). What changed: [CHANGELOG.md](CHANGELOG.md). What
comes next: [ROADMAP.md](ROADMAP.md).

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or
  <https://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or <https://opensource.org/licenses/MIT>)

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in
the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without
any additional terms or conditions.
