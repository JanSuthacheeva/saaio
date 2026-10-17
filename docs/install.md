# Install

## What you need

- macOS or Linux,
- git,
- Rust 1.95 or newer, to build flow ([rustup](https://rustup.rs) installs it),
- [Claude Code](https://github.com/anthropics/claude-code) or [Codex](https://github.com/openai/codex),
  or both.

## flow

```sh
cargo install --git https://github.com/JanSuthacheeva/flow --tag v0.1.0
```

This puts `flow` in `~/.cargo/bin`. Check it:

```
$ flow --version
flow 0.1.0
```

## The skills

Each worker starts with `/ship-feature <request>` (`$ship-feature <request>` in Codex). That is a
skill from [agent-skills](https://github.com/JanSuthacheeva/agent-skills) that takes a feature from
a plan to a merge request, using the other skills of the set: `design-code`, `implement-plan`,
`review-changes`, `write-merge-request` and `commit`. `setup-flow` prepares a repository for
flow (see [repository.md](repository.md#the-setup-flow-skill)). Install them for your user with
the [skills CLI](https://github.com/vercel-labs/skills):

```sh
npx skills add JanSuthacheeva/agent-skills -g --skill ship-feature design-code implement-plan review-changes write-merge-request commit setup-flow
```

It asks which agents to install them for. The skills are optional: with your own skills, set the
first prompt and the phases in `config.toml` (see [configuration.md](configuration.md#the-first-prompt)).

## The first run

In a repository:

```sh
flow ui
```

On its first run flow writes three files to `~/.config/flow`, each with its defaults:

- `config.toml`, every setting commented out (see [configuration.md](configuration.md)),
- `theme.toml`, every color commented out (see [themes.md](themes.md)),
- `master.md`, the master's instructions (see [flows.md](flows.md#the-master)).

The first time an agent opens in a new folder, it may ask whether you trust that folder; answer in
its pane. The first Codex worker also asks you to review flow's hooks: pick "Trust all and
continue". Codex remembers that until flow's hooks change.

## Updating

```sh
cargo install --git https://github.com/JanSuthacheeva/flow --tag <new tag> --force
flow ui --stop
flow ui
```

The running server keeps the old version until you stop it. Unfinished flows resume after the
restart. A newer flow refreshes `master.md`, `config.toml` and `theme.toml` while you have not
edited them; edited files stay as they are. See [CHANGELOG.md](../CHANGELOG.md) for what changed.

## Uninstalling

See [files.md](files.md#removing-flow).
