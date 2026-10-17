# Configuration

File: `~/.config/flow/config.toml`. Optional; every key has a default.

- [Use your own workflow](#use-your-own-workflow)
- [File location](#file-location)
- [When a change applies](#when-a-change-applies)
- [\[worker\]](#worker)
- [\[master\]](#master)
- [\[ui\]](#ui)
- [\[\[phases\]\]: the workflow](#phases-the-workflow)
- [Agents](#agents)
- [The first prompt](#the-first-prompt)
- [Errors](#errors)

## Use your own workflow

Replaces the default `/ship-feature` prompt and the four
[agent-skills](https://github.com/JanSuthacheeva/agent-skills) phases:

```toml
[worker]
prompt = "/my-feature {request}"   # $my-feature in Codex

[[phases]]
name = "plan"
skills = ["my-plan"]

[[phases]]
name = "build"
skills = ["my-build"]

[[phases]]
name = "ship"
skills = ["my-pr"]
```

| Setting | Effect |
| --- | --- |
| `[worker] prompt` | first prompt of every worker. `{request}` is the request: `flow start "SHOP-217 add CSV export"` sends `/my-feature SHOP-217 add CSV export`. |
| `[[phases]]` | the flow's steps. A skill the worker runs moves the flow to the phase that lists it. 1 to 4 phases; replaces the built-in four. |

- The prompt starts the work; the phases only track it. Either can be set without the other.
- Claude Code calls a skill `/name`, Codex `$name`. A `[worker] prompt` used by both names no
  skill, or each agent sets its own `prompt` ([The first prompt](#the-first-prompt)).
- `[[phases]]` applies at the next `flow start`. `[worker] prompt` applies after
  `flow ui --stop && flow ui`.
- A running flow keeps the phases it started with.

### Without matching skills

| | When no skill matches a phase |
| --- | --- |
| States (`working`, `waiting` with its question, `idle`, `done`) | shown; they come from the harness hooks |
| Opened merge request | finishes the flow |
| Steps | stay `○───○───○───○` |
| Phase label in the sidebar | replaced by the state: `starting` while working, `idle`, `waiting` |

A first prompt that names an uninstalled skill: Claude Code passes the line to the model as a plain
message. The worker gets the request and works without the skill.

## File location

| Order | Path |
| --- | --- |
| 1 | `$FLOW_CONFIG` (the file itself) |
| 2 | `$XDG_CONFIG_HOME/flow/config.toml` |
| 3 | `~/.config/flow/config.toml` |

The first set path wins.

- flow ui writes the file on its first run: every setting commented out at its default, one line
  per setting on what it does. Same content: [examples/config.toml](../examples/config.toml).
- Remove the `#` in front of a setting to set it.
- An edited file is never overwritten.
- Next to it: `theme.toml` ([themes.md](themes.md)), `master.md`, the master's instructions
  ([flows.md](flows.md#the-master)).

## When a change applies

| Setting | Applies |
| --- | --- |
| `[worker] agent` | at the next `flow start` |
| `[[phases]]` | at the next `flow start` |
| `[worker] prompt`, an agent's `prompt` | after `flow ui --stop && flow ui` (the server fills prompts in when it opens a worker) |
| `[agents]` | after `flow ui --stop && flow ui` |
| `[master]` | after `flow ui --stop && flow ui` |
| `[ui]` | after `flow ui --stop && flow ui` |
| the new-flow dialog's preselected agent | after `flow ui --stop && flow ui` |

## [worker]

```toml
[worker]
agent = "codex"
```

| Key | Default | Effect |
| --- | --- | --- |
| `agent` | `"claude"` | agent used by `flow start` and the new-flow dialog when none is picked; a name under `[agents]` |
| `prompt` | unset | first prompt of every agent without its own `prompt`, built-in agents included ([The first prompt](#the-first-prompt)) |

## [master]

```toml
[master]
agent = "codex"
```

| Key | Default | Effect |
| --- | --- | --- |
| `wake` | `true` | wake the master when a flow of its project waits, is blocked, goes idle, finishes or stops responding. `false`: the master learns only by running `flow status`; its row in flow ui still shows when it needs you. |
| `agent` | unset | agent the master runs. Unset: the built-in `master` agent (Claude Code). |

What the master is: [flows.md](flows.md#the-master).

## [ui]

```toml
[ui]
prefix = "C-b"
```

| Key | Default | Effect |
| --- | --- | --- |
| `prefix` | `"C-space"` | key pressed before every flow ui command: `"C-space"` or `"C-a"` to `"C-z"` |

| Refused prefix | Reason |
| --- | --- |
| `C-c`, `C-d`, `C-z`, `C-s`, `C-q` | used by the pane |
| `C-h`, `C-i`, `C-j`, `C-m` | same bytes as backspace, tab, newline, enter |

- flow ui's hints and help show the configured prefix.
- The prefix pressed twice is sent to the pane. A prefix that is also a flow ui key (`C-b`, page back
  in history) still works there.

## [[phases]]: the workflow

Default:

```toml
[[phases]]
name = "plan"
skills = ["design-code"]

[[phases]]
name = "implement"
skills = ["implement-plan"]

[[phases]]
name = "review"
skills = ["review-changes"]

[[phases]]
name = "merge-request"
skills = ["write-merge-request"]
```

| Key | Effect |
| --- | --- |
| `name` | phase name: lowercase letters, digits, `-` |
| `skills` | skills that start the phase. Matched with or without the plugin prefix (`superpowers:brainstorming` matches `brainstorming`). A skill starts at most one phase. |

- 1 to 4 phases, in order. A custom list replaces all four.
- flow ui shows the phases as steps (`●───●───○───○`) with the time spent in each.
- A phase starts when the worker runs one of its skills.
- A flow keeps the phases it started with; a change affects new flows only.
- A skill of a phase the running flow does not have reports nothing.
- An opened merge request finishes the flow in its last phase, whatever its name.

Example with [superpowers](https://github.com/obra/superpowers):

```toml
[worker]
prompt = "/brainstorming {request}"

[[phases]]
name = "spec"
skills = ["brainstorming", "writing-plans"]

[[phases]]
name = "build"
skills = ["executing-plans", "subagent-driven-development"]

[[phases]]
name = "ship"
skills = ["finishing-a-development-branch"]
```

## Agents

An agent is a command flow runs in a pane. Built in:

```toml
[agents.claude]
command = ["claude", "-n", "{name}", "--session-id", "{id}", "{prompt}"]
prompt = "/ship-feature {request}"
resume = ["claude", "-n", "{name}", "--resume", "{id}"]
adapter = "claude-code"

[agents.codex]
command = ["codex", "{prompt}"]
prompt = "$ship-feature {request}"
resume = ["codex", "resume", "{session}"]
adapter = "codex"

[agents.master]
command = ["claude", "-n", "{name}", "--session-id", "{id}"]
prompt = ""
resume = ["claude", "-n", "{name}", "--resume", "{id}"]
adapter = "claude-code"
```

- `claude` and `codex` run workers; `master` runs the master.
- An added agent appears in `flow start --agent` and the new-flow dialog.
- An agent with a built-in name replaces the built-in one whole. Repeat every key to keep. Only a
  missing `prompt` falls back to the built-in one.

| Key | Required | Effect |
| --- | --- | --- |
| `command` | yes | command line that starts a new session, one string per argument |
| `prompt` | no | first prompt, filled into `{prompt}` |
| `resume` | no | command line that continues the session after a restart. Without it: started once, not resumed. |
| `adapter` | no | `"claude-code"` or `"codex"`: the harness whose hooks report the agent's state. Without it: flow ui shows only whether the process runs. |

| Placeholder | Value |
| --- | --- |
| `{id}` | flow's id for the session |
| `{name}` | the flow's name |
| `{request}` | the request |
| `{prompt}` | the agent's first prompt, its placeholders filled |
| `{session}` | the session id the harness picked, for a harness that picks its own (Codex) |
| `{instructions}` | path of `master.md`, for a master that reads its instructions from a file |

Filled into `command`, `prompt` and `resume`. An argument that fills to an empty string is dropped.

Custom agent, [aider](https://aider.chat):

```toml
[agents.aider]
command = ["aider", "--message", "{prompt}"]
prompt = "{request}"
```

- No `adapter`: its row shows only whether it runs.
- A custom agent that runs Claude Code or Codex sets `adapter` to be followed like the built-in ones.
- With an adapter, flow adds to the command line: its hooks, and for workers a system prompt naming
  the flow master. Nothing is installed; nothing is written to `~/.claude` or `~/.codex`
  ([flows.md](flows.md#how-a-worker-reports)).

## The first prompt

A worker's first prompt is the first of:

| Order | Source |
| --- | --- |
| 1 | the agent's own `prompt` |
| 2 | `[worker] prompt`, as written, for every agent |
| 3 | the built-in agent of the same name: `/ship-feature {request}` (`claude`), `$ship-feature {request}` (`codex`) |
| 4 | none |

- Claude Code calls a skill `/name`, Codex `$name`. A `[worker] prompt` shared by both names no
  skill, or each agent sets its own `prompt`.
- A plain session gets no first prompt.

## Errors

A broken `config.toml` stops `flow start`, `flow new` and `flow ui` with the file, line and key:

```
error: "/Users/jan/.config/flow/config.toml is not valid: TOML parse error at line 2, column 1\n  |\n2 | agnet = \"x\"\n  | ^^^^^\nunknown field `agnet`, expected `agent` or `prompt`"
```

Running workers keep reporting with the built-in settings until the file is fixed.

Settings from earlier versions are refused with a message naming the new form:

| Earlier | Now |
| --- | --- |
| top-level `agent` | `agent` under `[worker]` |
| `[phases]` table of skill names | `[[phases]]` |
