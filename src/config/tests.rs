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
        Config::read(path(None, Some(xdg), Some(home.clone())).as_deref())
            .unwrap()
            .worker
            .agent,
        "from-xdg"
    );
    assert_eq!(
        Config::read(path(None, None, Some(home)).as_deref())
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
fn pauses_services_after_five_minutes_unless_set_and_never_at_zero() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("config.toml");
    let read = |text: &str| {
        fs::write(&file, text).unwrap();
        Config::read(Some(&file)).map(|config| config.services.pause_after())
    };

    assert_eq!(read("").unwrap(), Some(Duration::from_mins(5)));
    assert_eq!(
        read("[services]\npause_after_minutes = 12\n").unwrap(),
        Some(Duration::from_mins(12))
    );
    assert_eq!(read("[services]\npause_after_minutes = 0\n").unwrap(), None);
    let unknown = format!("{:#}", read("[services]\npause_after = 5\n").unwrap_err());
    assert!(
        unknown.contains("expected `pause_after_minutes`"),
        "{unknown}"
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
