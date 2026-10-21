//! `Config` from config.toml: agents, `[worker]`, `[[phases]]`, `[master]`, `[ui]`, `[services]`

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
use crate::config::prefix::Prefix;
use crate::default_file::DefaultFile;
use crate::text::{line_of, place, without_value};
use crate::workflow::Workflow;

pub mod prefix;
pub mod watch;

/// How to launch one agent: its command line, the first prompt it gets, and how to resume it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[cfg_attr(test, derive(Serialize))]
#[serde(deny_unknown_fields)]
pub struct Agent {
    pub command: Vec<String>,
    /// Its own first prompt; None takes [worker] prompt, else the built-in agent's of its name.
    /// Some after load for every agent.
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
    /// [[phases]]: the built-in workflow unless the file has one.
    #[serde(rename = "phases")]
    pub workflow: Workflow,
    pub worker: WorkerConfig,
    pub master: MasterConfig,
    pub ui: UiConfig,
    pub services: ServicesConfig,
}

/// [worker]: what a flow runs when saaio start names no agent, and what it is asked first.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[cfg_attr(test, derive(Serialize))]
#[serde(default, deny_unknown_fields)]
pub struct WorkerConfig {
    /// The agent saaio start runs, "claude" unless set.
    pub agent: String,
    /// The first prompt of every agent the file gives none, the built-in ones included; unset,
    /// each keeps its built-in default.
    pub prompt: Option<String>,
}

impl Default for WorkerConfig {
    fn default() -> WorkerConfig {
        WorkerConfig {
            agent: "claude".to_string(),
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

/// [ui]: how saaio ui takes keys, and whether it looks for a new release.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[cfg_attr(test, derive(Serialize))]
#[serde(default, deny_unknown_fields)]
pub struct UiConfig {
    /// The key before every saaio ui command, C-space unless set.
    pub prefix: Prefix,
    /// The UI asks for the latest release once a day and names a newer one; true unless set.
    pub update_check: bool,
}

impl Default for UiConfig {
    fn default() -> UiConfig {
        UiConfig {
            prefix: Prefix::default(),
            update_check: true,
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

    /// The file's text as a Config; refuses the top-level `agent` key, naming [worker] agent, and
    /// the [phases] skill table, naming [[phases]], each on the line of its key.
    fn parse(path: &Path, text: &str) -> Result<Config> {
        let at = |error: toml::de::Error| {
            let line = line_of(text, error.span().map_or(0, |span| span.start));
            anyhow!("{}", without_value(error.message().trim_end())).context(place(path, line))
        };
        let table = DeTable::parse(text).map_err(at)?.into_inner();
        for (key, value) in &table {
            let refusal = match key.get_ref().as_ref() {
                "agent" => "the top-level `agent` key is now `agent` under [worker]",
                "phases" if matches!(value.get_ref(), DeValue::Table(_)) => {
                    "[phases] is now a list of phases: [[phases]] with name = \"plan\" and \
                     skills = [\"design-code\"], one per phase in order"
                }
                _ => continue,
            };
            let line = line_of(text, key.span().start);
            return Err(anyhow!(refusal).context(place(path, line)));
        }
        toml::from_str(text).map_err(at)
    }

    /// The built-in agents and workflow alone, as if there were no config file.
    pub fn builtin() -> Config {
        let mut config = Config::default();
        config.add_builtins();
        config
    }

    /// The built-in agents under what the file set, each with its first prompt.
    fn add_builtins(&mut self) {
        let builtins = builtin_agents();
        let mut agents: BTreeMap<String, Agent> = builtins
            .iter()
            .map(|(name, agent)| {
                let bare = Agent {
                    prompt: None,
                    ..agent.clone()
                };
                (name.clone(), bare)
            })
            .collect();
        agents.append(&mut self.agents);
        self.agents = agents;
        self.resolve_prompts(&builtins);
    }

    /// Each agent's first prompt: its own from the file, else [worker] prompt, else the built-in
    /// agent's of its name, else none.
    fn resolve_prompts(&mut self, builtins: &BTreeMap<String, Agent>) {
        for (name, agent) in &mut self.agents {
            if agent.prompt.is_some() {
                continue;
            }
            let builtin = builtins.get(name).and_then(|agent| agent.prompt.clone());
            agent.prompt = Some(self.worker.prompt.clone().or(builtin).unwrap_or_default());
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

/// claude: ["claude", "-n", "{name}", "--session-id", "{id}", "{prompt}"], prompt "/ship-feature {request}",
/// resumed with ["claude", "-n", "{name}", "--resume", "{id}"]; codex: ["codex", "{prompt}"], prompt
/// "$ship-feature {request}", resumed by the session Codex picked ({session}); master: claude, to
/// which saaio server adds the orchestrator instructions (the master.md beside config.toml).
pub fn builtin_agents() -> BTreeMap<String, Agent> {
    let claude = Agent {
        command: strings(&["claude", "-n", "{name}", "--session-id", "{id}", "{prompt}"]),
        prompt: Some("/ship-feature {request}".to_string()),
        adapter: Some(Adapter::ClaudeCode),
        resume: Some(strings(&["claude", "-n", "{name}", "--resume", "{id}"])),
    };
    let codex = Agent {
        command: strings(&["codex", "{prompt}"]),
        prompt: Some("$ship-feature {request}".to_string()),
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
