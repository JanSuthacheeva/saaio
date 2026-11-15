# Configuration

- [Introduction](#introduction)
- [File location](#file-location)
- [Use your own workflow](#use-your-own-workflow)
    - [Without matching skills](#without-matching-skills)
- [\[worker\]](#worker)
- [\[master\]](#master)
- [\[ui\]](#ui)
- [\[services\]](#services)
- [\[exclusive\]](#exclusive)
- [Kinds of flows](#kinds-of-flows)
- [Agents](#agents)
    - [Adding an agent](#adding-an-agent)
- [The first prompt](#the-first-prompt)
- [When a change applies](#when-a-change-applies)
- [Errors](#errors)

## Introduction

saaio works without any configuration: every key has a default, and the file is optional. When
you want saaio to run your own skills, another agent or a different prefix key, you set it in
`~/.config/saaio/config.toml`.

A setting is one line in its table. To make Codex the agent new flows run, for example:

```toml
[worker]
agent = "codex"
```

Settings that belong to a repository, such as its branch names and heavy commands, live in the
repository's own `.saaio/config.toml` instead ([repository.md](repository.md)).

## File location

saaio reads the first of these paths that is set:

1. `$SAAIO_CONFIG` (the file itself)
2. `$XDG_CONFIG_HOME/saaio/config.toml`
3. `~/.config/saaio/config.toml`

You don't have to start from an empty file. The UI writes it on its first run, with every setting
commented out at its default and one line per setting on what it does:

```toml
#[worker]
# The agent a new flow runs when you don't pick one, by its name under [agents].
#agent = "claude"
```

To set one, remove the `#` in front of it and its table, and change its value.

The same content is in [examples/config.toml](../examples/config.toml). An edited file is never
overwritten; one still as an earlier saaio wrote it gets the current example.

Next to it live `theme.toml` ([themes.md](themes.md)) and `master.md`, the master's instructions
([flows.md](flows.md#the-master)).

## Use your own workflow

saaio is built around the [agent-skills](https://github.com/JanSuthacheeva/agent-skills)
workflow, but it does not depend on it. A flow's kind holds its workflow: the skill its first
prompt calls and the phases its steps show. To replace the built-in feature kind's `/ship-feature`
and its four phases with your own skills:

```toml
[kinds.feature]
skill = "my-feature"

[[kinds.feature.phases]]
name = "plan"
skills = ["my-plan"]

[[kinds.feature.phases]]
name = "build"
skills = ["my-build"]

[[kinds.feature.phases]]
name = "ship"
skills = ["my-pr"]
```

`skill` is the skill the first prompt calls: `saaio start "SHOP-217 add CSV export"` now sends
`/my-feature SHOP-217 add CSV export` in Claude Code, and `$my-feature ...` in Codex. `phases` are
the flow's steps, 1 to 4 of them, and a skill the worker runs moves the flow to the phase that
lists it. The prompt starts the work; the phases only track it.

A kind with a built-in name replaces the built-in one whole, so name its `phases`, and its
`branch_types` if you want them. The change applies at the next `saaio start`; a running flow
keeps the kind it started with.

> [!NOTE]
> Files from before kinds keep working: a top-level `[[phases]]` is the feature kind's phases, and
> `[worker] prompt` its prompt.

### Without matching skills

If your agent has no skills that match the phases, saaio still follows the flow. The states
(`working`, `waiting` with its question, `idle`, `done`) come from the harness hooks, and an opened
merge request still finishes the flow. Only the steps stay `○───○───○───○`, and the sidebar shows
the state where the phase label would be: `starting` while working, `idle`, `waiting`.

A first prompt that names a skill you have not installed is harmless, too. Claude Code passes the
line to the model as a plain message, so the worker gets the request and works without the skill.

## [worker]

The `[worker]` table picks what a new flow runs when you don't pick it yourself:

```toml
[worker]
agent = "codex"
```

| Key | Default | Effect |
| --- | --- | --- |
| `agent` | `"claude"` | agent used by `saaio start` and the new-flow dialog when none is picked. A name under `[agents]`. |
| `kind` | `"feature"` | kind of flow used by `saaio start`, `saaio new` and the new-flow dialog when none is picked. A name under `[kinds]`. |
| `prompt` | unset | from earlier versions: the feature kind's `prompt`, unless `[kinds.feature]` sets one |

## [master]

The `[master]` table sets how the master runs ([flows.md](flows.md#the-master)):

```toml
[master]
agent = "codex"
```

| Key | Default | Effect |
| --- | --- | --- |
| `wake` | `true` | wake the master when a flow of its project waits, is blocked, goes idle, finishes or stops responding. `false`: the master learns only by running `saaio status`. Its row in the UI still shows when it needs you. |
| `agent` | unset | agent the master runs. Unset: the built-in `master` agent (Claude Code). |

## [ui]

The `[ui]` table sets the terminal UI. If `C-space` is taken in your terminal, you might pick
another prefix:

```toml
[ui]
prefix = "C-b"
```

| Key | Default | Effect |
| --- | --- | --- |
| `prefix` | `"C-space"` | key pressed before every UI command: `"C-space"` or `"C-a"` to `"C-z"` |
| `update_check` | `true` | the UI looks for a newer release once a day and names it in its title row: `0.1.1 is out: saaio update` |
| `notify` | `true` | a flow that starts waiting or is blocked while you aren't looking at it raises a desktop notification through your terminal ([ui.md](ui.md#notifications)) |

The UI's hints and help show the configured prefix. Pressed twice, the prefix is sent to the pane,
and a prefix that is also a UI key (`C-b`, page back in history) still works there. A few keys are
refused, because the pane or the terminal needs them:

| Refused prefix | Reason |
| --- | --- |
| `C-c`, `C-d`, `C-z`, `C-s`, `C-q` | used by the pane |
| `C-h`, `C-i`, `C-j`, `C-m` | same bytes as backspace, tab, newline, enter |

> [!NOTE]
> The update check is the one network call saaio makes on its own (the server also runs `gh` and
> `glab` for merge requests). It reads one small file, the latest release's `VERSION`, through
> `curl` with a 5 second limit, at most once a day for every UI on the machine (`update.json`,
> [files.md](files.md#the-shared-folder)). Offline or without `curl` it shows nothing. The request
> carries nothing about you or your projects.

## [services]

The `[services]` table sets when a resting flow's services pause. On a machine with memory to
spare, you might give them longer:

```toml
[services]
pause_after_minutes = 15
```

| Key | Default | Effect |
| --- | --- | --- |
| `pause_after_minutes` | `5` | minutes a flow rests (idle, waiting, blocked, done or ended) before `saaio server` runs the repo's `.saaio/pause`. `.saaio/resume` runs when its worker works again. `0` never pauses. |

Only repos with both scripts are paused: [repository.md](repository.md#pause-and-resume).

## [exclusive]

A repository lists its heavy commands, and they run one at a time
([repository.md](repository.md#heavy-commands)). Whether your machine takes those turns is up to
you. On a big machine, you might let them run at once:

```toml
[exclusive]
enabled = false
```

| Key | Default | Effect |
| --- | --- | --- |
| `enabled` | `true` | a worker's shell call that runs a command its repository lists under `[exclusive]` waits while another worker of the project runs one. `false`: they run at once. |

## Kinds of flows

A kind is what a flow walks through: its phases, its first prompt and its branch. `saaio start
--kind`, `saaio new --kind` and the new-flow dialog pick one, and `[worker] kind` is the default.
saaio has three built in:

| Kind | First prompt (Claude Code) | Phases | Branch |
| --- | --- | --- | --- |
| `feature` | `/ship-feature <request>` | plan, implement, review, merge-request | type `feature` or `feat` |
| `fix` | `/ship-fix <request>` | diagnose, implement, review, merge-request | type `fix`, `bugfix` or `hotfix` |
| `research` | `Research this and answer here. Change no code and commit nothing: <request>` | research | none |

Written out in config.toml, with each key at its default, they look like this:

```toml
[kinds.feature]
skill = "ship-feature"
branch_types = ["feature", "feat"]
branch = true

[[kinds.feature.phases]]
name = "plan"
skills = ["design-code"]

[[kinds.feature.phases]]
name = "implement"
skills = ["implement-plan"]

[[kinds.feature.phases]]
name = "review"
skills = ["review-changes"]

[[kinds.feature.phases]]
name = "merge-request"
skills = ["write-merge-request"]

[kinds.fix]
skill = "ship-fix"
branch_types = ["fix", "bugfix", "hotfix"]
branch = true

[[kinds.fix.phases]]
name = "diagnose"
skills = ["diagnose-bug"]

# implement, review and merge-request as in feature

[kinds.research]
prompt = "Research this and answer here. Change no code and commit nothing: {request}"
branch_types = []
branch = false

[[kinds.research.phases]]
name = "research"
skills = []
```

| Key | Required | Effect |
| --- | --- | --- |
| `phases` | yes | 1 to 4 phases in order, each a `name` (lowercase letters, digits, `-`) and the `skills` that start it |
| `skill` | no | the skill the first prompt calls, filled into `{skill}`: `/name` in Claude Code, `$name` in Codex, the bare name for an agent without an adapter |
| `prompt` | no | the first prompt. Unset: `{skill} {request}`, or `{request}` without a skill. An agent's own `prompt` wins over it ([The first prompt](#the-first-prompt)). |
| `branch_types` | no | branch types the kind means. The first one the repo's `.saaio/config.toml` lists wins, else the repo's first ([repository.md](repository.md#branch-names)). `--type` picks another. |
| `branch` | no, `true` | `false`: no branch and nothing to merge. The worktree is detached at the base, the worker answers in its pane, and the flow is done once a turn ends without a question ([flows.md](flows.md#research-flows)). |

A kind with a built-in name replaces the built-in one whole. To add your own, give it a new name,
lowercase letters, digits and `-` like a phase's, and it shows in `saaio start --kind` and the
new-flow dialog. Here is a feature kind for the
[superpowers](https://github.com/obra/superpowers) skills:

```toml
[kinds.feature]
skill = "brainstorming"

[[kinds.feature.phases]]
name = "spec"
skills = ["brainstorming", "writing-plans"]

[[kinds.feature.phases]]
name = "build"
skills = ["executing-plans", "subagent-driven-development"]

[[kinds.feature.phases]]
name = "ship"
skills = ["finishing-a-development-branch"]
```

The UI shows the phases as steps (`●───●───○───○`) with the time spent in each. A phase starts when
the worker runs one of its skills. A phase without skills, first in its list, starts with the
flow's first turn, as research does. Skills match with or without the plugin prefix
(`superpowers:brainstorming` matches `brainstorming`), and a skill starts at most one phase of a
kind. An opened merge request finishes the flow in its last phase, whatever its name.

`saaio start` records the kind's phases and first prompt in the flow, so a change affects new flows
only. A skill of a phase the running flow does not have reports nothing.

> [!NOTE]
> Codex has no skill event. A Codex worker's phase starts when a prompt names `$skill`, or when it
> reads that skill's `SKILL.md`. saaio tells Codex to read each `SKILL.md` only when it begins that
> skill. One command that reads the skills of several phases starts none.

> [!WARNING]
> A top-level `[[phases]]` from earlier versions is the feature kind's phases. Beside
> `[[kinds.feature.phases]]` it is an error: set them in one place.

## Agents

An agent is a command saaio runs in a pane. For harness-specific setup and recommended worker
permissions, see [Agent permissions](agents.md). saaio has three built in: `claude` and `codex` run
workers, and `master` runs the master:

```toml
[agents.claude]
command = ["claude", "-n", "{name}", "--session-id", "{id}", "{prompt}"]
resume = ["claude", "-n", "{name}", "--resume", "{id}"]
adapter = "claude-code"

[agents.codex]
command = ["codex", "{prompt}"]
resume = ["codex", "resume", "{session}"]
adapter = "codex"

[agents.master]
command = ["claude", "-n", "{name}", "--session-id", "{id}"]
prompt = ""
resume = ["claude", "-n", "{name}", "--resume", "{id}"]
adapter = "claude-code"
```

`claude` and `codex` have no `prompt`: they get the flow's kind's
([The first prompt](#the-first-prompt)). An agent with a built-in name replaces the built-in one
whole, so repeat every key you want to keep.

| Key | Required | Effect |
| --- | --- | --- |
| `command` | yes | command line that starts a new session, one string per argument |
| `prompt` | no | first prompt for every kind, filled into `{prompt}`. Unset: the flow's kind's prompt. |
| `resume` | no | command line that continues the session after a restart. Without it: started once, not resumed. |
| `adapter` | no | `"claude-code"` or `"codex"`: the harness whose hooks report the agent's state. Without it: the UI shows only whether the process runs. |

The command lines may use these placeholders:

| Placeholder | Value |
| --- | --- |
| `{id}` | the flow's id for the session |
| `{name}` | the flow's name |
| `{request}` | the request |
| `{prompt}` | the first prompt, its placeholders filled |
| `{skill}` | the kind's skill as the agent's harness calls it. In a kind's `prompt` only. |
| `{session}` | the session id the harness picked, for a harness that picks its own (Codex). In `resume` only. |
| `{instructions}` | path of `master.md`, for a master that reads its instructions from a file. For the master only. |

`{id}` and `{name}` are filled in `command`, `prompt` and `resume`, `{request}` in `command` and
`prompt`, `{prompt}` in `command`, and `{skill}` in a kind's `prompt`. An argument that fills to an
empty string is dropped.

### Adding an agent

Any program that runs in a terminal can be an agent. Here is [aider](https://aider.chat):

```toml
[agents.aider]
command = ["aider", "--message", "{prompt}"]
prompt = "{request}"
```

An added agent appears in `saaio start --agent` and the new-flow dialog. Without an `adapter`, its
row shows only whether it runs. A custom agent that runs Claude Code or Codex sets `adapter`, so
saaio follows it like the built-in ones.

With an adapter, saaio adds to the command line: its hooks, for workers a system prompt naming the
saaio master, for the master `master.md`, and for Codex trust of the worktree in a
[trusted](repository.md#trust) repository. Nothing is installed, and nothing is written to
`~/.claude` or `~/.codex` ([flows.md](flows.md#how-a-worker-reports)).

## The first prompt

A worker's first prompt is the first of these that is set:

1. the agent's own `prompt`, for every kind
2. the kind's `prompt`
3. `{skill} {request}` with the kind's `skill`: `/ship-fix {request}` in Claude Code,
   `$ship-fix {request}` in Codex
4. `{request}`

saaio writes the skill the way the agent's harness calls one, so one kind serves Claude Code and
Codex alike. `saaio start` records the prompt in the flow, and the flow keeps it when the config
changes.

A plain session gets no first prompt. The master never gets one either: its agent's `prompt` is
ignored, and a new Codex master is told to run `saaio status`.

## When a change applies

You never need to restart anything. Commands read the file every time they run, and the UI and its
server read it again within a second of a save. When the new setting takes effect depends on the
setting:

| Setting | Applies |
| --- | --- |
| `[worker] agent` | at once: the next `saaio start`, the new-flow dialog's preselected agent |
| `[worker] kind`, `[kinds]` | at the next `saaio start`, and the next time the new-flow dialog opens. A running flow keeps its kind: its phases and its first prompt. |
| an agent's `prompt` | to the next flow that starts |
| `[agents]` | to the next session opened or resumed with that agent. A running session keeps its command and adapter. |
| `[master] wake` | at once |
| `[master] agent` | the next time the master starts |
| `[ui] prefix` | at once. The hints show the new prefix. |
| `[ui] update_check` | off: at once, the line goes. On: the line shows at once, and the next look follows within the hour once a day has passed. |
| `[ui] notify` | at once, from the next change of a flow |
| `[services] pause_after_minutes` | at once, for every resting flow |
| `[exclusive] enabled` | at the next tool call |
| an added or removed agent in the new-flow dialog | the next time the dialog opens |

## Errors

Every error names the file, the line and the reason. `saaio start` prints:

```
error: "/Users/jan/.config/saaio/config.toml:2: unknown field `agnet`, expected one of `agent`, `kind`, `prompt`"
```

A running UI shows the same in its title row, as
``config: ~/.config/saaio/config.toml:2: unknown field `agnet`, ...``.

A broken file never takes down what already runs. The UI and its server keep the last working
settings; the title row shows the error until the file reads again, and the server writes it to
`server.log`. Running workers keep reporting with the built-in settings until the file is fixed.
Only `saaio start`, `saaio new`, `saaio send --type` and `saaio` opening the UI stop with the error.

Settings from earlier versions are refused with a message naming the new form:

| Earlier | Now |
| --- | --- |
| top-level `agent` | `agent` under `[worker]` |
| `[phases]` table of skill names | `[[kinds.feature.phases]]` |

Still read: a top-level `[[phases]]`, as `[[kinds.feature.phases]]`, and `[worker] prompt`, as
`[kinds.feature] prompt`.
