# Configuration

flow runs fine without any configuration. When you want to change something, it goes in one file,
`~/.config/flow/config.toml`.

- [Use your own workflow](#use-your-own-workflow)
- [Where the file is](#where-the-file-is)
- [When a change applies](#when-a-change-applies)
- [\[worker\]](#worker)
- [\[master\]](#master)
- [\[ui\]](#ui)
- [\[\[phases\]\]: the workflow](#phases-the-workflow)
- [Agents](#agents)
- [The first prompt](#the-first-prompt)
- [Errors](#errors)

## Use your own workflow

Out of the box, every worker starts with `/ship-feature <request>` and walks through four phases
named after the [agent-skills](https://github.com/JanSuthacheeva/agent-skills) set: plan,
implement, review and merge-request. If you have skills of your own, you don't need that set.
Two settings swap it out:

```toml
[worker]
prompt = "/my-feature {request}"

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

`[worker] prompt` is what every new worker is told first. `{request}` becomes whatever you asked
for, so `flow start "SHOP-217 add CSV export"` opens a worker with
`/my-feature SHOP-217 add CSV export`.

`[[phases]]` is how flow knows how far a worker got. Each time the worker runs a skill, flow checks
which phase lists it and moves the flow there. The sidebar draws the phases as steps
(`●───●───○`), so this example gives every new flow three. You can have one to four phases, and
yours replace the built-in four entirely.

The two settings are independent. The prompt starts the work; the phases only watch it. Your
`my-feature` skill might call `my-plan`, `my-build` and `my-pr` in turn, or the worker might pick
them up as it goes. Either way, a phase starts when one of its skills runs.

A few things to know:

- Claude Code calls a skill `/my-feature` and Codex calls it `$my-feature`. If you use both, either
  write a `[worker] prompt` that names no skill, or give each agent its own `prompt` (see
  [The first prompt](#the-first-prompt)).
- `flow start` reads `[[phases]]` each time, so the next flow uses the new phases right away. A
  change to `[worker] prompt` needs `flow ui --stop` and `flow ui` first.
- A running flow keeps the phases it started with.

### Without matching skills

A flow doesn't need any of this to work. If none of the worker's skills match a phase, the flow
still shows when the worker is working, when it's waiting on you (with its question), when its turn
ended, and when it opened a merge request, which finishes the flow as usual. That all comes from
the agent's hooks, not from skills.

What you lose is the progress. The steps stay `○───○───○───○`, and where the sidebar would name
the phase, it names the state instead, such as `starting` while the worker works or `idle` once
its turn ends.

The same goes when the first prompt names a skill you haven't installed. Claude Code then hands
the line to the agent as an ordinary message, so the worker still gets your request and works on
it, just without the skill.

## Where the file is

flow reads the first of these that is set:

1. `$FLOW_CONFIG`, the file itself
2. `$XDG_CONFIG_HOME/flow/config.toml`
3. `~/.config/flow/config.toml`

The first time flow ui runs, it writes this file for you, with every setting commented out at its
default and a line on what each does. The same file is in the repository as
[examples/config.toml](../examples/config.toml). To change a setting, remove the `#` in front of
it. Once you've edited the file, flow never overwrites it.

Two more files live next to it: `theme.toml` for colors (see [themes.md](themes.md)) and
`master.md`, the master's instructions (see [flows.md](flows.md#the-master)).

## When a change applies

Most changes need a restart of flow ui:

```sh
flow ui --stop && flow ui
```

Two settings don't. `flow start` reads `[worker] agent` and `[[phases]]` every time it runs, so the
next flow you start already uses them. The new-flow dialog's preselected agent still waits for the
restart, and so does every prompt, since the server fills a prompt in when it opens the worker.

## [worker]

What a new flow runs.

| Key | Default | Does |
| --- | --- | --- |
| `agent` | `"claude"` | the agent `flow start` and the new-flow dialog use when you don't pick one; a name under `[agents]` |
| `prompt` | unset | the first prompt for every agent that doesn't set its own, built-in agents included (see [The first prompt](#the-first-prompt)) |

```toml
[worker]
agent = "codex"
```

## [master]

How the master behaves. [flows.md](flows.md#the-master) explains what the master is.

| Key | Default | Does |
| --- | --- | --- |
| `wake` | `true` | wakes the master when one of its project's flows waits, gets blocked, goes idle, finishes or stops responding. With `false`, it only finds out when it runs `flow status`, though its row in flow ui still shows when it needs you. |
| `agent` | unset | the agent the master runs. Unset, it's the built-in `master` agent, which is Claude Code. |

```toml
[master]
agent = "codex"
```

## [ui]

| Key | Default | Does |
| --- | --- | --- |
| `prefix` | `"C-space"` | the key you press before every flow ui command: `"C-space"`, or anything from `"C-a"` to `"C-z"` |

Some keys belong to the pane, so flow refuses them as a prefix: `C-c`, `C-d`, `C-z`, `C-s` and
`C-q`, plus `C-h`, `C-i`, `C-j` and `C-m`, which send the same bytes as backspace, tab, newline and
enter.

```toml
[ui]
prefix = "C-b"
```

flow ui's hints and help show whichever prefix you set. Pressing the prefix twice sends it to the
pane, so a prefix like `C-b`, which flow ui also uses to page back through history, still works
there.

## [[phases]]: the workflow

A flow walks through up to four phases. flow ui shows them as the flow's steps
(`●───●───○───○`), with the time spent in each, and a phase starts when the worker runs one of its
skills. The built-in workflow follows the
[agent-skills](https://github.com/JanSuthacheeva/agent-skills) chain:

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
| `name` | the phase's name, in lowercase letters, digits and `-` |
| `skills` | the skills that start the phase. A skill matches with or without its plugin's prefix, so `superpowers:brainstorming` matches `brainstorming`. Each skill can start only one phase. |

Your own `[[phases]]` replace the built-in four as a whole, with one to four phases in order. Here
is a workflow built on the skills of [superpowers](https://github.com/obra/superpowers):

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

A flow keeps the phases it started with, so a new workflow only affects new flows. If a running
flow's worker runs a skill from a phase that flow doesn't have, nothing is reported. Whatever your
last phase is called, an opened merge request finishes the flow there.

## Agents

An agent is a command flow runs in a pane. Three come built in:

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

`claude` and `codex` run workers, and `master` runs the master. Any agent you add shows up in
`flow start --agent` and in the new-flow dialog. If you define an agent with a built-in name, yours
replaces the built-in one completely, so repeat every key you want to keep. The one exception is
`prompt`: leave it out and the built-in prompt still applies.

| Key | Required | Does |
| --- | --- | --- |
| `command` | yes | the command line that starts a new session, one string per argument |
| `prompt` | no | the first prompt, filled into `{prompt}` |
| `resume` | no | the command line that continues the session after a restart. Without it, the agent starts once and isn't resumed. |
| `adapter` | no | `"claude-code"` or `"codex"`, the harness whose hooks tell flow what the agent is doing. Without one, flow ui only shows whether the process is running. |

These placeholders are filled into `command`, `prompt` and `resume`:

| Placeholder | Becomes |
| --- | --- |
| `{id}` | flow's id for the session |
| `{name}` | the flow's name |
| `{request}` | what you asked for |
| `{prompt}` | the agent's first prompt, with its own placeholders filled |
| `{session}` | the session id the harness picked, for a harness that picks its own (Codex) |
| `{instructions}` | the path of `master.md`, for a master that reads its instructions from a file |

An argument that ends up empty is dropped, so an agent without a prompt doesn't get an empty one.

Here's an agent of your own, [aider](https://aider.chat):

```toml
[agents.aider]
command = ["aider", "--message", "{prompt}"]
prompt = "{request}"
```

flow can't see inside aider, so its row only shows whether it's running. If your custom agent runs
Claude Code or Codex underneath, set `adapter` and flow follows it like the built-in ones.

For an agent with an adapter, flow adds its own wiring to the command line: its hooks, and for
workers a short system prompt saying who the flow master is. It installs nothing and writes nothing
to `~/.claude` or `~/.codex`. [flows.md](flows.md#how-a-worker-reports) has the details.

## The first prompt

A worker's first prompt is what starts your workflow. flow takes the first of these that exists:

1. the agent's own `prompt`
2. `[worker] prompt`, exactly as written, for every agent
3. the prompt of the built-in agent with the same name (`/ship-feature {request}` for `claude`,
   `$ship-feature {request}` for `codex`)
4. no prompt at all

Since Claude Code calls a skill `/name` and Codex calls it `$name`, a `[worker] prompt` shared by
both should name no skill. Otherwise give each agent its own `prompt`.

A plain session never gets a first prompt.

## Errors

If `config.toml` is broken, `flow start`, `flow new` and `flow ui` stop with an error that names
the file, the line and the key:

```
error: "/Users/jan/.config/flow/config.toml is not valid: TOML parse error at line 2, column 1\n  |\n2 | agnet = \"x\"\n  | ^^^^^\nunknown field `agnet`, expected `agent` or `prompt`"
```

Workers that are already running keep reporting with the built-in settings until you fix the file.

Settings from earlier versions are refused, with a message that shows the new form. That covers a
top-level `agent` key (now `agent` under `[worker]`) and a `[phases]` table of skill names (now
`[[phases]]`).
