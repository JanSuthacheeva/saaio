use std::collections::BTreeMap;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use serde::Deserialize;
#[cfg(test)]
use serde::Serialize;
use toml::de::{DeTable, DeValue};

use crate::adapter::Adapter;
use crate::default_file::DefaultFile;
use crate::text::{line_of, place, without_value};
use crate::ui::prefix::Prefix;
use crate::workflow::Workflow;

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

/// [ui]: how saaio ui takes keys.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[cfg_attr(test, derive(Serialize))]
#[serde(default, deny_unknown_fields)]
pub struct UiConfig {
    /// The key before every saaio ui command, C-space unless set.
    pub prefix: Prefix,
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
pub fn config_file(
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
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;
    use crate::default_file::uncommented;

    #[test]
    fn the_example_uncommented_is_the_built_in_config() {
        let text = uncommented(EXAMPLE.text);

        let mut example = Config::parse(Path::new("config.toml"), &text).unwrap();
        example.add_builtins();

        assert_eq!(example, Config::builtin());
    }

    #[test]
    fn the_example_lists_every_setting_and_no_other() {
        let listed: toml::Table = toml::from_str(&uncommented(EXAMPLE.text)).unwrap();
        let builtin = toml::Table::try_from(Config::builtin()).unwrap();

        assert_eq!(listed, builtin);
    }

    #[test]
    fn the_example_as_it_is_parses_to_the_built_in_config() {
        let mut example = Config::parse(Path::new("config.toml"), EXAMPLE.text).unwrap();
        example.add_builtins();

        assert_eq!(example, Config::builtin());
    }

    #[test]
    fn knows_claude_code_without_a_config_file() {
        let dir = TempDir::new().unwrap();
        let config = Config::read(Some(&dir.path().join("missing.toml"))).unwrap();

        let (name, agent) = config.agent(None).unwrap();
        assert_eq!(name, "claude");
        assert_eq!(
            agent.command,
            vec!["claude", "-n", "{name}", "--session-id", "{id}", "{prompt}"]
        );
        assert_eq!(agent.prompt.as_deref(), Some("/ship-feature {request}"));
        assert_eq!(
            agent.resume.as_deref(),
            Some(&["claude", "-n", "{name}", "--resume", "{id}"].map(String::from)[..])
        );
        let (_, codex) = config.agent(Some("codex")).unwrap();
        assert_eq!(codex.command, ["codex", "{prompt}"]);
        assert_eq!(codex.prompt.as_deref(), Some("$ship-feature {request}"));
        assert_eq!(
            codex.resume.as_deref(),
            Some(&["codex", "resume", "{session}"].map(String::from)[..])
        );
        assert_eq!(codex.adapter, Some(Adapter::Codex));
        assert!(config.agent(Some("pi")).is_none());
        let (_, master) = config.agent(Some(MASTER_AGENT)).unwrap();
        assert_eq!(
            master.command,
            ["claude", "-n", "{name}", "--session-id", "{id}"]
        );
        assert_eq!(master.prompt.as_deref(), Some(""));
    }

    #[test]
    fn reads_the_workflow_from_the_file_or_takes_the_built_in_one() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("config.toml");
        fs::write(
            &file,
            "[[phases]]\nname = \"spec\"\nskills = [\"brainstorming\"]\n\n\
             [[phases]]\nname = \"build\"\nskills = [\"executing-plans\", \"tdd\"]\n",
        )
        .unwrap();

        let config = Config::read(Some(&file)).unwrap();
        let none = Config::read(Some(&dir.path().join("missing.toml"))).unwrap();

        assert_eq!(none.workflow, Workflow::builtin());
        assert_eq!(
            config.workflow,
            Workflow::of(&[
                ("spec", &["brainstorming"]),
                ("build", &["executing-plans", "tdd"])
            ]),
            "the file's list replaces the built-in one whole"
        );
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
    fn refuses_a_workflow_it_cannot_draw_on_its_line() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("config.toml");
        let error = |text: &str| {
            fs::write(&file, text).unwrap();
            format!("{:#}", Config::read(Some(&file)).unwrap_err())
        };
        let phase = |name: &str| format!("[[phases]]\nname = \"{name}\"\n");

        let five = error(&["a", "b", "c", "d", "e"].map(phase).concat());
        assert!(five.contains(&file.display().to_string()), "{five}");
        assert!(five.contains("a workflow has 1 to 4 phases"), "{five}");
        let twice = error(&[phase("review"), phase("review")].concat());
        assert!(twice.contains("two phases are named review"), "{twice}");
        let name = error(&phase("Review"));
        assert!(name.contains("lowercase letters, digits and -"), "{name}");
        assert!(name.contains("config.toml:2: "), "{name}");
        let key = error("[[phases]]\nname = \"plan\"\nskill = [\"design-code\"]\n");
        assert!(key.contains("skill"), "{key}");
    }

    #[test]
    fn names_the_new_shape_when_the_file_has_the_old_phases_table() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("config.toml");
        fs::write(&file, "[phases]\nbrainstorm = \"design\"\n").unwrap();

        let error = format!("{:#}", Config::read(Some(&file)).unwrap_err());

        assert!(error.contains(&file.display().to_string()), "{error}");
        assert!(error.contains("[[phases]]"), "{error}");
    }

    #[test]
    fn adds_agents_and_overrides_the_built_in_ones_from_the_file() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("config.toml");
        fs::write(
            &file,
            r#"
[worker]
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

        let config = Config::read(Some(&file)).unwrap();

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
            config.agent(Some("claude")).unwrap().1.prompt.as_deref(),
            Some("/design-code {request}")
        );
    }

    #[test]
    fn worker_prompt_overrides_the_built_in_prompts_but_not_an_agents_own() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("config.toml");
        fs::write(
            &file,
            r#"
[worker]
prompt = "/my-flow {request}"

[agents.aider]
command = ["aider", "--message", "{prompt}"]
prompt = "{request}"

[agents.pi]
command = ["pi", "{prompt}"]
"#,
        )
        .unwrap();

        let config = Config::read(Some(&file)).unwrap();
        let prompt = |name| config.agent(Some(name)).unwrap().1.prompt.as_deref();

        assert_eq!(prompt("claude"), Some("/my-flow {request}"));
        assert_eq!(prompt("codex"), Some("/my-flow {request}"), "as written");
        assert_eq!(prompt("aider"), Some("{request}"), "its own wins");
        assert_eq!(prompt("pi"), Some("/my-flow {request}"));
    }

    #[test]
    fn an_agent_without_a_prompt_keeps_the_built_in_one_of_its_name() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("config.toml");
        fs::write(
            &file,
            "[agents.claude]\ncommand = [\"claude\", \"--verbose\", \"{prompt}\"]\n\n\
             [agents.pi]\ncommand = [\"pi\"]\n",
        )
        .unwrap();

        let config = Config::read(Some(&file)).unwrap();
        let prompt = |name| config.agent(Some(name)).unwrap().1.prompt.as_deref();

        assert_eq!(prompt("claude"), Some("/ship-feature {request}"));
        assert_eq!(prompt("codex"), Some("$ship-feature {request}"));
        assert_eq!(prompt("pi"), Some(""), "no prompt anywhere");
    }

    #[test]
    fn finds_the_file_by_flow_config_then_xdg_then_home() {
        let dir = TempDir::new().unwrap();
        let xdg = dir.path().join("xdg");
        fs::create_dir_all(xdg.join("saaio")).unwrap();
        fs::write(
            xdg.join("saaio/config.toml"),
            "[worker]\nagent = \"from-xdg\"\n",
        )
        .unwrap();
        let home = dir.path().join("home");
        fs::create_dir_all(home.join(".config/saaio")).unwrap();
        fs::write(
            home.join(".config/saaio/config.toml"),
            "[worker]\nagent = \"from-home\"\n",
        )
        .unwrap();

        assert_eq!(
            Config::read(config_file(None, Some(xdg), Some(home.clone())).as_deref())
                .unwrap()
                .worker
                .agent,
            "from-xdg"
        );
        assert_eq!(
            Config::read(config_file(None, None, Some(home)).as_deref())
                .unwrap()
                .worker
                .agent,
            "from-home"
        );
    }

    #[test]
    fn says_every_error_as_file_line_and_message() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("config.toml");
        let error = |text: &str| {
            fs::write(&file, text).unwrap();
            format!("{:#}", Config::read(Some(&file)).unwrap_err())
        };
        let at = |line: usize| format!("{}:{line}: ", file.display());

        let unknown = error("[worker]\nagnet = \"x\"\n");
        assert_eq!(
            unknown,
            format!(
                "{}unknown field `agnet`, expected `agent` or `prompt`",
                at(2)
            )
        );
        assert!(error("\n\n[worker\n").starts_with(&at(3)));
        assert_eq!(
            error("[ui]\nprefix = \"C-b\"\nagent = \"codex\"\n"),
            format!("{}unknown field `agent`, expected `prefix`", at(3))
        );
        let top = error("# old\nagent = \"codex\"\n");
        assert!(top.starts_with(&at(2)), "{top}");
        assert!(top.contains("`agent` under [worker]"), "{top}");
        let phases = error("\n[phases]\nplan = \"design-code\"\n");
        assert!(phases.starts_with(&at(2)), "{phases}");
        assert!(phases.contains("[[phases]]"), "{phases}");
        assert_eq!(
            error("[master]\nwake = \"sk-secret\"\n"),
            format!("{}invalid type: a string, expected a boolean", at(2)),
            "the value can be a secret pasted in the wrong place"
        );
    }

    #[test]
    fn reads_the_built_in_config_without_a_file() {
        assert_eq!(Config::read(None).unwrap(), Config::builtin());
    }

    #[test]
    fn names_the_file_when_it_does_not_parse() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("config.toml");
        fs::write(&file, "[worker]\nagent = [").unwrap();

        let error = Config::read(Some(&file)).unwrap_err();

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

        let error = format!("{:#}", Config::read(Some(&file)).unwrap_err());

        assert!(error.contains(&file.display().to_string()), "{error}");
        assert!(
            error.ends_with(":4: unknown variant, expected `claude-code` or `codex`"),
            "{error}"
        );
    }

    #[test]
    fn wakes_the_master_unless_the_file_says_not_to() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("config.toml");
        let load = || Config::read(Some(&file)).unwrap();

        assert!(load().master.wake, "no file");
        fs::write(&file, "[worker]\nagent = \"claude\"\n").unwrap();
        assert!(load().master.wake, "no [master]");
        fs::write(&file, "[master]\nwake = false\n").unwrap();
        assert!(!load().master.wake);
        fs::write(&file, "[master]\nwke = false\n").unwrap();
        assert!(Config::read(Some(&file)).is_err(), "a typo is an error");
    }

    #[test]
    fn runs_the_master_agent_unless_the_file_names_another() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("config.toml");
        let load = || Config::read(Some(&file)).unwrap();

        assert_eq!(
            load().master_agent().map(|(name, _)| name),
            Some(MASTER_AGENT)
        );
        fs::write(&file, "[master]\nagent = \"codex\"\n").unwrap();
        assert_eq!(load().master_agent().map(|(name, _)| name), Some("codex"));
        fs::write(&file, "[master]\nagent = \"pi\"\n").unwrap();
        assert_eq!(
            load().master_agent().map(|(name, _)| name),
            None,
            "no such agent"
        );
        let builtin = Config::builtin();
        let (_, master) = builtin.master_agent().unwrap();
        assert!(
            !master
                .command
                .iter()
                .any(|part| part.contains("{instructions}")),
            "the server adds the instructions through the adapter"
        );
    }

    #[test]
    fn lists_the_agents_a_flow_can_run_the_default_first() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("config.toml");
        fs::write(&file, "[worker]\nagent = \"codex\"\n").unwrap();

        let config = Config::read(Some(&file)).unwrap();

        assert_eq!(config.worker_agents(), ["codex", "claude"]);
        assert_eq!(Config::builtin().worker_agents(), ["claude", "codex"]);
    }

    #[test]
    fn names_worker_agent_when_the_file_has_the_old_key() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("config.toml");
        fs::write(&file, "agent = \"codex\"\n").unwrap();

        let error = format!("{:#}", Config::read(Some(&file)).unwrap_err());

        assert!(error.contains(&file.display().to_string()), "{error}");
        assert!(error.contains("[worker]"), "{error}");
    }

    #[test]
    fn refuses_a_top_level_key_or_section_flow_does_not_know() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("config.toml");
        let error = |text: &str| {
            fs::write(&file, text).unwrap();
            format!("{:#}", Config::read(Some(&file)).unwrap_err())
        };

        let key = error("agnets = 1\n");
        assert!(key.contains(&file.display().to_string()), "{key}");
        assert!(key.contains("agnets"), "{key}");
        assert!(key.contains("config.toml:1: "), "{key}");
        assert!(!key.ends_with('\n'), "{key:?}");
        let section = error("[mastr]\nwake = false\n");
        assert!(section.contains("mastr"), "{section}");
        let worker = error("[worker]\nagnet = \"codex\"\n");
        assert!(worker.contains("agnet"), "{worker}");
        let agent = error("[agents.pi]\ncommand = [\"pi\"]\nprompt = \"{request}\"\nresum = []\n");
        assert!(agent.contains("resum"), "{agent}");
    }

    #[test]
    fn reads_the_ui_prefix_and_refuses_one_a_pane_needs() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("config.toml");
        let load = |text: &str| {
            fs::write(&file, text).unwrap();
            Config::read(Some(&file))
        };

        assert_eq!(Config::builtin().ui.prefix, Prefix::default());
        assert_eq!(
            load("[ui]\nprefix = \"C-b\"\n").unwrap().ui.prefix,
            Prefix::try_from("C-b".to_string()).unwrap()
        );
        let needed = format!("{:#}", load("[ui]\nprefix = \"C-c\"\n").unwrap_err());
        assert!(needed.contains(&file.display().to_string()), "{needed}");
        assert!(needed.contains("config.toml:2: "), "{needed}");
        assert!(
            needed.contains("C-c interrupts the agent in a pane"),
            "{needed}"
        );
        let unknown = format!("{:#}", load("[ui]\nprefx = \"C-b\"\n").unwrap_err());
        assert!(unknown.contains("prefx"), "{unknown}");
    }
}
