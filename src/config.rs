use std::collections::BTreeMap;
use std::fs;
use std::io::ErrorKind;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::adapter::Adapter;
use crate::event::Phase;

/// Skill name -> the phase it starts, for adapters that see skills start.
pub type PhaseMap = BTreeMap<String, Phase>;

/// How to launch one agent: its command line, the first prompt it gets, and how to resume it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Agent {
    pub command: Vec<String>,
    pub prompt: String,
    /// The command that continues session {id}; an agent without one is started fresh by hand.
    #[serde(default)]
    pub resume: Option<Vec<String>>,
    /// The harness flow observes it through ("claude-code"); without one, flow ui shows only
    /// whether its process runs.
    #[serde(default)]
    pub adapter: Option<Adapter>,
}

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct Config {
    pub agent: String,
    pub agents: BTreeMap<String, Agent>,
    /// Skill name -> phase: the built-in map with the file's entries over it.
    pub phases: PhaseMap,
    pub master: MasterConfig,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            agent: "claude".to_string(),
            agents: BTreeMap::new(),
            phases: PhaseMap::new(),
            master: MasterConfig::default(),
        }
    }
}

/// [master]: how the master works with its workers.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MasterConfig {
    /// flow wakes the master when a worker stops (wake = false: it learns only from flow status).
    pub wake: bool,
}

impl Default for MasterConfig {
    fn default() -> MasterConfig {
        MasterConfig { wake: true }
    }
}

impl Config {
    /// $FLOW_CONFIG, else $XDG_CONFIG_HOME/flow/config.toml, else ~/.config/flow/config.toml;
    /// the built-in agents alone when there is no file.
    pub fn load(
        flow_config: Option<PathBuf>,
        xdg_config: Option<PathBuf>,
        home: Option<PathBuf>,
    ) -> Result<Config> {
        let mut config = match config_file(flow_config, xdg_config, home) {
            None => Config::default(),
            Some(file) => match fs::read_to_string(&file) {
                Ok(text) => toml::from_str::<Config>(&text)
                    .with_context(|| format!("{} is not valid", file.display()))?,
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

    /// The built-in agents and phases alone, as if there were no config file.
    pub fn builtin() -> Config {
        let mut config = Config::default();
        config.add_builtins();
        config
    }

    /// The built-in agents and phases under what the file set.
    fn add_builtins(&mut self) {
        let mut agents = builtin_agents();
        agents.append(&mut self.agents);
        self.agents = agents;
        let mut phases = builtin_phases();
        phases.append(&mut self.phases);
        self.phases = phases;
    }

    /// The named agent, or the default one.
    pub fn agent(&self, name: Option<&str>) -> Option<(&str, &Agent)> {
        let name = name.unwrap_or(&self.agent);
        self.agents
            .get_key_value(name)
            .map(|(name, agent)| (name.as_str(), agent))
    }
}

/// $FLOW_CONFIG, else $XDG_CONFIG_HOME/flow/config.toml, else ~/.config/flow/config.toml.
pub fn config_file(
    flow_config: Option<PathBuf>,
    xdg_config: Option<PathBuf>,
    home: Option<PathBuf>,
) -> Option<PathBuf> {
    let set = |path: Option<PathBuf>| path.filter(|path| !path.as_os_str().is_empty());
    set(flow_config)
        .or_else(|| set(xdg_config).map(|dir| dir.join("flow/config.toml")))
        .or_else(|| set(home).map(|dir| dir.join(".config/flow/config.toml")))
}

/// The phases of the agent-skills chain: design-code, implement-plan, review-changes,
/// write-merge-request. A config file adds others or points these elsewhere.
pub fn builtin_phases() -> PhaseMap {
    PhaseMap::from([
        ("design-code".to_string(), Phase::Design),
        ("implement-plan".to_string(), Phase::Implement),
        ("review-changes".to_string(), Phase::Review),
        ("write-merge-request".to_string(), Phase::MergeRequest),
    ])
}

fn strings(parts: &[&str]) -> Vec<String> {
    parts.iter().copied().map(String::from).collect()
}

/// The agent flow ui runs as its master when there is one; else the default agent.
pub const MASTER_AGENT: &str = "master";

/// claude: ["claude", "-n", "{name}", "--session-id", "{id}", "{prompt}"], prompt "/ship-feature {request}",
/// resumed with ["claude", "-n", "{name}", "--resume", "{id}"]; codex: ["codex", "{prompt}"], prompt
/// "$ship-feature {request}", resumed by the session Codex picked ({session}); master: claude with the
/// orchestrator instructions appended to its system prompt ({instructions}, the master.md beside
/// config.toml).
pub fn builtin_agents() -> BTreeMap<String, Agent> {
    let claude = Agent {
        command: strings(&["claude", "-n", "{name}", "--session-id", "{id}", "{prompt}"]),
        prompt: "/ship-feature {request}".to_string(),
        adapter: Some(Adapter::ClaudeCode),
        resume: Some(strings(&["claude", "-n", "{name}", "--resume", "{id}"])),
    };
    let codex = Agent {
        command: strings(&["codex", "{prompt}"]),
        prompt: "$ship-feature {request}".to_string(),
        adapter: Some(Adapter::Codex),
        resume: Some(strings(&["codex", "resume", "{session}"])),
    };
    let master = Agent {
        command: strings(&[
            "claude",
            "-n",
            "{name}",
            "--session-id",
            "{id}",
            "--append-system-prompt-file",
            "{instructions}",
        ]),
        prompt: String::new(),
        adapter: Some(Adapter::ClaudeCode),
        resume: Some(strings(&[
            "claude",
            "-n",
            "{name}",
            "--resume",
            "{id}",
            "--append-system-prompt-file",
            "{instructions}",
        ])),
    };
    BTreeMap::from([
        ("claude".to_string(), claude),
        ("codex".to_string(), codex),
        (MASTER_AGENT.to_string(), master),
    ])
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;

    #[test]
    fn knows_claude_code_without_a_config_file() {
        let dir = TempDir::new().unwrap();
        let config = Config::load(Some(dir.path().join("missing.toml")), None, None).unwrap();

        let (name, agent) = config.agent(None).unwrap();
        assert_eq!(name, "claude");
        assert_eq!(
            agent.command,
            vec!["claude", "-n", "{name}", "--session-id", "{id}", "{prompt}"]
        );
        assert_eq!(agent.prompt, "/ship-feature {request}");
        assert_eq!(
            agent.resume.as_deref(),
            Some(&["claude", "-n", "{name}", "--resume", "{id}"].map(String::from)[..])
        );
        let (_, codex) = config.agent(Some("codex")).unwrap();
        assert_eq!(codex.command, ["codex", "{prompt}"]);
        assert_eq!(codex.prompt, "$ship-feature {request}");
        assert_eq!(
            codex.resume.as_deref(),
            Some(&["codex", "resume", "{session}"].map(String::from)[..])
        );
        assert_eq!(codex.adapter, Some(Adapter::Codex));
        assert!(config.agent(Some("pi")).is_none());
        let (_, master) = config.agent(Some(MASTER_AGENT)).unwrap();
        assert!(
            master
                .command
                .ends_with(&["--append-system-prompt-file", "{instructions}"].map(String::from))
        );
        assert_eq!(master.prompt, "");
    }

    #[test]
    fn maps_the_agent_skills_to_phases_and_takes_more_from_the_file() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("config.toml");
        fs::write(
            &file,
            "[phases]\nbrainstorm = \"design\"\nreview-changes = \"implement\"\n",
        )
        .unwrap();

        let config = Config::load(Some(file), None, None).unwrap();
        let none = Config::load(Some(dir.path().join("missing.toml")), None, None).unwrap();

        assert_eq!(none.phases, builtin_phases());
        assert_eq!(config.phases["brainstorm"], Phase::Design);
        assert_eq!(
            config.phases["review-changes"],
            Phase::Implement,
            "the file wins"
        );
        assert_eq!(config.phases["design-code"], Phase::Design);
        assert_eq!(
            none.agent(None).unwrap().1.adapter,
            Some(Adapter::ClaudeCode)
        );
        assert_eq!(
            none.agent(Some(MASTER_AGENT)).unwrap().1.adapter,
            Some(Adapter::ClaudeCode)
        );
    }

    #[test]
    fn adds_agents_and_overrides_the_built_in_ones_from_the_file() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("config.toml");
        fs::write(
            &file,
            r#"
agent = "codex"

[agents.codex]
command = ["codex", "{prompt}"]
prompt = "{request}"
resume = ["codex", "resume", "{id}"]

[agents.claude]
command = ["claude", "{prompt}"]
prompt = "/design-code {request}"
"#,
        )
        .unwrap();

        let config = Config::load(Some(file), None, None).unwrap();

        assert_eq!(config.agent(None).map(|(name, _)| name), Some("codex"));
        assert_eq!(
            config.agent(None).unwrap().1.resume,
            Some(vec![
                "codex".to_string(),
                "resume".to_string(),
                "{id}".to_string()
            ])
        );
        assert_eq!(config.agent(Some("claude")).unwrap().1.resume, None);
        assert_eq!(
            config.agent(Some("claude")).unwrap().1.prompt,
            "/design-code {request}"
        );
    }

    #[test]
    fn finds_the_file_by_flow_config_then_xdg_then_home() {
        let dir = TempDir::new().unwrap();
        let xdg = dir.path().join("xdg");
        fs::create_dir_all(xdg.join("flow")).unwrap();
        fs::write(xdg.join("flow/config.toml"), "agent = \"from-xdg\"\n").unwrap();
        let home = dir.path().join("home");
        fs::create_dir_all(home.join(".config/flow")).unwrap();
        fs::write(
            home.join(".config/flow/config.toml"),
            "agent = \"from-home\"\n",
        )
        .unwrap();

        assert_eq!(
            Config::load(None, Some(xdg), Some(home.clone()))
                .unwrap()
                .agent,
            "from-xdg"
        );
        assert_eq!(
            Config::load(None, None, Some(home)).unwrap().agent,
            "from-home"
        );
    }

    #[test]
    fn names_the_file_when_it_does_not_parse() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("config.toml");
        fs::write(&file, "agent = [").unwrap();

        let error = Config::load(Some(file.clone()), None, None).unwrap_err();

        assert!(
            format!("{error:#}").contains(&file.display().to_string()),
            "{error:#}"
        );
    }

    #[test]
    fn refuses_an_adapter_flow_does_not_know() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("config.toml");
        fs::write(
            &file,
            "[agents.aider]\ncommand = [\"aider\"]\nprompt = \"{request}\"\nadapter = \"claud-code\"\n",
        )
        .unwrap();

        let error = format!(
            "{:#}",
            Config::load(Some(file.clone()), None, None).unwrap_err()
        );

        assert!(error.contains(&file.display().to_string()), "{error}");
        assert!(error.contains("claud-code"), "{error}");
    }

    #[test]
    fn wakes_the_master_unless_the_file_says_not_to() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("config.toml");
        let load = || Config::load(Some(file.clone()), None, None).unwrap();

        assert!(load().master.wake, "no file");
        fs::write(&file, "agent = \"claude\"\n").unwrap();
        assert!(load().master.wake, "no [master]");
        fs::write(&file, "[master]\nwake = false\n").unwrap();
        assert!(!load().master.wake);
        fs::write(&file, "[master]\nwke = false\n").unwrap();
        assert!(
            Config::load(Some(file.clone()), None, None).is_err(),
            "a typo is an error"
        );
    }
}
