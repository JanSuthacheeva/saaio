# Setting up a repository

None of this is required. `flow start` works in any git repository. These additions let several
workers run side by side without collisions.

| File | Purpose |
| --- | --- |
| `.flow/config.toml` | branch name convention |
| `.flow/setup` | runs in each new worktree |
| `.flow/teardown` | runs when a worktree is removed |

- [Worktrees](#worktrees)
- [Branch names](#branch-names)
- [Setup and teardown](#setup-and-teardown)
- [Slots and ports](#slots-and-ports)
- [A Docker Compose example](#a-docker-compose-example)
- [The setup-flow skill](#the-setup-flow-skill)

## Worktrees

- Each flow gets a [git worktree](https://git-scm.com/docs/git-worktree) in
  `.flow/worktrees/<name>`, on a new branch from the current checkout's `HEAD`.
- The worker runs there. The main checkout is untouched.
- On first use flow adds `.flow/worktrees/` to `.git/info/exclude`.
- `flow remove` and `flow clear` remove worktrees ([cli.md](cli.md#flow-remove)).

## Branch names

Default: `flow/<name>`. A committed `.flow/config.toml` sets a convention:

```toml
[branch]
name  = "{type}/{ticket}-{slug}"
types = ["feature", "bugfix", "hotfix", "chore"]
```

```
$ flow start --type bugfix "SHOP-230 fix totals on refunds"
flow: shop-230
...
branch: bugfix/SHOP-230-fix-totals-on-refunds

$ flow start "add a dark mode to the admin panel please"
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
| `{type}` | `--type` of `flow start`, the type picked in the new-flow dialog, or the first in `types` |
| `{ticket}` | the ticket key at the start of the request, as written (`SHOP-230`) |
| `{slug}` | up to five words of the request after the ticket key, lowercased, joined by `-` |
| `{name}` | the flow's name |

- An empty placeholder drops the separator next to it: without a ticket key,
  `feature/{ticket}-{slug}` gives `feature/add-a-dark-mode-to`.
- An existing branch name gets `-2`, `-3` and so on.
- The worktree folder uses the flow's name.

An unlisted type is refused:

```
$ flow start --type nope "x"
error: "no branch type nope; this repo has feature, bugfix, hotfix, chore"
help[1]: "Name the types in .flow/config.toml: [branch] types = [\"feature\", \"bugfix\"]"
```

## Setup and teardown

| Script | Runs |
| --- | --- |
| `.flow/setup` | after `flow start` creates the worktree, before the worker starts |
| `.flow/teardown` | when `flow remove` or `flow clear` removes the worktree |

Both must be executable. They run from the main checkout with the worktree as working directory.

| Variable | Value |
| --- | --- |
| `FLOW_SLOT` | the flow's slot, 1 to 20 |
| `FLOW_WORKTREE` | the worktree's path |
| `FLOW_REPO` | the main checkout's path |
| `FLOW_NAME` | the flow's name, such as `shop-217` |
| `FLOW_ID` | the flow's id |

The worker's session gets the same variables.

| Case | Result |
| --- | --- |
| setup runs | `flow start` waits. Output is not shown. |
| setup exits non-zero | the flow is blocked with the setup's last error line. The worker still opens. |
| teardown fails | the removal goes on |
| a script changes | applies at once (run from the main checkout) |
| `compose.yaml` or `.gitignore` changes | reach new worktrees only once committed (worktrees are made from the last commit) |

```
$ flow start "SHOP-240 broken setup"
flow: shop-240
...
setup: "failed: composer: lock file out of date"
```

## Slots and ports

- Every unfinished flow holds a slot, 1 to 20, unique across all projects.
- `flow start` claims the lowest free slot before the setup runs.
- A done, ended or removed flow releases its slot.
- Use it for ports: slot 3 serves on `$((8000 + FLOW_SLOT))`, port 8003.

All 20 in use:

```
error: all 20 slots are in use
help[2]: Run `flow status --all` to see which flows hold them,Run `flow clear` in projects with finished flows
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
# .flow/setup
set -e
export HTTP_PORT=$((18080 + FLOW_SLOT)) DB_PORT=$((13306 + FLOW_SLOT))
docker compose up -d --wait
docker compose exec -T app php artisan migrate --seed --force
```

```sh
#!/bin/sh
# .flow/teardown
docker compose down -v --remove-orphans
```

- `docker compose up` without flow uses the default ports.
- Compose names the project after its folder, so each worktree has its own stack.
  `docker compose exec` in a worktree reaches that worktree's stack.
- Remove fixed `container_name` entries. They clash between worktrees.
- Two repositories with same-named flows: set `COMPOSE_PROJECT_NAME` to the repository and flow
  name.

## The setup-flow skill

[setup-flow](https://github.com/JanSuthacheeva/agent-skills/tree/main/skills/setup-flow) writes
both scripts for a project.

```sh
npx skills add JanSuthacheeva/agent-skills --skill setup-flow
```

Then ask the agent in the repository: "set up flow for this project".

- Reads how the project runs: Docker Compose, Laravel Sail, npm or vite dev servers, `.env` files.
- Plans a port per service and slot.
- Makes the compose ports configurable.
- Writes `.flow/setup` and `.flow/teardown` (`.env`, dependencies, migrations, ports).
- Tests them in a scratch worktree.
