//! `Config` from config.toml: agents, `[worker]`, `[kinds.*]`, `[master]`, `[ui]`, `[services]`

use std::collections::BTreeMap;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use serde::Deserialize;
#[cfg(test)]
use serde::Serialize;
use toml::de::{DeTable, DeValue};

use crate::adapter::Adapter;
use crate::config::kind::{DEFAULT_KIND, Kind};
use crate::config::prefix::Prefix;
use crate::default_file::DefaultFile;
use crate::text::{line_of, place, without_value};
use crate::workflow::Workflow;

pub mod kind;
pub mod prefix;
pub mod watch;

/// How to launch one agent: its command line, the first prompt it gets, and how to resume it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[cfg_attr(test, derive(Serialize))]
#[serde(deny_unknown_fields)]
pub struct Agent {
    pub command: Vec<String>,
    /// Its own first prompt, for every kind; None takes the flow's kind's.
    #[serde(default)]
    pub prompt: Option<String>,
    /// The command that continues session {id}; an agent without one is started fresh by hand.
    #[serde(default)]
    pub resume: Option<Vec<String>>,
    /// The harness saaio observes it through ("claude-code"); without one, saaio ui shows only
    /// whether its process runs.
    #[serde(default)]
    pub adapter: Option<Adapter>,
}

#[derive(Debug, Default, Deserialize)]
#[cfg_attr(test, derive(Serialize, PartialEq))]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub agents: BTreeMap<String, Agent>,
    /// [kinds.<name>]: the built-in feature, fix and research under the file's.
    pub kinds: BTreeMap<String, Kind>,
    /// The top-level [[phases]] of older files: the feature kind's after load.
    #[serde(rename = "phases", skip_serializing_if = "Option::is_none")]
    legacy_phases: Option<Workflow>,
    pub worker: WorkerConfig,
    pub master: MasterConfig,
    pub ui: UiConfig,
    pub services: ServicesConfig,
}

/// [worker]: what a flow runs when saaio start names no agent or kind.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[cfg_attr(test, derive(Serialize))]
#[serde(default, deny_unknown_fields)]
pub struct WorkerConfig {
    /// The agent saaio start runs, "claude" unless set.
    pub agent: String,
    /// The kind saaio start gives a flow, "feature" unless set.
    pub kind: String,
    /// The feature kind's first prompt, from older files; [kinds.feature] prompt wins.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
}

impl Default for WorkerConfig {
    fn default() -> WorkerConfig {
        WorkerConfig {
            agent: "claude".to_string(),
            kind: DEFAULT_KIND.to_string(),
            prompt: None,
        }
    }
}

/// [master]: how the master works with its workers.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[cfg_attr(test, derive(Serialize))]
#[serde(default, deny_unknown_fields)]
pub struct MasterConfig {
    /// saaio wakes the master when a worker stops (wake = false: it learns only from saaio status).
    pub wake: bool,
    /// The agent saaio ui runs as its master, "codex" for one; else MASTER_AGENT.
    pub agent: Option<String>,
}

impl Default for MasterConfig {
    fn default() -> MasterConfig {
        MasterConfig {
            wake: true,
            agent: None,
        }
    }
}

/// [ui]: how saaio ui takes keys, whether it looks for a new release and raises notifications.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[cfg_attr(test, derive(Serialize))]
#[serde(default, deny_unknown_fields)]
pub struct UiConfig {
    /// The key before every saaio ui command, C-space unless set.
    pub prefix: Prefix,
    /// The UI asks for the latest release once a day and names a newer one; true unless set.
    pub update_check: bool,
    /// A flow that comes to waiting or blocked out of sight raises a desktop notification; true
    /// unless set.
    pub notify: bool,
}

impl Default for UiConfig {
    fn default() -> UiConfig {
        UiConfig {
            prefix: Prefix::default(),
            update_check: true,
            notify: true,
        }
    }
}

/// [services]: when saaio pauses a resting flow's services.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[cfg_attr(test, derive(Serialize))]
#[serde(default, deny_unknown_fields)]
pub struct ServicesConfig {
    /// Minutes a flow rests before its services pause; 0 never pauses. 5 unless set.
    pub pause_after_minutes: u32,
}

impl Default for ServicesConfig {
    fn default() -> ServicesConfig {
        ServicesConfig {
            pause_after_minutes: 5,
        }
    }
}

impl ServicesConfig {
    /// The delay; None at 0.
    pub fn pause_after(&self) -> Option<Duration> {
        (self.pause_after_minutes > 0)
            .then(|| Duration::from_mins(u64::from(self.pause_after_minutes)))
    }
}

impl Config {
    /// The file's config with the built-in agents; the built-in alone when `file` is None or
    /// missing. Errors read "<file>:<line>: <message>".
    pub fn read(file: Option<&Path>) -> Result<Config> {
        let mut config = match file {
            None => Config::default(),
            Some(file) => match fs::read_to_string(file) {
                Ok(text) => Config::parse(file, &text)?,
                Err(error) if error.kind() == ErrorKind::NotFound => Config::default(),
                Err(error) => {
                    return Err(error)
                        .with_context(|| format!("could not read {}", file.display()));
                }
            },
        };
        config.add_builtins();
        Ok(config)
    }

    /// The file's text as a Config; refuses the top-level `agent` key, naming [worker] agent, the
    /// [phases] skill table, naming [[phases]], [[phases]] beside [[kinds.feature.phases]], and a
    /// [worker] kind there is none of, each on the line of its key.
    fn parse(path: &Path, text: &str) -> Result<Config> {
        let at = |error: toml::de::Error| {
            let line = line_of(text, error.span().map_or(0, |span| span.start));
            anyhow!("{}", without_value(error.message().trim_end())).context(place(path, line))
        };
        let table = DeTable::parse(text).map_err(at)?.into_inner();
        let refuse = |at: usize, refusal: &str| {
            Err(anyhow!("{refusal}").context(place(path, line_of(text, at))))
        };
        let feature_phases = nested(&table, &["kinds", DEFAULT_KIND, "phases"]).is_some();
        for (key, value) in &table {
            let refusal = match key.get_ref().as_ref() {
                "agent" => "the top-level `agent` key is now `agent` under [worker]",
                "phases" if matches!(value.get_ref(), DeValue::Table(_)) => {
                    "[phases] is now a kind's list of phases: [[kinds.feature.phases]] with \
                     name = \"plan\" and skills = [\"design-code\"], one per phase in order"
                }
                "phases" if feature_phases => {
                    "[[phases]] is the feature kind's; set it in one place, \
                     [[kinds.feature.phases]]"
                }
                _ => continue,
            };
            return refuse(key.span().start, refusal);
        }
        let config: Config = toml::from_str(text).map_err(at)?;
        let kind = &config.worker.kind;
        if !config.kinds.contains_key(kind) && !Kind::builtins().contains_key(kind) {
            let at = nested(&table, &["worker", "kind"]).unwrap_or(0);
            return refuse(at, &format!("no kind named {kind}"));
        }
        Ok(config)
    }

    /// The built-in agents and workflow alone, as if there were no config file.
    pub fn builtin() -> Config {
        let mut config = Config::default();
        config.add_builtins();
        config
    }

    /// The built-in agents and kinds under what the file set; the older top-level [[phases]] and
    /// [worker] prompt into the feature kind.
    fn add_builtins(&mut self) {
        let mut agents = builtin_agents();
        agents.append(&mut self.agents);
        self.agents = agents;
        let mut kinds = Kind::builtins();
        kinds.append(&mut self.kinds);
        self.kinds = kinds;
        if let Some(feature) = self.kinds.get_mut(DEFAULT_KIND) {
            if let Some(phases) = self.legacy_phases.take() {
                feature.phases = phases;
            }
            if feature.prompt.is_none() {
                feature.prompt.clone_from(&self.worker.prompt);
            }
        }
    }

    /// The master's agent: [master] agent, else MASTER_AGENT, else the default agent; None when
    /// [master] names an agent there is none of.
    pub fn master_agent(&self) -> Option<(&str, &Agent)> {
        match &self.master.agent {
            Some(name) => self.agent(Some(name)),
            None => self.agent(Some(MASTER_AGENT)).or_else(|| self.agent(None)),
        }
    }

    /// The agents a flow can run, the default first: every one but MASTER_AGENT.
    pub fn worker_agents(&self) -> Vec<&str> {
        let others = self
            .agents
            .keys()
            .map(String::as_str)
            .filter(|name| *name != MASTER_AGENT && *name != self.worker.agent);
        self.agent(None)
            .map(|(name, _)| name)
            .into_iter()
            .chain(others)
            .collect()
    }

    /// The named kind, or the default one.
    pub fn kind(&self, name: Option<&str>) -> Option<(&str, &Kind)> {
        let name = name.unwrap_or(&self.worker.kind);
        self.kinds
            .get_key_value(name)
            .map(|(name, kind)| (name.as_str(), kind))
    }

    /// The kinds a flow can be, the default first.
    pub fn worker_kinds(&self) -> Vec<(&str, &Kind)> {
        let others = self
            .kinds
            .iter()
            .filter(|(name, _)| **name != self.worker.kind)
            .map(|(name, kind)| (name.as_str(), kind));
        self.kind(None).into_iter().chain(others).collect()
    }

    /// The default kind's workflow: the master's, and an older flow's that recorded none.
    pub fn workflow(&self) -> Workflow {
        self.kind(None)
            .map_or_else(Workflow::builtin, |(_, kind)| kind.phases.clone())
    }

    /// The named agent, or the default one.
    pub fn agent(&self, name: Option<&str>) -> Option<(&str, &Agent)> {
        let name = name.unwrap_or(&self.worker.agent);
        self.agents
            .get_key_value(name)
            .map(|(name, agent)| (name.as_str(), agent))
    }
}

/// $SAAIO_CONFIG, else $XDG_CONFIG_HOME/saaio/config.toml, else ~/.config/saaio/config.toml.
pub fn path(
    saaio_config: Option<PathBuf>,
    xdg_config: Option<PathBuf>,
    home: Option<PathBuf>,
) -> Option<PathBuf> {
    let set = |path: Option<PathBuf>| path.filter(|path| !path.as_os_str().is_empty());
    set(saaio_config)
        .or_else(|| set(xdg_config).map(|dir| dir.join("saaio/config.toml")))
        .or_else(|| set(home).map(|dir| dir.join(".config/saaio/config.toml")))
}

/// Where the value under `keys` starts in the file, table by table.
fn nested(table: &DeTable, keys: &[&str]) -> Option<usize> {
    let (first, rest) = keys.split_first()?;
    let (key, value) = table.get_key_value(*first)?;
    if rest.is_empty() {
        return Some(key.span().start);
    }
    match value.get_ref() {
        DeValue::Table(inner) => nested(inner, rest),
        _ => None,
    }
}

fn strings(parts: &[&str]) -> Vec<String> {
    parts.iter().copied().map(String::from).collect()
}

/// What saaio writes as config.toml on its first run: every setting at its default, commented out.
pub const EXAMPLE: DefaultFile = DefaultFile {
    text: include_str!("../examples/config.toml"),
    earlier: &[],
};

/// The agent saaio ui runs as its master when there is one and [master] names no other; else the
/// default agent.
pub const MASTER_AGENT: &str = "master";

/// claude: ["claude", "-n", "{name}", "--session-id", "{id}", "{prompt}"], resumed with
/// ["claude", "-n", "{name}", "--resume", "{id}"]; codex: ["codex", "{prompt}"], resumed by the
/// session Codex picked ({session}); both asked the flow's kind's prompt; master: claude without
/// a prompt, to which saaio server adds the orchestrator instructions (master.md).
pub fn builtin_agents() -> BTreeMap<String, Agent> {
    let claude = Agent {
        command: strings(&["claude", "-n", "{name}", "--session-id", "{id}", "{prompt}"]),
        prompt: None,
        adapter: Some(Adapter::ClaudeCode),
        resume: Some(strings(&["claude", "-n", "{name}", "--resume", "{id}"])),
    };
    let codex = Agent {
        command: strings(&["codex", "{prompt}"]),
        prompt: None,
        adapter: Some(Adapter::Codex),
        resume: Some(strings(&["codex", "resume", "{session}"])),
    };
    let master = Agent {
        command: strings(&["claude", "-n", "{name}", "--session-id", "{id}"]),
        prompt: Some(String::new()),
        adapter: Some(Adapter::ClaudeCode),
        resume: Some(strings(&["claude", "-n", "{name}", "--resume", "{id}"])),
    };
    BTreeMap::from([
        ("claude".to_string(), claude),
        ("codex".to_string(), codex),
        (MASTER_AGENT.to_string(), master),
    ])
}

#[cfg(test)]
mod tests;
