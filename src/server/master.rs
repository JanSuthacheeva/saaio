use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::config::Agent;
use crate::launch::{self, LaunchEnv, PaneSpec};
use crate::proto::PaneKey;

/// The name the master session gets.
pub const MASTER_NAME: &str = "master";

/// The master's session id and the directory it runs in, kept across server restarts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MasterRecord {
    pub session: String,
    pub cwd: PathBuf,
}

impl MasterRecord {
    /// The record in master.json, or a new one (fresh session id, cwd) written there; true when new.
    pub fn load_or_create(file: &Path, cwd: &Path) -> Result<(MasterRecord, bool)> {
        match fs::read_to_string(file) {
            Ok(text) => {
                let record = serde_json::from_str(&text)
                    .with_context(|| format!("could not read {}", file.display()))?;
                Ok((record, false))
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
                Ok((record, true))
            }
            Err(error) => Err(error).with_context(|| format!("could not read {}", file.display())),
        }
    }

    /// The agent's command (name master, no prompt) when new, its resume command after;
    /// None when it must resume and the agent has no resume template.
    pub fn spec(&self, fresh: bool, agent: &Agent, env: &LaunchEnv) -> Option<PaneSpec> {
        let argv = if fresh {
            let bare = Agent {
                prompt: String::new(),
                ..agent.clone()
            };
            launch::agent_argv(&bare, &self.session, MASTER_NAME, "")
        } else {
            launch::resume_argv(agent, &self.session, MASTER_NAME)?
        };
        Some(PaneSpec {
            key: PaneKey::Master,
            argv,
            cwd: self.cwd.clone(),
            env: launch::pane_env(None, env),
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

        let (created, fresh) = MasterRecord::load_or_create(&file, Path::new("/work")).unwrap();
        let (loaded, again) = MasterRecord::load_or_create(&file, Path::new("/elsewhere")).unwrap();

        assert!(fresh);
        assert!(!again);
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

        let fresh = record.spec(true, claude, &LaunchEnv::default()).unwrap();
        let resumed = record.spec(false, claude, &LaunchEnv::default()).unwrap();

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
            resume: None,
            ..claude.clone()
        };
        assert_eq!(record.spec(false, &plain, &LaunchEnv::default()), None);
    }
}
