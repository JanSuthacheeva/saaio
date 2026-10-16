# Plan: [worker] agent

Review page: https://claude.ai/artifact/1XkJhQ5CoG7Qi51guEpoUS

## Decision

The worker's default agent moves from the top-level `agent` key to `[worker] agent`, beside
`[master] agent`. No fallback: a file with the top-level `agent` key fails to load with an error
naming `[worker] agent`. Every other unknown top-level key or section is an error too, naming the
file and the key.

```toml
[worker]
agent = "claude"     # the default for flow start

[master]
agent = "codex"
```

## Units

`src/config.rs` (modified):

```rust
/// [worker]: what a flow runs when flow start names no agent.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WorkerConfig {
    pub agent: String,          // default "claude"
}

impl Default for WorkerConfig { fn default() -> WorkerConfig; }

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub agents: BTreeMap<String, Agent>,
    pub phases: PhaseMap,
    pub worker: WorkerConfig,   // replaces `agent: String`
    pub master: MasterConfig,
}

impl Config {
    /// The file's text as a Config; refuses the top-level `agent` key, naming [worker] agent.
    fn parse(text: &str) -> Result<Config>;
}
```

`Config::load` calls `parse` and keeps its `{file} is not valid` context. `Config::agent(None)`,
`worker_agents` and `master_agent`'s fallback read `self.worker.agent`.

`src/cli/commands/start.rs` (`pick_agent`) and `src/server/panes.rs` (`open_master`):
`config.agent` -> `config.worker.agent`.

## Call chain: loading a config file

1. `Env` -> `Config::load(flow_config, xdg, home): Result<Config>`
2. `Config::load` -> `Config::parse(&text): Result<Config>`
3. `parse` -> `toml::from_str::<toml::Table>(text)`; a syntax error returns here.
4. Top-level `agent` present -> `bail!("the top-level `agent` key is now `agent` under [worker]")`
5. Else `toml::from_str::<Config>(text)` (the text again, not the table, so serde's errors keep
   their line and column); an unknown key or section returns its error naming it. toml's errors
   lose their trailing newline.
6. `load` adds `{file} is not valid`; the command exits 1 with that message.

## Test seams

- `config.rs` unit tests: existing ones on `[worker] agent`; new tests for the old key and for an
  unknown top-level key and section.
- `tests/cli.rs`: `flow start` with an old-style config exits 1 naming `[worker] agent`.

## Assumptions

- `[worker]` and each `[agents.<name>]` refuse unknown keys, like `[master]` (found in
  review: the README promised it for every key).
- Rust names stay `agent` (`config.worker.agent`, `Config::agent`).
- Old plans and `docs/design/` pages stay as written.
- HANDOVER gets one line beside the `[master] agent` line.

## Tasks

- [x] Add `WorkerConfig`, replace `Config.agent` with `Config.worker`, `deny_unknown_fields` on `Config`, `Config::parse` refusing the old key; update call sites; unit tests for the old key, an unknown top-level key and an unknown section
- [x] End-to-end test in `tests/cli.rs`: `flow start` with the old key exits 1 naming `[worker] agent`
- [x] README and HANDOVER on `[worker] agent`
- [x] End-to-end check: run `flow start` with an old-style and a new-style config against a scratch FLOW_HOME
