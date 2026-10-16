use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::config::Agent;
use crate::launch::pane::{self, LaunchEnv, PaneSpec};
use crate::proto::PaneKey;

/// The name the master session gets.
pub const MASTER_NAME: &str = "master";
/// What flow writes to master.md: how the master works with flow.
pub const DEFAULT_INSTRUCTIONS: &str = include_str!("master.md");
/// Earlier defaults: a master.md still holding one of them was never edited, so it gets the current one.
const PREVIOUS_DEFAULTS: [&str; 2] = [include_str!("master-v1.md"), include_str!("master-v2.md")];

/// Writes the default instructions to `file` when it is missing or still an earlier default; an edited
/// file is never overwritten.
pub fn ensure_instructions(file: &Path) -> Result<()> {
    match fs::read_to_string(file) {
        Ok(text) if text == DEFAULT_INSTRUCTIONS || !PREVIOUS_DEFAULTS.contains(&text.as_str()) => {
            return Ok(());
        }
        Ok(_) => {}
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => {
            return Err(error).with_context(|| format!("could not read {}", file.display()));
        }
    }
    if let Some(dir) = file.parent() {
        fs::create_dir_all(dir).with_context(|| format!("could not create {}", dir.display()))?;
    }
    fs::write(file, DEFAULT_INSTRUCTIONS)
        .with_context(|| format!("could not write {}", file.display()))
}

/// Whether the master starts a new session or resumes the one in master.json.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Start {
    Fresh,
    Resume,
}

/// The master's session id and the directory it runs in, kept across server restarts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MasterRecord {
    pub session: String,
    pub cwd: PathBuf,
}

impl MasterRecord {
    /// The record in master.json to resume, or a new one (fresh session id, cwd) written there.
    pub fn load_or_create(file: &Path, cwd: &Path) -> Result<(MasterRecord, Start)> {
        match fs::read_to_string(file) {
            Ok(text) => {
                let record = serde_json::from_str(&text)
                    .with_context(|| format!("could not read {}", file.display()))?;
                Ok((record, Start::Resume))
            }
            Err(error) if error.kind() == ErrorKind::NotFound => {
                let record = MasterRecord {
                    session: Uuid::new_v4().to_string(),
                    cwd: cwd.to_path_buf(),
                };
                if let Some(dir) = file.parent() {
                    fs::create_dir_all(dir)
                        .with_context(|| format!("could not create {}", dir.display()))?;
                }
                let text = serde_json::to_string_pretty(&record).expect("a record serialises");
                fs::write(file, text)
                    .with_context(|| format!("could not write {}", file.display()))?;
                Ok((record, Start::Fresh))
            }
            Err(error) => Err(error).with_context(|| format!("could not read {}", file.display())),
        }
    }

    /// The agent's command (name master, no prompt) when new, its resume command after, with
    /// {instructions} filled in and, when it `wake`s, its adapter's waiter wired in after the
    /// program; None when it must resume and the agent has no resume template.
    pub fn spec(
        &self,
        start: Start,
        agent: &Agent,
        env: &LaunchEnv,
        instructions: &Path,
        wake: bool,
    ) -> Option<PaneSpec> {
        let argv = match start {
            Start::Fresh => {
                let bare = Agent {
                    prompt: String::new(),
                    ..agent.clone()
                };
                pane::agent_argv(&bare, &self.session, MASTER_NAME, "")
            }
            Start::Resume => pane::resume_argv(agent, &self.session, MASTER_NAME)?,
        };
        let instructions = instructions.display().to_string();
        let mut argv: Vec<String> = argv
            .into_iter()
            .map(|part| part.replace("{instructions}", &instructions))
            .collect();
        if wake && let Some(adapter) = agent.adapter {
            let at = 1.min(argv.len());
            argv.splice(at..at, adapter.master_wiring(&env.flow_exe));
        }
        Some(PaneSpec {
            key: PaneKey::Master,
            argv,
            cwd: self.cwd.clone(),
            env: pane::pane_env(env),
        })
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;
    use crate::config::builtin_agents;

    #[test]
    fn creates_the_record_once_and_reads_it_back_after() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("state/master.json");

        let (created, first) = MasterRecord::load_or_create(&file, Path::new("/work")).unwrap();
        let (loaded, again) = MasterRecord::load_or_create(&file, Path::new("/elsewhere")).unwrap();

        assert_eq!(first, Start::Fresh);
        assert_eq!(again, Start::Resume);
        assert_eq!(loaded, created);
        assert_eq!(loaded.cwd, PathBuf::from("/work"));
        assert!(Uuid::parse_str(&loaded.session).is_ok());
    }

    #[test]
    fn names_the_file_when_it_does_not_parse() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("master.json");
        fs::write(&file, "{").unwrap();

        let error = MasterRecord::load_or_create(&file, Path::new("/work")).unwrap_err();

        assert!(format!("{error:#}").contains("master.json"), "{error:#}");
    }

    #[test]
    fn starts_claude_without_a_prompt_then_resumes_it() {
        let record = MasterRecord {
            session: "5e55".into(),
            cwd: PathBuf::from("/work"),
        };
        let agents = builtin_agents();
        let claude = &agents["claude"];

        let md = Path::new("/cfg/master.md");
        let fresh = record
            .spec(Start::Fresh, claude, &LaunchEnv::default(), md, false)
            .unwrap();
        let resumed = record
            .spec(Start::Resume, claude, &LaunchEnv::default(), md, false)
            .unwrap();

        assert_eq!(fresh.key, PaneKey::Master);
        assert_eq!(
            fresh.argv,
            ["claude", "-n", "master", "--session-id", "5e55"].map(String::from)
        );
        assert_eq!(
            resumed.argv,
            ["claude", "-n", "master", "--resume", "5e55"].map(String::from)
        );
        assert_eq!(resumed.cwd, PathBuf::from("/work"));
        assert!(resumed.env.iter().all(|(key, _)| key != "FLOW_ID"));
        let plain = Agent {
            adapter: None,
            resume: None,
            ..claude.clone()
        };
        assert_eq!(
            record.spec(Start::Resume, &plain, &LaunchEnv::default(), md, true),
            None
        );
    }

    #[test]
    fn gives_the_master_agent_its_instructions_fresh_and_resumed() {
        let record = MasterRecord {
            session: "5e55".into(),
            cwd: PathBuf::from("/work"),
        };
        let agents = builtin_agents();
        let master = &agents[crate::config::MASTER_AGENT];
        let md = Path::new("/cfg/master.md");

        let fresh = record
            .spec(Start::Fresh, master, &LaunchEnv::default(), md, false)
            .unwrap();
        let resumed = record
            .spec(Start::Resume, master, &LaunchEnv::default(), md, false)
            .unwrap();

        assert_eq!(
            fresh.argv,
            [
                "claude",
                "-n",
                "master",
                "--session-id",
                "5e55",
                "--append-system-prompt-file",
                "/cfg/master.md"
            ]
            .map(String::from)
        );
        assert_eq!(&resumed.argv[3..5], ["--resume", "5e55"]);
        assert_eq!(resumed.argv.last().unwrap(), "/cfg/master.md");
    }

    #[test]
    fn writes_the_default_instructions_once_and_keeps_an_edited_file() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("flow/master.md");

        ensure_instructions(&file).unwrap();
        assert_eq!(fs::read_to_string(&file).unwrap(), DEFAULT_INSTRUCTIONS);
        fs::write(&file, "mine").unwrap();
        ensure_instructions(&file).unwrap();

        assert_eq!(fs::read_to_string(&file).unwrap(), "mine");
    }

    #[test]
    fn replaces_an_earlier_default_nobody_edited() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("master.md");
        for previous in PREVIOUS_DEFAULTS {
            fs::write(&file, previous).unwrap();

            ensure_instructions(&file).unwrap();

            assert_eq!(fs::read_to_string(&file).unwrap(), DEFAULT_INSTRUCTIONS);
            assert_ne!(previous, DEFAULT_INSTRUCTIONS);
        }
    }

    #[test]
    fn wires_the_masters_waiter_in_when_it_wakes() {
        let record = MasterRecord {
            session: "5e55".into(),
            cwd: PathBuf::from("/work"),
        };
        let agents = builtin_agents();
        let master = &agents[crate::config::MASTER_AGENT];
        let md = Path::new("/cfg/master.md");
        let env = LaunchEnv {
            flow_exe: PathBuf::from("/bin/flow"),
            ..LaunchEnv::default()
        };

        for start in [Start::Fresh, Start::Resume] {
            let woken = record.spec(start, master, &env, md, true).unwrap();
            assert_eq!(woken.argv[0], "claude");
            assert_eq!(woken.argv[1], "--settings");
            assert!(
                woken.argv[2].contains(r#""args":["wait","--master"]"#),
                "{}",
                woken.argv[2]
            );
            assert_eq!(
                woken.argv[3..],
                record.spec(start, master, &env, md, false).unwrap().argv[1..]
            );
        }
        let plain = Agent {
            adapter: None,
            ..master.clone()
        };
        assert_eq!(
            record
                .spec(Start::Fresh, &plain, &env, md, true)
                .unwrap()
                .argv,
            record
                .spec(Start::Fresh, &plain, &env, md, false)
                .unwrap()
                .argv,
            "an agent without an adapter has nothing to wire in"
        );
    }
}
