# Setting up a repository

flow needs nothing from a repository to start a flow. This page covers what a repository can add so
several workers run side by side without getting in each other's way: its own branch names, a
setup and a teardown script, and a port range per worktree.

- [Worktrees](#worktrees)
- [Branch names](#branch-names)
- [Setup and teardown](#setup-and-teardown)
- [Slots and ports](#slots-and-ports)
- [A Docker Compose example](#a-docker-compose-example)
- [The setup-flow skill](#the-setup-flow-skill)

## Worktrees

`flow start` gives every flow its own [git worktree](https://git-scm.com/docs/git-worktree) in
`.flow/worktrees/<name>`, on a new branch made from what the checkout you run it in has checked
out. The worker works there, so your own checkout stays as it is.

flow adds `.flow/worktrees/` to `.git/info/exclude` the first time, so the worktrees never show in
`git status`. `flow remove` and `flow clear` remove them again (see [cli.md](cli.md#flow-remove)).

## Branch names

A flow's branch is `flow/<name>`, unless the repository commits its own convention in
`.flow/config.toml`:

```toml
[branch]
name  = "{type}/{ticket}-{slug}"
types = ["feature", "bugfix", "hotfix", "chore"]
```

| Key | Does |
| --- | --- |
| `name` | the branch name, with placeholders |
| `types` | the types a flow can be; the first is the default. Optional. |

| Placeholder | Becomes |
| --- | --- |
| `{type}` | the type: `--type` of `flow start`, the type picked in the new-flow dialog, or the first in `types` |
| `{ticket}` | the ticket key at the start of the request, as written (`SHOP-230`) |
| `{slug}` | up to five words of the request after the ticket key, lowercased and joined by `-` |
| `{name}` | the flow's name |

With the convention above:

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

A placeholder with nothing to fill takes the separator next to it along: without a ticket key,
`feature/{ticket}-{slug}` becomes `feature/add-a-dark-mode-to`, not `feature/-add-a-dark-mode-to`.
A branch that already exists gets `-2`, `-3` and so on. The worktree folder keeps the flow's name.

A type the repository does not list is refused:

```
$ flow start --type nope "x"
error: "no branch type nope; this repo has feature, bugfix, hotfix, chore"
help[1]: "Name the types in .flow/config.toml: [branch] types = [\"feature\", \"bugfix\"]"
```

## Setup and teardown

A repository can have two executable scripts:

- `.flow/setup` runs after `flow start` created the worktree, before the worker starts.
- `.flow/teardown` runs when `flow remove` or `flow clear` removes the worktree.

flow runs them from the main checkout, with the worktree as their working folder, and sets:

| Variable | Holds |
| --- | --- |
| `FLOW_SLOT` | the flow's slot, 1 to 20 |
| `FLOW_WORKTREE` | the worktree's path |
| `FLOW_REPO` | the main checkout's path |
| `FLOW_NAME` | the flow's name, such as `shop-217` |
| `FLOW_ID` | the flow's id |

The worker's session gets the same variables, so it can reach its own services.

`flow start` waits for the setup, and keeps its output to itself. When the setup fails (exits with
another code than 0), the flow is marked blocked with the setup's last error line, and the worker
still opens so you can look:

```
$ flow start "SHOP-240 broken setup"
flow: shop-240
...
setup: "failed: composer: lock file out of date"
```

A failing teardown does not stop the removal.

Since the scripts run from the main checkout, a change to them applies at once. The worktree,
though, is made from the last commit: a change to `compose.yaml` or `.gitignore` reaches new
workers only once it is committed.

## Slots and ports

Every unfinished flow holds a slot, a number from 1 to 20 that no other unfinished flow holds, in
this project or any other. `flow start` takes the lowest free one before the setup runs, and a flow
gives it back when it is done, ended or removed. With all 20 in use, `flow start` refuses:

```
error: all 20 slots are in use
help[2]: Run `flow status --all` to see which flows hold them,Run `flow clear` in projects with finished flows
```

Use the slot for anything that must not collide between worktrees, such as ports: a worker on
slot 3 can serve its app on `$((8000 + FLOW_SLOT))`, port 8003, while another serves on 8004.

## A Docker Compose example

For a project whose services run in Docker Compose, first let `compose.yaml` read its published
ports from variables, keeping today's ports as defaults:

```yaml
services:
  app:
    ports:
      - "${HTTP_PORT:-8080}:80"
  db:
    ports:
      - "${DB_PORT:-3306}:3306"
```

`docker compose up` without flow does what it did before. Then:

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

Compose names a project after its folder, so each worktree runs its own stack, and
`docker compose exec ...` inside a worktree reaches that worktree's stack. Fixed `container_name`
entries would clash between worktrees, so remove them. Two repositories can have flows of the same
name; setting `COMPOSE_PROJECT_NAME` to the repository's and the flow's name together keeps their
stacks apart.

## The setup-flow skill

Writing these scripts well for a real project takes some care: the `.env` file, installing
dependencies, migrations, every published port. The
[setup-flow](https://github.com/JanSuthacheeva/agent-skills/tree/main/skills/setup-flow) skill does
it with you. It reads how the project runs (Docker Compose, Laravel Sail, npm or vite dev servers,
`.env` files), plans a port per service and slot, makes the compose ports configurable, writes
`.flow/setup` and `.flow/teardown`, and tries them in a scratch worktree before it hands over.

```sh
npx skills add JanSuthacheeva/agent-skills --skill setup-flow
```

Then ask your agent, in the repository: "set up flow for this project".
