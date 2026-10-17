# Setting up a repository

A repository doesn't need anything special for flow. Run `flow start` in any git repository and you
get a worker in its own worktree. This page is about what you can add so several workers run side
by side without stepping on each other: your own branch names, a setup and a teardown script, and
a range of ports per worktree.

- [Worktrees](#worktrees)
- [Branch names](#branch-names)
- [Setup and teardown](#setup-and-teardown)
- [Slots and ports](#slots-and-ports)
- [A Docker Compose example](#a-docker-compose-example)
- [The setup-flow skill](#the-setup-flow-skill)

## Worktrees

`flow start` gives every flow its own [git worktree](https://git-scm.com/docs/git-worktree) in
`.flow/worktrees/<name>`, on a new branch made from whatever the checkout you run it in has checked
out. The worker works there, so your own checkout stays as it is.

The first time, flow adds `.flow/worktrees/` to `.git/info/exclude`, so the worktrees never show up
in `git status`. `flow remove` and `flow clear` remove them again (see [cli.md](cli.md#flow-remove)).

## Branch names

A flow's branch is `flow/<name>`, unless the repository commits its own convention in
`.flow/config.toml`:

```toml
[branch]
name  = "{type}/{ticket}-{slug}"
types = ["feature", "bugfix", "hotfix", "chore"]
```

With that convention, branches come out like this:

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

When a placeholder has nothing to fill in, the separator next to it goes too. Without a ticket key,
`feature/{ticket}-{slug}` becomes `feature/add-a-dark-mode-to`, not `feature/-add-a-dark-mode-to`.
If the branch already exists, flow adds `-2`, `-3` and so on. The worktree folder keeps the flow's
name either way.

A type the repository doesn't list is refused:

```
$ flow start --type nope "x"
error: "no branch type nope; this repo has feature, bugfix, hotfix, chore"
help[1]: "Name the types in .flow/config.toml: [branch] types = [\"feature\", \"bugfix\"]"
```

## Setup and teardown

A repository can have two executable scripts. `.flow/setup` runs after `flow start` has created the
worktree and before the worker starts. `.flow/teardown` runs when `flow remove` or `flow clear`
removes the worktree.

flow runs them from the main checkout, with the worktree as the working folder, and sets these
variables:

| Variable | Holds |
| --- | --- |
| `FLOW_SLOT` | the flow's slot, 1 to 20 |
| `FLOW_WORKTREE` | the worktree's path |
| `FLOW_REPO` | the main checkout's path |
| `FLOW_NAME` | the flow's name, such as `shop-217` |
| `FLOW_ID` | the flow's id |

The worker's session gets the same variables, so it can reach its own services.

`flow start` waits for the setup and keeps its output to itself. If the setup fails (exits with
anything but 0), the flow is marked blocked with the setup's last error line. The worker still
opens, so you can look into it:

```
$ flow start "SHOP-240 broken setup"
flow: shop-240
...
setup: "failed: composer: lock file out of date"
```

A failing teardown doesn't stop the removal.

Because the scripts run from the main checkout, a change to them applies right away. The worktree,
on the other hand, is made from the last commit, so a change to `compose.yaml` or `.gitignore` only
reaches new workers once it's committed.

## Slots and ports

Every unfinished flow holds a slot, a number from 1 to 20 that no other unfinished flow holds, in
this project or any other. Use it for anything that mustn't collide between worktrees, such as
ports. A worker on slot 3 can serve its app on `$((8000 + FLOW_SLOT))`, port 8003, while the one on
slot 4 serves on 8004.

`flow start` takes the lowest free slot before the setup runs, and a flow gives it back when it's
done, ended or removed. When all 20 are in use, `flow start` refuses:

```
error: all 20 slots are in use
help[2]: Run `flow status --all` to see which flows hold them,Run `flow clear` in projects with finished flows
```

## A Docker Compose example

If your project's services run in Docker Compose, first let `compose.yaml` read its published ports
from variables, keeping today's ports as the defaults:

```yaml
services:
  app:
    ports:
      - "${HTTP_PORT:-8080}:80"
  db:
    ports:
      - "${DB_PORT:-3306}:3306"
```

`docker compose up` without flow still does what it did before. Then the two scripts:

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
entries would clash between worktrees, so remove them. Two repositories can have flows with the
same name; setting `COMPOSE_PROJECT_NAME` to the repository's and the flow's name together keeps
their stacks apart.

## The setup-flow skill

Writing these scripts well for a real project takes some care: the `.env` file, installing
dependencies, migrations, every published port. The
[setup-flow](https://github.com/JanSuthacheeva/agent-skills/tree/main/skills/setup-flow) skill does
it with you. It reads how the project runs (Docker Compose, Laravel Sail, npm or vite dev servers,
`.env` files), plans a port per service and slot, and makes the compose ports configurable. Then it
writes `.flow/setup` and `.flow/teardown` and tries them in a scratch worktree before handing over.

```sh
npx skills add JanSuthacheeva/agent-skills --skill setup-flow
```

Then, in the repository, ask your agent to "set up flow for this project".
