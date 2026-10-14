use std::collections::BTreeMap;
use std::fs;
use std::io::ErrorKind;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::Deserialize;

/// How to launch one agent: its command line and the first prompt it gets.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Agent {
    pub command: Vec<String>,
    pub prompt: String,
}

#[derive(Debug, Deserialize)]
pub struct Config {
    #[serde(default = "default_agent_name")]
    pub agent: String,
    #[serde(default)]
    pub agents: BTreeMap<String, Agent>,
}

impl Config {
    /// $FLOW_CONFIG, else $XDG_CONFIG_HOME/flow/config.toml, else ~/.config/flow/config.toml;
    /// the built-in agents alone when there is no file.
    pub fn load(
        flow_config: Option<PathBuf>,
        xdg_config: Option<PathBuf>,
        home: Option<PathBuf>,
    ) -> Result<Config> {
        let set = |path: Option<PathBuf>| path.filter(|path| !path.as_os_str().is_empty());
        let file = set(flow_config)
            .or_else(|| set(xdg_config).map(|dir| dir.join("flow/config.toml")))
            .or_else(|| set(home).map(|dir| dir.join(".config/flow/config.toml")));
        let mut config = match file {
            None => Config::empty(),
            Some(file) => match fs::read_to_string(&file) {
                Ok(text) => toml::from_str::<Config>(&text)
                    .with_context(|| format!("could not read {}", file.display()))?,
                Err(error) if error.kind() == ErrorKind::NotFound => Config::empty(),
                Err(error) => {
                    return Err(error)
                        .with_context(|| format!("could not read {}", file.display()));
                }
            },
        };
        let mut agents = builtin_agents();
        agents.append(&mut config.agents);
        config.agents = agents;
        Ok(config)
    }

    /// The named agent, or the default one.
    pub fn agent(&self, name: Option<&str>) -> Option<(&str, &Agent)> {
        let name = name.unwrap_or(&self.agent);
        self.agents
            .get_key_value(name)
            .map(|(name, agent)| (name.as_str(), agent))
    }
}

/// claude: ["claude", "-n", "{name}", "--session-id", "{id}", "{prompt}"], prompt "/ship-feature {request}".
pub fn builtin_agents() -> BTreeMap<String, Agent> {
    let claude = Agent {
        command: ["claude", "-n", "{name}", "--session-id", "{id}", "{prompt}"]
            .map(String::from)
            .to_vec(),
        prompt: "/ship-feature {request}".to_string(),
    };
    BTreeMap::from([("claude".to_string(), claude)])
}

impl Config {
    fn empty() -> Config {
        Config {
            agent: default_agent_name(),
            agents: BTreeMap::new(),
        }
    }
}

fn default_agent_name() -> String {
    "claude".to_string()
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
        assert!(config.agent(Some("codex")).is_none());
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

[agents.claude]
command = ["claude", "{prompt}"]
prompt = "/design-code {request}"
"#,
        )
        .unwrap();

        let config = Config::load(Some(file), None, None).unwrap();

        assert_eq!(config.agent(None).map(|(name, _)| name), Some("codex"));
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
}
