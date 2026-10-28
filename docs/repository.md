# Setting up a repository

None of this is required. `saaio start` works in any git repository. These additions let several
workers run side by side without collisions.

Recommended for every project: run the [setup-saaio skill](#the-setup-saaio-skill) once. It writes
`.saaio/setup` and `.saaio/teardown` for the project, and `.saaio/pause` and `.saaio/resume` when
the setup starts services.

| File | Purpose |
| --- | --- |
| `.saaio/config.toml` | branch name convention |
| `.saaio/setup` | runs in each new worktree |
| `.saaio/teardown` | runs when a worktree is removed |
| `.saaio/pause` | stops a resting flow's services |
| `.saaio/resume` | starts them again when the worker works |

- [Worktrees](#worktrees)
- [Branch names](#branch-names)
- [Trust](#trust)
- [Setup and teardown](#setup-and-teardown)
- [Pause and resume](#pause-and-resume)
- [Slots and ports](#slots-and-ports)
- [A Docker Compose example](#a-docker-compose-example)
- [The setup-saaio skill](#the-setup-saaio-skill)

## Worktrees

- Each flow gets a [git worktree](https://git-scm.com/docs/git-worktree) in
  `.saaio/worktrees/<name>`, on a new branch from the current checkout's `HEAD`. A research flow's
  worktree has no branch: it is detached at that commit.
- The worker runs there. The main checkout is untouched.
- `saaio start` refuses a repository whose `.saaio` or `.saaio/worktrees` is a symbolic link, so a
  worktree is never created outside the repository.
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
$ saaio start --kind fix "SHOP-230 fix totals on refunds"
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
| `types` | allowed types. The flow's kind picks the first of its own the repo lists, else the first here. Optional. |

| Placeholder | Value |
| --- | --- |
| `{type}` | `--type` of `saaio start`, the type picked in the new-flow dialog, else the first of the kind's `branch_types` that `types` lists (`bugfix` for a fix), else the first in `types` |
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

## Trust

The scripts in `.saaio` are code from the repository. saaio runs them, and lets a Codex worker
trust its worktree, only once you trusted the repository. A repository without scripts needs no
trust for a Claude Code worker.

| Command | Does |
| --- | --- |
| `saaio trust` | trusts the repository you are in, from now on. Only in your own terminal: an agent's shell is refused. It lists the scripts that will run. |
| `saaio trust --revoke` | takes it back. saaio asks again before it runs the scripts. |
| `saaio trust --list` | every trusted repository |

Read the scripts before you trust a repository you did not write: a script changed later is not
asked about again. The trust holds for the repository's folder, in every store.

| Case | Result |
| --- | --- |
| `saaio start` in a repository not trusted yet | refuses and creates nothing. The error names the scripts that would run. |
| the new-flow dialog in `saaio` | asks you, then starts the flow |
| `saaio remove` or `saaio clear` in a repository no longer trusted | the worktree goes, the teardown does not run (`teardown: not run, the repository is not trusted`) |
| a resting flow in a repository no longer trusted | its services are not paused. Paused ones are not resumed: the resume fails with `the repository is not trusted`, and its tool calls stop waiting. |

```
$ saaio start "SHOP-240 export"
error: "~/projects/shop is not trusted: saaio would run .saaio/setup and .saaio/teardown"
help[2]: "Ask the person to read its .saaio scripts, then run `saaio trust` in ~/projects/shop in their terminal","Or start the flow in `saaio`, which asks them"
```

## Setup and teardown

| Script | Runs |
| --- | --- |
| `.saaio/setup` | after `saaio start` creates the worktree, before the worker starts |
| `.saaio/teardown` | before `saaio remove` or `saaio clear` removes the worktree. `saaio clear` runs it even on a worktree it then keeps for its uncommitted work. |

Both must be executable. They run from the main checkout with the worktree as working directory,
once the repository is [trusted](#trust).

| Variable | Value |
| --- | --- |
| `SAAIO_SLOT` | the flow's slot, 1 to 20 |
| `SAAIO_WORKTREE` | the worktree's path |
| `SAAIO_REPO` | the main checkout's path |
| `SAAIO_FLOW_NAME` | the flow's name, such as `shop-217` |
| `SAAIO_FLOW_ID` | the flow's id |
| `SAAIO_BASE` | the branch the flow started from with `saaio start --base`. Unset without one. |

The worker's session gets the same variables.

Teardown, pause and resume also get `GIT_DIR`, the worktree's entry in the main checkout's
`.git/worktrees/`, and `GIT_WORK_TREE`, the worktree. Git in them always acts on the flow's
worktree, whatever its `.git` file says. A script that wants git on the main checkout clears them:
`env -u GIT_DIR -u GIT_WORK_TREE git -C "$SAAIO_REPO" status`. A worktree git no longer lists
fails these scripts without running them.

| Case | Result |
| --- | --- |
| setup runs | `saaio start` waits. Output is not shown. |
| setup exits non-zero | the flow is blocked with the setup's last error line. The worker still opens. |
| setup runs longer than 10 minutes | it is stopped with everything it started, and the flow is blocked with `setup failed: no answer within 10m` |
| teardown fails | the removal goes on |
| teardown runs longer than 2 minutes | it is stopped with everything it started, and the removal goes on |
| a script changes | applies at once (run from the main checkout) |
| `compose.yaml` or `.gitignore` changes | reach new worktrees only once committed (worktrees are made from the last commit) |

```
$ saaio start "SHOP-240 broken setup"
flow: shop-240
...
setup: "failed: composer: lock file out of date"
```

## Pause and resume

Many flows with a Docker stack each use a lot of CPU and memory, though most of them wait for you.
`saaio server` pauses the services of a flow that rests and starts them again before its worker
needs them.

| Script | Runs |
| --- | --- |
| `.saaio/pause` | the flow rested (idle, waiting, blocked, done or ended) for `pause_after_minutes`, 5 by default ([configuration.md](configuration.md#services)) |
| `.saaio/resume` | the paused flow's worker works again: a new prompt, or its first tool call after the master woke it |

They run like teardown: from the main checkout, in the worktree, with the same variables and git
pinned to the worktree. A repo needs both, executable; with one or neither nothing is paused and
nothing shows.

While services pause, are paused or resume, every tool call of the worker waits for them, up to 2
minutes. The model's thinking before its first tool call hides most of a resume.

| Case | Result |
| --- | --- |
| a script runs | on a thread of `saaio server`; at most 5 minutes, then it is stopped with everything it started, and fails |
| pause fails | the flow's timeline says `pause failed: <last error line>`. It is tried again once the flow's state changed. |
| resume fails | the tool runs anyway. The board card says `working · services down`, `saaio status` says `resume-failed` with the error. It is tried again at the next turn. |
| resume takes longer than 2 minutes | the tool runs. The script goes on. |
| the worker works while a pause runs | tool calls wait for the pause, then for the resume |
| paused, and no `saaio server` runs | a tool call waits 2 seconds, then runs |
| the server stops during a script | the next server runs it again, so both scripts must be idempotent |
| `saaio remove` or `saaio clear` | teardown runs as always, on the paused services |
| a plain session, or not responding | never paused |

## Slots and ports

- Every unfinished flow except a plain session holds a slot, 1 to 20, unique across all projects.
- `saaio start` claims the lowest free slot before the setup runs.
- An ended or removed flow releases its slot. A done flow (its merge request open) keeps it until
  its session ends.
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

```sh
#!/bin/sh
# .saaio/pause
docker compose stop
```

```sh
#!/bin/sh
# .saaio/resume
export HTTP_PORT=$((18080 + SAAIO_SLOT)) DB_PORT=$((13306 + SAAIO_SLOT))
docker compose up -d --no-recreate --wait
```

- `docker compose up` without saaio uses the default ports.
- Compose names the project after its folder, so each worktree has its own stack.
  `docker compose exec` in a worktree reaches that worktree's stack.
- Pause keeps the containers and their data; resume starts the same ones and returns once they
  are healthy.
- Remove fixed `container_name` entries. They clash between worktrees.
- Two repositories with same-named flows: set `COMPOSE_PROJECT_NAME` to the repository and flow
  name.

## The setup-saaio skill

[setup-saaio](https://github.com/JanSuthacheeva/agent-skills/tree/main/skills/setup-saaio) writes
the scripts for a project. Install it once, globally, and run it in every project saaio works in.

```sh
npx skills add JanSuthacheeva/agent-skills -g --skill setup-saaio
```

Then ask the agent in the repository: "set up saaio for this project".

- Reads how the project runs: Docker Compose, Laravel Sail, npm or vite dev servers, `.env` files.
- Plans a port per service and slot.
- Makes the compose ports configurable.
- Writes `.saaio/setup` and `.saaio/teardown` (`.env`, dependencies, migrations, ports).
- Writes `.saaio/pause` and `.saaio/resume` when the setup starts services.
- Tests them in a scratch worktree.
