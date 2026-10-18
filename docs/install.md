# Install

```sh
cargo install --git https://github.com/JanSuthacheeva/saaio --tag v0.1.0
saaio --version    # saaio 0.1.0
```

Installs `saaio` to `~/.cargo/bin`.

| Requirement | Notes |
| --- | --- |
| macOS or Linux | |
| git | |
| Rust 1.95+ | build only. Install with [rustup](https://rustup.rs). |
| [Claude Code](https://github.com/anthropics/claude-code) or [Codex](https://github.com/openai/codex) | either or both |

## Skills

Default first prompt: `/ship-feature <request>` (`$ship-feature <request>` in Codex), a skill from
[agent-skills](https://github.com/JanSuthacheeva/agent-skills). Install with the
[skills CLI](https://github.com/vercel-labs/skills), which asks which agents to install for:

```sh
npx skills add JanSuthacheeva/agent-skills -g --skill ship-feature design-code implement-plan review-changes write-merge-request commit setup-saaio
```

| Skill | Does |
| --- | --- |
| `ship-feature` | takes a feature from plan to merge request, using the next four and `commit` |
| `design-code` | starts the plan phase |
| `implement-plan` | starts the implement phase |
| `review-changes` | starts the review phase |
| `write-merge-request` | starts the merge-request phase |
| `commit` | commits |
| `setup-saaio` | prepares a repository for saaio ([repository.md](repository.md#the-setup-saaio-skill)) |

Optional. Own skills instead: [Use your own workflow](configuration.md#use-your-own-workflow). No
skills at all: [Without matching skills](configuration.md#without-matching-skills).

## First run

```sh
saaio ui    # in a repository
```

Writes to `~/.config/saaio`:

| File | Holds |
| --- | --- |
| `config.toml` | every setting, commented out ([configuration.md](configuration.md)) |
| `theme.toml` | every color, commented out ([themes.md](themes.md)) |
| `master.md` | the master's instructions ([flows.md](flows.md#the-master)) |

| Prompt | Answer |
| --- | --- |
| an agent opening in a new folder asks whether to trust it | answer in its pane |
| the first Codex worker asks to review flow's hooks | "Trust all and continue". Asked again only when flow's hooks change. |

## Updating

```sh
cargo install --git https://github.com/JanSuthacheeva/saaio --tag <new tag> --force
saaio ui --stop
saaio ui
```

- The running server keeps the old version until stopped.
- Unfinished flows resume after the restart.
- `master.md`, `config.toml` and `theme.toml` are refreshed unless edited. Edited files stay as
  they are.
- Changes: [CHANGELOG.md](../CHANGELOG.md).

## Uninstalling

[files.md](files.md#removing-saaio).
