# Configuration

flow works without any configuration. Everything you can change lives in one file, `config.toml`.

- [Where the file is](#where-the-file-is)
- [When a change applies](#when-a-change-applies)
- [\[worker\]](#worker)
- [\[master\]](#master)
- [\[ui\]](#ui)
- [\[\[phases\]\]: the workflow](#phases-the-workflow)
- [Agents](#agents)
- [The first prompt](#the-first-prompt)
- [Errors](#errors)

## Where the file is

flow reads the first of these that is set:

1. `$FLOW_CONFIG`, the file itself,
2. `$XDG_CONFIG_HOME/flow/config.toml`,
3. `~/.config/flow/config.toml`.

On its first run, flow ui writes this file with every setting commented out at its default, and a
line on what each does. The same file is in the repository as
[examples/config.toml](../examples/config.toml). Remove the `#` in front of a setting to change it.
flow never overwrites the file once you edited it.

Two more files live beside it: `theme.toml` (see [themes.md](themes.md)) and `master.md`, the
master's instructions (see [flows.md](flows.md#the-master)).

## When a change applies

Commands read the file each time they run, so `flow start` picks up a new `[worker] agent` or
workflow at once. flow ui and its server read it when they start: a change to `[agents]`, to
`[worker] prompt` or an agent's `prompt` (the server fills it in when it opens a worker), to
`[master]` or `[ui]`, and the new-flow dialog's default agent, apply after `flow ui --stop` and the
next `flow ui`.

## [worker]

What a new flow runs.

| Key | Default | Does |
| --- | --- | --- |
| `agent` | `"claude"` | the agent `flow start` and the new-flow dialog pick when you name none; a name under `[agents]` |
| `prompt` | unset | the first prompt of every agent that sets none of its own, the built-in ones too (see [The first prompt](#the-first-prompt)) |

```toml
[worker]
agent = "codex"
```

## [master]

How the master works. See [flows.md](flows.md#the-master) for what the master is.

| Key | Default | Does |
| --- | --- | --- |
| `wake` | `true` | wake the master when a flow of its project waits, is blocked, goes idle, finishes or stops responding. With `false` it learns only when it runs `flow status`; its row in flow ui still shows when it needs you. |
| `agent` | unset | the agent the master runs. Unset: the built-in `master` agent, Claude Code. |

```toml
[master]
agent = "codex"
```

## [ui]

How flow ui takes keys.

| Key | Default | Does |
| --- | --- | --- |
| `prefix` | `"C-space"` | the key you press before every flow ui command: `"C-space"`, or `"C-a"` to `"C-z"` |

A pane needs some keys, so these are refused: `C-c`, `C-d`, `C-z`, `C-s` and `C-q`, and `C-h`,
`C-i`, `C-j` and `C-m`, which send the same bytes as backspace, tab, newline and enter. flow ui's
hints and help name the prefix you set. Pressed twice, the prefix reaches the pane, so a prefix that
flow ui also uses as a key, such as `C-b` for paging back in the history, still works there.

```toml
[ui]
prefix = "C-b"
```

## [[phases]]: the workflow

A flow walks through up to four phases. flow ui shows them as the flow's steps
(`●───●───○───○`), with the time spent in each. A phase starts when the worker runs one of its
skills. The built-in workflow is the [agent-skills](https://github.com/JanSuthacheeva/agent-skills)
chain:

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

| Key | Does |
| --- | --- |
| `name` | the phase's name: lowercase letters, digits and `-` |
| `skills` | the skills that start the phase. A skill matches with or without its plugin's prefix (`superpowers:brainstorming` is `brainstorming`). A skill can start only one phase. |

Your own `[[phases]]` replace the built-in four as a whole: 1 to 4 phases, in order. For example,
with the skills of [superpowers](https://github.com/obra/superpowers):

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

A flow keeps the phases it started with. Changing the workflow changes new flows only, and a skill
of a phase a running flow does not have reports nothing. An opened merge request finishes a flow in
its last phase, whatever that phase is called.

## Agents

An agent is a command flow runs in a pane. Three are built in:

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

`claude` and `codex` run workers; `master` runs the master. An agent you add appears in
`flow start --agent` and in the new-flow dialog. An agent with a built-in name replaces that one
whole, so repeat every key you want to keep; only a missing `prompt` still falls back to the
built-in one.

| Key | Required | Does |
| --- | --- | --- |
| `command` | yes | the command line that starts a new session, one string per argument |
| `prompt` | no | the first prompt, filled into `{prompt}` |
| `resume` | no | the command line that continues the session after a restart. Without it, the agent starts once and is not resumed. |
| `adapter` | no | `"claude-code"` or `"codex"`: how flow learns what the agent does, through that harness's own hooks. Without one, flow ui shows only whether the process runs. |

Placeholders, filled into `command`, `prompt` and `resume`:

| Placeholder | Becomes |
| --- | --- |
| `{id}` | flow's id for the session |
| `{name}` | the flow's name |
| `{request}` | what you asked for |
| `{prompt}` | the agent's first prompt, with its own placeholders filled |
| `{session}` | the session id the harness picked, for a harness that picks its own (Codex) |
| `{instructions}` | the path of `master.md`, for a master that reads its instructions from a file |

An argument that fills to nothing is left out, so an agent without a prompt gets no empty one.

An agent of your own, for example [aider](https://aider.chat):

```toml
[agents.aider]
command = ["aider", "--message", "{prompt}"]
prompt = "{request}"
```

flow cannot see inside aider, so its row shows only whether it runs. A custom agent that runs Claude
Code or Codex should set `adapter`, so flow follows it like the built-in ones.

flow adds its own wiring to an agent with an adapter: its hooks, and for workers a short system
prompt that says who the flow master is. It installs nothing and writes nothing to `~/.claude` or
`~/.codex`. See [flows.md](flows.md#how-a-worker-reports).

## The first prompt

A worker's first prompt starts your workflow. flow picks it in this order:

1. the agent's own `prompt`,
2. `[worker] prompt`, exactly as written, for every agent,
3. the built-in agent's prompt of the same name (`/ship-feature {request}` for `claude`,
   `$ship-feature {request}` for `codex`),
4. none.

Claude Code calls a skill `/name` and Codex calls it `$name`. A `[worker] prompt` shared by both
should therefore name no skill, or give each agent its own `prompt`.

A plain session never gets a first prompt.

## Errors

A broken `config.toml` stops `flow start`, `flow new` and `flow ui` with an error that names the
file, the line and the key:

```
error: "/Users/jan/.config/flow/config.toml is not valid: TOML parse error at line 2, column 1\n  |\n2 | agnet = \"x\"\n  | ^^^^^\nunknown field `agnet`, expected `agent` or `prompt`"
```

Workers that already run keep reporting with the built-in settings until the file is fixed.

Settings from earlier versions are refused with a message that names the new form: a top-level
`agent` key (now `agent` under `[worker]`), and a `[phases]` table of skill names (now
`[[phases]]`).
