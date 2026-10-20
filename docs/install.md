# Install

```sh
curl -fsSL https://github.com/JanSuthacheeva/saaio/releases/latest/download/install.sh | sh
saaio --version    # saaio 0.1.0 (abc1234)
```

The script downloads the binary for this machine from the latest
[release](https://github.com/JanSuthacheeva/saaio/releases), checks it against the release's
`SHA256SUMS` and installs it to `~/.local/bin`. It edits no shell startup file: when the folder is
not on `PATH`, it prints the line to add.

| Variable | Default | Sets |
| --- | --- | --- |
| `SAAIO_VERSION` | the latest release | the tag to install, such as `v0.1.0` |
| `SAAIO_INSTALL_DIR` | `~/.local/bin` | the folder `saaio` is installed to |
| `SAAIO_DOWNLOAD_URL` | the GitHub release | a folder or mirror holding the release assets (`file://` works). `SHA256SUMS` is read from the same place, so it checks the download, not the source. |

```sh
curl -fsSL https://github.com/JanSuthacheeva/saaio/releases/latest/download/install.sh | SAAIO_VERSION=v0.1.0 sh
```

| Requirement | Notes |
| --- | --- |
| macOS or Linux | Apple silicon or Intel; x86_64 or aarch64. The Linux binaries are static and run on any distribution. |
| git | |
| [Claude Code](https://github.com/anthropics/claude-code) or [Codex](https://github.com/openai/codex) | either or both |

## Manual download

Each release holds one `saaio-<target>.tar.gz` per platform and a `SHA256SUMS` file:

| Target | Platform |
| --- | --- |
| `aarch64-apple-darwin` | macOS, Apple silicon |
| `x86_64-apple-darwin` | macOS, Intel |
| `x86_64-unknown-linux-musl` | Linux, x86_64 |
| `aarch64-unknown-linux-musl` | Linux, aarch64 |

```sh
url=https://github.com/JanSuthacheeva/saaio/releases/latest/download
curl -fsSLO "$url/saaio-aarch64-apple-darwin.tar.gz"
curl -fsSLO "$url/SHA256SUMS"
shasum -a 256 -c --ignore-missing SHA256SUMS    # on Linux: sha256sum -c --ignore-missing SHA256SUMS
tar xzf saaio-aarch64-apple-darwin.tar.gz saaio
mkdir -p ~/.local/bin && mv saaio ~/.local/bin/
```

Each tarball has a build provenance attestation: proof that the release workflow built it from
the tagged commit. With the [GitHub CLI](https://cli.github.com):

```sh
gh attestation verify saaio-aarch64-apple-darwin.tar.gz --repo JanSuthacheeva/saaio
```

The binaries are not signed. macOS refuses to open one downloaded with a browser until its
quarantine flag is removed: `xattr -d com.apple.quarantine ~/.local/bin/saaio`. curl sets no such
flag.

## From source

For other platforms, or to build it yourself. Requires Rust 1.95+, installed with
[rustup](https://rustup.rs).

```sh
cargo install --locked --git https://github.com/JanSuthacheeva/saaio --tag v0.1.0
```

Installs `saaio` to `~/.cargo/bin`. `saaio --version` names the commit it was built from, or
`unknown` when the source is not a git checkout of saaio. To name one anyway, set `SAAIO_COMMIT`
when building.

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
| `setup-saaio` | prepares a repository for saaio, recommended once per project ([repository.md](repository.md#the-setup-saaio-skill)) |

Optional. Own skills instead: [Use your own workflow](configuration.md#use-your-own-workflow). No
skills at all: [Without matching skills](configuration.md#without-matching-skills).

## Agent settings

saaio leaves permissions to each harness. Recommended settings for workers, such as Claude Code's
permission mode and allow rules and Codex's automatic review, sandbox network access and command
rules: [agents.md](agents.md).

## First run

```sh
saaio    # in a repository
```

Writes to `~/.config/saaio`:

| File | Holds |
| --- | --- |
| `config.toml` | every setting, commented out ([configuration.md](configuration.md)) |
| `theme.toml` | every color, commented out ([themes.md](themes.md)) |
| `master.md` | the master's instructions ([flows.md](flows.md#the-master)) |

| Prompt | Answer |
| --- | --- |
| an agent opening in a folder it does not trust yet (a Claude Code worker's new worktree, the master's or a plain session's folder) asks whether to trust it | answer in its pane |

Codex workers trust saaio's hooks and their own worktree without asking.

## Updating

Run the install script again, then restart the server:

```sh
curl -fsSL https://github.com/JanSuthacheeva/saaio/releases/latest/download/install.sh | sh
saaio --stop
saaio
```

From source: `cargo install --locked --git https://github.com/JanSuthacheeva/saaio --tag <new tag> --force`
instead of the script.

- The running server keeps the old version until stopped.
- Unfinished flows resume after the restart.
- `master.md` is refreshed while it still holds an earlier default. `config.toml` and `theme.toml`
  are written only when missing. Edited files stay as they are.
- Changes: [CHANGELOG.md](../CHANGELOG.md).

## Uninstalling

[files.md](files.md#removing-saaio).
