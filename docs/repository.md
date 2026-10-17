# Setting up a repository

None of this is required. `saaio start` works in any git repository. These additions let several
workers run side by side without collisions.

| File | Purpose |
| --- | --- |
| `.saaio/config.toml` | branch name convention |
| `.saaio/setup` | runs in each new worktree |
| `.saaio/teardown` | runs when a worktree is removed |

- [Worktrees](#worktrees)
- [Branch names](#branch-names)
- [Setup and teardown](#setup-and-teardown)
- [Slots and ports](#slots-and-ports)
- [A Docker Compose example](#a-docker-compose-example)
- [The setup-saaio skill](#the-setup-saaio-skill)

## Worktrees

- Each flow gets a [git worktree](https://git-scm.com/docs/git-worktree) in
  `.saaio/worktrees/<name>`, on a new branch from the current checkout's `HEAD`.
- The worker runs there. The main checkout is untouched.
- On first use saaio adds `.saaio/worktrees/` to `.git/info/exclude`.
- `saaio remove` and `saaio clear` remove worktrees ([cli.md](cli.md#saaio-remove)).

## Branch names

Default: `flow/<name>`. A committed `.saaio/config.toml` sets a convention:

```toml
[branch]
name  = "{type}/{ticket}-{slug}"
types = ["feature", "bugfix", "hotfix", "chore"]
```

```
$ saaio start --type bugfix "SHOP-230 fix totals on refunds"
flow: shop-230
...
branch: bugfix/SHOP-230-fix-totals-on-refunds

$ saaio start "add a dark mode to the admin panel please"
flow: add-a-dark-mode
...
branch: feature/add-a-dark-mode-to
```

| Key | Effect |
| --- | --- |
| `name` | the branch name, with placeholders |
| `types` | allowed types. The first is the default. Optional. |

| Placeholder | Value |
| --- | --- |
| `{type}` | `--type` of `saaio start`, the type picked in the new-flow dialog, or the first in `types` |
| `{ticket}` | the ticket key at the start of the request, as written (`SHOP-230`) |
| `{slug}` | up to five words of the request after the ticket key, lowercased, joined by `-` |
| `{name}` | the flow's name |

- An empty placeholder drops the separator next to it: without a ticket key,
  `feature/{ticket}-{slug}` gives `feature/add-a-dark-mode-to`.
- An existing branch name gets `-2`, `-3` and so on.
- The worktree folder uses the flow's name.

An unlisted type is refused:

```
$ saaio start --type nope "x"
error: "no branch type nope; this repo has feature, bugfix, hotfix, chore"
help[1]: "Name the types in .saaio/config.toml: [branch] types = [\"feature\", \"bugfix\"]"
```

## Setup and teardown

| Script | Runs |
| --- | --- |
| `.saaio/setup` | after `saaio start` creates the worktree, before the worker starts |
| `.saaio/teardown` | when `saaio remove` or `saaio clear` removes the worktree |

Both must be executable. They run from the main checkout with the worktree as working directory.

| Variable | Value |
| --- | --- |
| `SAAIO_SLOT` | the flow's slot, 1 to 20 |
| `SAAIO_WORKTREE` | the worktree's path |
| `SAAIO_REPO` | the main checkout's path |
| `SAAIO_FLOW_NAME` | the flow's name, such as `shop-217` |
| `SAAIO_FLOW_ID` | the flow's id |

The worker's session gets the same variables.

| Case | Result |
| --- | --- |
| setup runs | `saaio start` waits. Output is not shown. |
| setup exits non-zero | the flow is blocked with the setup's last error line. The worker still opens. |
| teardown fails | the removal goes on |
| a script changes | applies at once (run from the main checkout) |
| `compose.yaml` or `.gitignore` changes | reach new worktrees only once committed (worktrees are made from the last commit) |

```
$ saaio start "SHOP-240 broken setup"
flow: shop-240
...
setup: "failed: composer: lock file out of date"
```

## Slots and ports

- Every unfinished flow holds a slot, 1 to 20, unique across all projects.
- `saaio start` claims the lowest free slot before the setup runs.
- A done, ended or removed flow releases its slot.
- Use it for ports: slot 3 serves on `$((8000 + SAAIO_SLOT))`, port 8003.

All 20 in use:

```
error: all 20 slots are in use
help[2]: Run `saaio status --all` to see which flows hold them,Run `saaio clear` in projects with finished flows
```

## A Docker Compose example

`compose.yaml` reads its published ports from variables, defaulting to today's ports:

```yaml
services:
  app:
    ports:
      - "${HTTP_PORT:-8080}:80"
  db:
    ports:
      - "${DB_PORT:-3306}:3306"
```

```sh
#!/bin/sh
# .saaio/setup
set -e
export HTTP_PORT=$((18080 + SAAIO_SLOT)) DB_PORT=$((13306 + SAAIO_SLOT))
docker compose up -d --wait
docker compose exec -T app php artisan migrate --seed --force
```

```sh
#!/bin/sh
# .saaio/teardown
docker compose down -v --remove-orphans
```

- `docker compose up` without saaio uses the default ports.
- Compose names the project after its folder, so each worktree has its own stack.
  `docker compose exec` in a worktree reaches that worktree's stack.
- Remove fixed `container_name` entries. They clash between worktrees.
- Two repositories with same-named flows: set `COMPOSE_PROJECT_NAME` to the repository and flow
  name.

## The setup-saaio skill

[setup-saaio](https://github.com/JanSuthacheeva/agent-skills/tree/main/skills/setup-saaio) writes
both scripts for a project.

```sh
npx skills add JanSuthacheeva/agent-skills --skill setup-saaio
```

Then ask the agent in the repository: "set up saaio for this project".

- Reads how the project runs: Docker Compose, Laravel Sail, npm or vite dev servers, `.env` files.
- Plans a port per service and slot.
- Makes the compose ports configurable.
- Writes `.saaio/setup` and `.saaio/teardown` (`.env`, dependencies, migrations, ports).
- Tests them in a scratch worktree.
