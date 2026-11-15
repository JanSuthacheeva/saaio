# Setting up a repository

- [Introduction](#introduction)
- [Worktrees](#worktrees)
- [The setup-saaio skill](#the-setup-saaio-skill)
- [Trust](#trust)
    - [Trusting a repository](#trusting-a-repository)
    - [Taking trust back](#taking-trust-back)
- [Setup and teardown](#setup-and-teardown)
    - [The scripts' environment](#the-scripts-environment)
    - [When setup or teardown fails](#when-setup-or-teardown-fails)
- [Slots and ports](#slots-and-ports)
- [A Docker Compose example](#a-docker-compose-example)
- [Pause and resume](#pause-and-resume)
    - [Waiting for services](#waiting-for-services)
    - [When pause or resume fails](#when-pause-or-resume-fails)
- [Branch names](#branch-names)
    - [Placeholders](#placeholders)
    - [Branch types](#branch-types)
- [Heavy commands](#heavy-commands)
    - [How a command matches](#how-a-command-matches)
    - [Waiting for a turn](#waiting-for-a-turn)

## Introduction

saaio works in any git repository without any setup at all. `saaio start` gives each flow its own
worktree and leaves your main checkout alone, so you can start a worker in a project you have
never told saaio anything about.

Once several workers run side by side, though, they start to get in each other's way. Two dev
servers want the same port, two Docker stacks fight over one container name, and two test suites
running at once leave a small machine crawling. The files in your repository's `.saaio` folder
are how you tell saaio about your project, so every worker gets its own ports, its own services
and a fair turn at the heavy commands:

| File | Purpose |
| --- | --- |
| `.saaio/config.toml` | branch name convention, heavy commands |
| `.saaio/setup` | runs in each new worktree |
| `.saaio/teardown` | runs when a worktree is removed |
| `.saaio/pause` | stops a resting flow's services |
| `.saaio/resume` | starts them again when the worker works |

> [!TIP]
> You rarely need to write these scripts by hand. Run the [setup-saaio skill](#the-setup-saaio-skill)
> once in every project: it writes `.saaio/setup` and `.saaio/teardown` for you, and
> `.saaio/pause` and `.saaio/resume` when the setup starts services.

## Worktrees

Every flow gets a [git worktree](https://git-scm.com/docs/git-worktree) of its own in
`.saaio/worktrees/<name>`, on a new branch from your current checkout's `HEAD`. The worker runs
there, so your main checkout is never touched while it works. A research flow's worktree has no
branch: it is detached at that commit.

```
.saaio/worktrees/shop-217    # the worker of flow shop-217 runs here
```

The first time saaio creates a worktree in a repository, it adds `.saaio/worktrees/` to
`.git/info/exclude`, so the worktrees never show up in `git status`. When a flow is finished,
`saaio remove` and `saaio clear` remove its worktree again ([cli.md](cli.md#saaio-remove)).

> [!NOTE]
> `saaio start` refuses a repository whose `.saaio` or `.saaio/worktrees` is a symbolic link, so a
> worktree is never created outside the repository.

## The setup-saaio skill

Every project runs a little differently, so the scripts that give each worker its own stack are
different in every project too. The
[setup-saaio](https://github.com/JanSuthacheeva/agent-skills/tree/main/skills/setup-saaio) skill
writes them for you. Install it once, globally:

```sh
npx skills add JanSuthacheeva/agent-skills -g --skill setup-saaio
```

Then open an agent in the repository and ask it to "set up saaio for this project". The skill
reads how the project runs (Docker Compose, Laravel Sail, npm or vite dev servers, `.env` files)
and plans a port for every service and slot. It then makes the compose ports configurable, writes
`.saaio/setup` and `.saaio/teardown` (`.env`, dependencies, migrations, ports), writes
`.saaio/pause` and `.saaio/resume` when the setup starts services, and tests them all in a scratch
worktree.

The rest of this page explains what those scripts do and when saaio runs them, so you can read,
adjust or write them yourself.

## Trust

The scripts in `.saaio` are code from the repository, and saaio runs them on your machine. That is
why it runs them, and lets a Codex worker trust its worktree, only once you trusted the
repository. A repository without scripts needs no trust for a Claude Code worker.

### Trusting a repository

Run `saaio trust` in the repository to trust it from now on. It lists the scripts that will run:

```sh
saaio trust           # trust the repository you are in
saaio trust --list    # every trusted repository
```

`saaio trust` only works in your own terminal: an agent's shell is refused, so a worker can never
trust a repository on your behalf. The trust holds for the repository's folder, in every store.

> [!WARNING]
> Read the scripts before you trust a repository you did not write. A script that changes later is
> not asked about again.

Until you do, `saaio start` refuses the repository and creates nothing. Its error names the
scripts that would run:

```
$ saaio start "SHOP-240 export"
error: "~/projects/shop is not trusted: saaio would run .saaio/setup and .saaio/teardown"
help[2]: "Ask the person to read its .saaio scripts, then run `saaio trust` in ~/projects/shop in their terminal","Or start the flow in `saaio`, which asks them"
```

The new-flow dialog in `saaio` asks you instead, and starts the flow once you said yes.

### Taking trust back

`saaio trust --revoke` takes the trust back, and saaio asks again before it runs the scripts:

```sh
saaio trust --revoke
```

Flows that already run in the repository carry on without its scripts. When you `saaio remove` or
`saaio clear` one of them, the worktree still goes, but the teardown does not run
(`teardown: not run, the repository is not trusted`). A resting flow's services are not paused,
and paused ones are not resumed: the resume fails with `the repository is not trusted`, and the
worker's tool calls stop waiting for it.

## Setup and teardown

A fresh worktree is only the code. Before a worker can run your tests, it usually needs an `.env`
file, its dependencies, a database and services on ports nobody else uses. `.saaio/setup` prepares
all of that, and `.saaio/teardown` cleans it up again:

| Script | Runs |
| --- | --- |
| `.saaio/setup` | after `saaio start` creates the worktree, before the worker starts |
| `.saaio/teardown` | before `saaio remove` or `saaio clear` removes the worktree. `saaio clear` runs it even on a worktree it then keeps for its uncommitted work. |

Both must be executable. They run from the main checkout with the worktree as working directory,
once the repository is [trusted](#trust). A small setup might look like this:

```sh
#!/bin/sh
# .saaio/setup
set -e
cp "$SAAIO_REPO/.env" .env
composer install
```

While the setup runs, `saaio start` waits for it, and its output is not shown. Since the scripts
run from the main checkout, a change to them applies at once. A change to `compose.yaml` or
`.gitignore`, on the other hand, reaches new worktrees only once it is committed, because
worktrees are made from the last commit.

### The scripts' environment

saaio tells the scripts which flow they run for through environment variables. The worker's
session gets the same variables, so your agent can use them too:

| Variable | Value |
| --- | --- |
| `SAAIO_SLOT` | the flow's slot, 1 to 20 |
| `SAAIO_WORKTREE` | the worktree's path |
| `SAAIO_REPO` | the main checkout's path |
| `SAAIO_FLOW_NAME` | the flow's name, such as `shop-217` |
| `SAAIO_FLOW_ID` | the flow's id |
| `SAAIO_BASE` | the branch the flow started from with `saaio start --base`. Unset without one. |

Teardown, pause and resume also get `GIT_DIR`, the worktree's entry in the main checkout's
`.git/worktrees/`, and `GIT_WORK_TREE`, the worktree. That way git in these scripts always acts on
the flow's worktree, whatever its `.git` file says. If a script needs git on the main checkout
instead, it clears them:

```sh
env -u GIT_DIR -u GIT_WORK_TREE git -C "$SAAIO_REPO" status
```

> [!NOTE]
> A worktree git no longer lists fails these scripts without running them.

### When setup or teardown fails

A failed setup does not stop the flow. The flow is blocked with the setup's last error line, and
the worker still opens, so you can look at what went wrong:

```
$ saaio start "SHOP-240 broken setup"
flow: shop-240
...
setup: "failed: composer: lock file out of date"
```

A setup that runs longer than 10 minutes is stopped with everything it started, and the flow is
blocked with `setup failed: no answer within 10m`.

A teardown never holds up a removal. When it fails, the removal goes on; when it runs longer than
2 minutes, it is stopped with everything it started, and the removal goes on as well.

## Slots and ports

Two workers can't both serve on port 8000. To keep them apart, every unfinished flow except a
plain session holds a slot, a number from 1 to 20 that is unique across all your projects. Your
scripts read it from `SAAIO_SLOT` and turn it into ports:

```sh
php artisan serve --port=$((8000 + SAAIO_SLOT))    # slot 3 serves on port 8003
```

`saaio start` claims the lowest free slot before the setup runs. An ended or removed flow releases
its slot; a done flow (its merge request open) keeps it until its session ends. When all 20 are in
use, `saaio start` tells you where to look:

```
error: all 20 slots are in use
help[2]: Run `saaio status --all` to see which flows hold them,Run `saaio clear` in projects with finished flows
```

## A Docker Compose example

Docker Compose is where slots matter most, since every worker wants its own stack. Start by letting
`compose.yaml` read its published ports from variables, defaulting to the ports you use today:

```yaml
services:
  app:
    ports:
      - "${HTTP_PORT:-8080}:80"
  db:
    ports:
      - "${DB_PORT:-3306}:3306"
```

`docker compose up` without saaio keeps using the default ports. The setup picks ports from the
slot, starts the stack and prepares the database:

```sh
#!/bin/sh
# .saaio/setup
set -e
export HTTP_PORT=$((18080 + SAAIO_SLOT)) DB_PORT=$((13306 + SAAIO_SLOT))
docker compose up -d --wait
docker compose exec -T app php artisan migrate --seed --force
```

The teardown removes the stack with its volumes:

```sh
#!/bin/sh
# .saaio/teardown
docker compose down -v --remove-orphans
```

The pause and resume scripts, which the [next section](#pause-and-resume) explains, stop the
containers and start the same ones again:

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

Compose names the project after its folder, so each worktree gets a stack of its own, and
`docker compose exec` in a worktree reaches that worktree's stack. Pause keeps the containers and
their data; resume starts the same ones and returns once they are healthy.

> [!WARNING]
> Remove fixed `container_name` entries from `compose.yaml`. They clash between worktrees.

> [!TIP]
> If two repositories have flows of the same name, set `COMPOSE_PROJECT_NAME` to the repository and
> flow name.

## Pause and resume

A Docker stack for every flow adds up quickly in CPU and memory, though most flows spend their
time waiting for you. `saaio server` therefore pauses the services of a flow that rests, and
starts them again before its worker needs them:

| Script | Runs |
| --- | --- |
| `.saaio/pause` | the flow rested (idle, waiting, blocked, done or ended) for `pause_after_minutes`, 5 by default ([configuration.md](configuration.md#services)) |
| `.saaio/resume` | the paused flow's worker works again: a new prompt, or its first tool call after the master woke it |

They run like teardown: from the main checkout, in the worktree, with the same variables and git
pinned to the worktree. Each runs on a thread of `saaio server`, for at most 5 minutes; past that
it is stopped with everything it started, and fails.

> [!IMPORTANT]
> A repository needs both scripts, and both executable. With only one, or neither, nothing is
> paused and nothing shows.

If the server stops during a script, the next server runs it again, so both scripts must be
idempotent. Plain sessions, and flows that are not responding, are never paused. `saaio remove`
and `saaio clear` run the teardown as always, on the paused services.

### Waiting for services

While services pause, are paused or resume, every tool call of the worker waits for them, up to 2
minutes. You will hardly notice: the model's thinking before its first tool call hides most of a
resume. If the worker works while a pause is still running, its tool calls wait for the pause,
then for the resume.

A resume that takes longer than 2 minutes lets the tool run, and the script goes on. When the
services are paused and no `saaio server` runs, a tool call waits 2 seconds, then runs.

### When pause or resume fails

When a pause fails, the flow's timeline says `pause failed: <last error line>`, and saaio tries
again once the flow's state changed.

When a resume fails, the tool runs anyway. The board card says `working · services down`, and
`saaio status` says `resume-failed` with the error. saaio tries again at the next turn.

## Branch names

By default, a flow's branch is called `flow/<name>`. If your team names branches its own way, a
committed `.saaio/config.toml` sets the convention:

```toml
[branch]
name  = "{type}/{ticket}-{slug}"
types = ["feature", "bugfix", "hotfix", "chore"]
```

With it, a flow's branch follows the request it was started with:

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

An existing branch name gets `-2`, `-3` and so on. The worktree folder keeps using the flow's
name.

### Placeholders

The `name` template may use these placeholders:

| Placeholder | Value |
| --- | --- |
| `{type}` | `--type` of `saaio start`, the type picked in the new-flow dialog, else the first of the kind's `branch_types` that `types` lists (`bugfix` for a fix), else the first in `types` |
| `{ticket}` | the ticket key at the start of the request, as written (`SHOP-230`) |
| `{slug}` | up to five words of the request after the ticket key, lowercased, joined by `-` |
| `{name}` | the flow's name |

An empty placeholder drops the separator next to it. Without a ticket key, for example,
`feature/{ticket}-{slug}` gives `feature/add-a-dark-mode-to`.

### Branch types

A type the repository does not list is refused, and the error names the ones it does:

```
$ saaio start --type nope "x"
error: "no branch type nope; this repo has feature, bugfix, hotfix, chore"
help[1]: "Name the types in .saaio/config.toml: [branch] types = [\"feature\", \"bugfix\"]"
```

## Heavy commands

A test suite, a formatter or a static analysis run takes most of a small machine's memory and
disk. When two workers of one project run them at once, both crawl. List those commands in
`.saaio/config.toml`, and saaio runs them one at a time across the project's workers:

```toml
[exclusive]
commands = ["artisan test", "sail test", "pest", "pint", "phpstan"]
```

Now, when a worker's shell call runs a listed command while another worker of the same repository
runs one, it waits until that run ends, then goes on. A worker's own second heavy call waits for
its first, too.

The repository says what is heavy, but each person decides whether their machine takes turns,
with `[exclusive] enabled` in their own config ([configuration.md](configuration.md#exclusive)).

### How a command matches

A command matches anywhere in the shell line, on word boundaries, so it is found however the
worker calls it:

```sh
php artisan test                                        # runs artisan test
./vendor/bin/sail pint                                  # runs pint
docker compose exec app php artisan test                # runs artisan test
docker exec shop-app vendor/bin/phpstan analyse         # runs phpstan
cat pint.json                                           # no match
```

A word is letters, digits, `_`, `.` and `-`, which is why `pint.json`, `pest-plugin` and
`artisan testing` don't match. A command of several words matches those words in a row:
`artisan test` is not in `sail test`, so list `sail test` on its own.

> [!NOTE]
> A line that only names a command waits too, such as `echo pint`: saaio reads the words of the
> line, not what they do.

The turn lasts while a process below the worker's agent runs a listed command, so a run in the
background keeps it until its process exits. A process the agent detaches itself (`cmd &` inside
a call, `nohup`) leaves the agent's processes and stops counting. What a container runs by itself
(watchers, queues) never counts; for idle stacks, see [Pause and resume](#pause-and-resume).

### Waiting for a turn

While a call waits, the sidebar says `waits turn` under the flow, and its third row names the run
it waits for:

```
after shop-42: pint
```

The board card says `working · waits turn`, and `saaio status` has `turn: waiting`. Once the run
ends, or is killed, the waiting call goes on within about a second. If two calls wait, either may
go first.

saaio makes sure a turn is never held by something that is gone. When the worker that has the
turn crashes or its session ends, the turn is free at once. When a call took the turn but no
listed process showed within 10 seconds (the tool was refused or failed to start), the turn is let
go; time spent on a permission prompt doesn't count. And a call that waited 30 minutes runs
anyway, which server.log notes.

> [!TIP]
> Turns fail open. When something goes wrong, such as a config that can't be read or a worker's
> agent saaio does not know, the call runs without waiting, and server.log says why.

Each harness has a few edges of its own:

- Esc in a Claude Code worker while its call waits cancels the call. `waits turn` stays until the
  worker's next prompt, since Claude Code says nothing on Esc.
- When one Claude Code turn runs two heavy calls in parallel, the second may wait about 10 seconds
  for the first, then both run at once.
- Codex's shell yields after about 30 seconds without output, so the model may call a held command
  slow, send it again (the copy waits too) or give up on it.
