# Install

```sh
cargo install --git https://github.com/JanSuthacheeva/flow --tag v0.1.0
```

That puts `flow` in `~/.cargo/bin`. Check that it's there:

```
$ flow --version
flow 0.1.0
```

To build it you need Rust 1.95 or newer ([rustup](https://rustup.rs) installs it). To run it you
need macOS or Linux, git, and [Claude Code](https://github.com/anthropics/claude-code) or
[Codex](https://github.com/openai/codex), or both.

## Skills

Each worker starts with `/ship-feature <request>` (`$ship-feature <request>` in Codex). That's a
skill from [agent-skills](https://github.com/JanSuthacheeva/agent-skills) that takes a feature from
a plan to a merge request, using `design-code`, `implement-plan`, `review-changes`,
`write-merge-request` and `commit` along the way. The same set has `setup-flow`, which prepares a
repository for flow (see [repository.md](repository.md#the-setup-flow-skill)).

To use them, install them for your user with the [skills CLI](https://github.com/vercel-labs/skills).
It asks which agents to install them for.

```sh
npx skills add JanSuthacheeva/agent-skills -g --skill ship-feature design-code implement-plan review-changes write-merge-request commit setup-flow
```

If you'd rather use your own skills, skip this and set the first prompt and the phases in
`config.toml` instead. [Use your own workflow](configuration.md#use-your-own-workflow) shows how,
and what a flow looks like with no matching skills at all.

## First run

In a repository:

```sh
flow ui
```

The first run writes three files to `~/.config/flow`, each holding its defaults:

- `config.toml`, every setting commented out (see [configuration.md](configuration.md))
- `theme.toml`, every color commented out (see [themes.md](themes.md))
- `master.md`, the master's instructions (see [flows.md](flows.md#the-master))

The first time an agent opens in a new folder, it may ask whether you trust that folder. Answer in
its pane. The first Codex worker also asks you to review flow's hooks; pick "Trust all and
continue". Codex remembers that until flow's hooks change.

## Updating

```sh
cargo install --git https://github.com/JanSuthacheeva/flow --tag <new tag> --force
flow ui --stop
flow ui
```

The server that's already running keeps the old version until you stop it. Unfinished flows resume
after the restart. A newer flow also refreshes `master.md`, `config.toml` and `theme.toml` as long
as you haven't edited them; files you've edited stay as they are. [CHANGELOG.md](../CHANGELOG.md)
says what changed.

## Uninstalling

See [files.md](files.md#removing-flow).
