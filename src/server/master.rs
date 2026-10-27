//! The master session: `MasterRecord` in master.json, its launch, master.md (`INSTRUCTIONS`)

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::config::Agent;
use crate::default_file::DefaultFile;
use crate::event::is_uuid;
use crate::launch::pane::{self, LaunchEnv, PaneSpec};
use crate::private;
use crate::proto::PaneKey;

/// The name the master session gets.
pub const MASTER_NAME: &str = "master";
/// What saaio writes to master.md: how the master works with saaio. An earlier default still in
/// master.md was never edited, so it gets the current one.
pub const INSTRUCTIONS: DefaultFile = DefaultFile {
    text: include_str!("master.md"),
    earlier: &[
        include_str!("master-v1.md"),
        include_str!("master-v2.md"),
        include_str!("master-v3.md"),
        include_str!("master-v4.md"),
        include_str!("master-v5.md"),
        include_str!("master-v6.md"),
        include_str!("master-v7.md"),
        include_str!("master-v8.md"),
        include_str!("master-v9.md"),
        include_str!("master-v10.md"),
        include_str!("master-v11.md"),
        include_str!("master-v12.md"),
    ],
};

/// master.md beside config.toml: its path for an agent's {instructions}, its text for the adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Instructions {
    pub path: PathBuf,
    pub text: String,
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
                    private::create_dir(dir)
                        .with_context(|| format!("could not create {}", dir.display()))?;
                }
                let text = serde_json::to_string_pretty(&record).expect("a record serialises");
                private::write_atomic(file, text.as_bytes())
                    .with_context(|| format!("could not write {}", file.display()))?;
                Ok((record, Start::Fresh))
            }
            Err(error) => Err(error).with_context(|| format!("could not read {}", file.display())),
        }
    }

    /// The agent's command (name master, no prompt) when new, its resume command after, by
    /// `session` when its harness picked one (else a new session), with {instructions} filled in.
    /// After the program: the adapter's master wiring (its hooks, which keep where its turn stands;
    /// Claude Code's waiter; Codex's hooks also record its session),
    /// then the instructions. None when it
    /// must resume and the agent has no resume template.
    pub fn spec(
        &self,
        start: Start,
        agent: &Agent,
        env: &LaunchEnv,
        instructions: &Instructions,
        session: Option<&str>,
    ) -> Option<PaneSpec> {
        let session = session.filter(|id| is_uuid(id));
        let start = match start {
            Start::Resume if session.is_none() && pane::names_session(agent) => Start::Fresh,
            start => start,
        };
        let argv = match start {
            Start::Fresh => {
                let bare = Agent {
                    prompt: None,
                    ..agent.clone()
                };
                pane::agent_argv(&bare, &self.session, MASTER_NAME, "")
            }
            Start::Resume => pane::resume_argv(
                agent,
                &self.session,
                MASTER_NAME,
                session.unwrap_or(&self.session),
            )?,
        };
        let path = instructions.path.display().to_string();
        let mut argv: Vec<String> = argv
            .into_iter()
            .map(|part| part.replace("{instructions}", &path))
            .collect();
        if let Some(adapter) = agent.adapter {
            let fresh = start == Start::Fresh;
            argv = adapter.wire_master(argv, &env.flow_exe, &instructions.text, fresh);
        }
        Some(PaneSpec {
            key: PaneKey::Master,
            argv,
            cwd: self.cwd.clone(),
            env: [(pane::MASTER_ENV.to_string(), "1".to_string())]
                .into_iter()
                .chain(pane::shared_env(env))
                .collect(),
            adapter: agent.adapter,
        })
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;
    use crate::adapter::Adapter;
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

    fn record() -> MasterRecord {
        MasterRecord {
            session: "5e55".into(),
            cwd: PathBuf::from("/work"),
        }
    }

    fn instructions() -> Instructions {
        Instructions {
            path: PathBuf::from("/cfg/master.md"),
            text: "# You are the master".into(),
        }
    }

    #[test]
    fn starts_an_agent_without_a_prompt_then_resumes_it() {
        let agents = builtin_agents();
        let bare = Agent {
            adapter: None,
            ..agents["claude"].clone()
        };
        let env = LaunchEnv::default();
        let md = instructions();

        let fresh = record().spec(Start::Fresh, &bare, &env, &md, None).unwrap();
        let resumed = record()
            .spec(Start::Resume, &bare, &env, &md, None)
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
        assert!(resumed.env.iter().all(|(key, _)| key != "SAAIO_FLOW_ID"));
        assert!(
            resumed
                .env
                .contains(&("SAAIO_MASTER".to_string(), "1".to_string()))
        );
        let unresumable = Agent {
            resume: None,
            ..bare.clone()
        };
        assert_eq!(
            record().spec(Start::Resume, &unresumable, &env, &md, None),
            None
        );
        let pointed = Agent {
            command: ["aider", "--read", "{instructions}"]
                .map(String::from)
                .to_vec(),
            ..bare
        };
        assert_eq!(
            record()
                .spec(Start::Fresh, &pointed, &env, &md, None)
                .unwrap()
                .argv[2],
            "/cfg/master.md",
            "{{instructions}} is the file"
        );
    }

    #[test]
    fn gives_the_claude_master_its_hooks_instructions_and_its_waiter() {
        let agents = builtin_agents();
        let master = &agents[crate::config::MASTER_AGENT];
        let env = LaunchEnv {
            flow_exe: PathBuf::from("/bin/saaio"),
            ..LaunchEnv::default()
        };
        let md = instructions();

        for start in [Start::Fresh, Start::Resume] {
            let spec = record().spec(start, master, &env, &md, None).unwrap();
            assert_eq!(spec.argv[0], "claude");
            assert_eq!(spec.argv[1], "--settings");
            assert!(
                spec.argv[2].contains(r#""args":["hook","claude-code"]"#)
                    && spec.argv[2].contains(r#""args":["wait","--master"]"#),
                "{}",
                spec.argv[2]
            );
            assert_eq!(
                spec.argv[3..5],
                ["--append-system-prompt", "# You are the master"]
            );
            assert!(
                !spec
                    .argv
                    .iter()
                    .any(|arg| arg == crate::adapter::MASTER_OPENING),
                "Claude Code wakes itself"
            );
        }
    }

    const SESSION: &str = "01a14000-0000-7000-8000-000000000001";

    #[test]
    fn a_codex_master_resumes_by_its_own_session_or_starts_again() {
        let agents = builtin_agents();
        let codex = &agents["codex"];
        let env = LaunchEnv {
            flow_exe: PathBuf::from("/bin/saaio"),
            ..LaunchEnv::default()
        };
        let md = instructions();
        let exe = Path::new("/bin/saaio");
        let mut wired = crate::adapter::codex::hooks(exe);
        wired.extend(Adapter::Codex.instruct("# You are the master"));

        let fresh = record().spec(Start::Fresh, codex, &env, &md, None).unwrap();
        let resumed = record()
            .spec(Start::Resume, codex, &env, &md, Some(SESSION))
            .unwrap();
        let forged = record()
            .spec(Start::Resume, codex, &env, &md, Some("-cnotify=x"))
            .unwrap();
        let never_ran = record()
            .spec(Start::Resume, codex, &env, &md, None)
            .unwrap();

        let opening = crate::adapter::MASTER_OPENING;
        assert_eq!(fresh.argv[0], "codex");
        assert_eq!(fresh.argv[1..=wired.len()], wired[..]);
        assert_eq!(
            fresh.argv[wired.len() + 1..],
            [opening],
            "a first turn at once, so it gets news from the start"
        );
        assert_eq!(resumed.argv[1..=wired.len()], wired[..]);
        assert_eq!(resumed.argv[wired.len() + 1..], ["resume", SESSION]);
        assert_eq!(never_ran.argv, fresh.argv, "no session yet: a new one");
        assert_eq!(forged.argv, fresh.argv, "no UUID: a new one");
    }

    #[test]
    fn a_master_md_left_at_the_last_default_takes_the_new_one() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("master.md");
        fs::write(&path, include_str!("master-v12.md")).unwrap();

        INSTRUCTIONS.ensure(&path).unwrap();

        let text = fs::read_to_string(&path).unwrap();
        assert_eq!(text, INSTRUCTIONS.text);
        assert!(
            text.contains("--base <the previous flow's branch>"),
            "{text}"
        );
    }

    #[test]
    fn every_earlier_default_differs_from_the_current_one() {
        for earlier in INSTRUCTIONS.earlier {
            assert_ne!(*earlier, INSTRUCTIONS.text);
        }
    }
}
