use std::env;
use std::path::{Path, PathBuf};

use crate::config::{self, Config};
use crate::registry::Registry;
use crate::store::Store;
use crate::ui::clipboard::Clipboard;
use crate::workspace;

/// The environment saaio runs in: its variables and its terminal, read once by main so commands get
/// them as values.
#[expect(
    clippy::struct_excessive_bools,
    reason = "independent facts about the environment"
)]
#[derive(Debug, Clone, Default)]
pub struct Env {
    pub saaio_home: Option<PathBuf>,
    pub saaio_config: Option<PathBuf>,
    pub flow_id: Option<String>,
    /// $SAAIO_MASTER is 1: the master's pane, whose hooks report no flow.
    pub saaio_master: bool,
    pub home: Option<PathBuf>,
    pub xdg_state_home: Option<PathBuf>,
    pub xdg_config_home: Option<PathBuf>,
    pub claude_config_dir: Option<PathBuf>,
    /// stdin and stdout are both terminals.
    pub terminal: bool,
    /// $CLAUDECODE or $CODEX_THREAD_ID is set: an agent's shell tool runs flow.
    pub agent_shell: bool,
    /// $SSH_CONNECTION or $SSH_TTY is set.
    pub ssh: bool,
    /// $WAYLAND_DISPLAY / $DISPLAY are set.
    pub wayland: bool,
    pub x11: bool,
}

impl Env {
    /// Where the UI copies selected text to.
    pub(crate) fn clipboard(&self) -> Clipboard {
        Clipboard::new(self.ssh, self.wayland, self.x11)
    }

    /// The store: $SAAIO_HOME, else the project store of the repository `cwd` is in, else the
    /// shared default store; Ok(None) when neither $SAAIO_HOME, $XDG_STATE_HOME nor $HOME is set.
    pub(crate) fn store(&self, cwd: Option<&Path>) -> anyhow::Result<Option<Store>> {
        if let Some(home) = self
            .saaio_home
            .as_ref()
            .filter(|home| !home.as_os_str().is_empty())
        {
            return Ok(Some(Store::at(home.clone())));
        }
        let Some(registry) = self.registry() else {
            return Ok(None);
        };
        match cwd.and_then(workspace::git::common_dir) {
            Some(git_dir) => registry.project_store(&git_dir).map(Some),
            None => Ok(Some(Store::at(registry.root().to_path_buf()))),
        }
    }

    /// The shared folder every store registers in, from $XDG_STATE_HOME or $HOME.
    pub(crate) fn registry(&self) -> Option<Registry> {
        Registry::locate(self.xdg_state_home.clone(), self.home.clone())
    }

    /// The config in the file config_file() names.
    pub(crate) fn config(&self) -> anyhow::Result<Config> {
        Config::read(self.config_file().as_deref())
    }

    /// The config file $SAAIO_CONFIG, $XDG_CONFIG_HOME or $HOME point to.
    pub(crate) fn config_file(&self) -> Option<PathBuf> {
        config::path(
            self.saaio_config.clone(),
            self.xdg_config_home.clone(),
            self.home.clone(),
        )
    }

    /// The folder of the config file, where master.md and theme.toml live.
    pub(crate) fn config_dir(&self) -> Option<PathBuf> {
        self.config_file()
            .and_then(|file| file.parent().map(Path::to_path_buf))
    }

    /// Claude Code's home, for the peek: $CLAUDE_CONFIG_DIR, else ~/.claude.
    pub(crate) fn claude_home(&self) -> PathBuf {
        self.claude_config_dir
            .clone()
            .filter(|dir| !dir.as_os_str().is_empty())
            .or_else(|| self.home.as_ref().map(|home| home.join(".claude")))
            .unwrap_or_else(|| PathBuf::from(".claude"))
    }

    /// $HOME, unless it is empty.
    pub(crate) fn user_home(&self) -> Option<&Path> {
        self.home
            .as_deref()
            .filter(|home| !home.as_os_str().is_empty())
    }

    /// True in the master's session: its hooks record the master, not a flow.
    pub(crate) fn is_master(&self) -> bool {
        self.saaio_master
    }

    /// $SAAIO_FLOW_ID, unless it is empty: set in a worker's session, not in the master's.
    pub(crate) fn flow_id(&self) -> Option<&str> {
        self.flow_id.as_deref().filter(|id| !id.is_empty())
    }

    /// True when a person runs flow at a terminal: not in a flow's session, the master's, or an
    /// agent's shell.
    pub(crate) fn is_person(&self) -> bool {
        self.terminal && !self.agent_shell && !self.saaio_master && self.flow_id.is_none()
    }

    /// This binary, with ~ for the user's home.
    pub(crate) fn bin_path(&self) -> String {
        let Ok(exe) = env::current_exe() else {
            return "saaio".to_string();
        };
        match self
            .user_home()
            .and_then(|home| exe.strip_prefix(home).ok())
        {
            Some(rest) => format!("~/{}", rest.display()),
            None => exe.display().to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::process::Command;

    use tempfile::TempDir;

    use super::*;

    fn repo(dir: &TempDir, name: &str) -> PathBuf {
        let repo = dir.path().canonicalize().unwrap().join(name);
        fs::create_dir_all(repo.join("src")).unwrap();
        let status = Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["init", "-q"])
            .status()
            .unwrap();
        assert!(status.success());
        repo
    }

    fn root(env: &Env, cwd: &Path) -> PathBuf {
        env.store(Some(cwd)).unwrap().unwrap().root().to_path_buf()
    }

    #[test]
    fn finds_the_store_in_saaio_home_first() {
        let dir = TempDir::new().unwrap();
        let shop = repo(&dir, "shop");
        let env = Env {
            saaio_home: Some(PathBuf::from("/flows")),
            home: Some(PathBuf::from("/home/jan")),
            ..Env::default()
        };

        assert_eq!(root(&env, &shop), Path::new("/flows"));
        assert!(Env::default().store(Some(&shop)).unwrap().is_none());
    }

    #[test]
    fn finds_the_project_store_of_the_repository() {
        let dir = TempDir::new().unwrap();
        let shop = repo(&dir, "shop");
        let env = Env {
            saaio_home: Some(PathBuf::new()),
            xdg_state_home: Some(dir.path().join("state")),
            ..Env::default()
        };

        assert_eq!(
            root(&env, &shop.join("src")),
            dir.path().join("state/saaio/projects/shop")
        );
    }

    #[test]
    fn finds_the_shared_store_outside_a_repository() {
        let dir = TempDir::new().unwrap();
        let outside = dir.path().join("notes");
        fs::create_dir_all(&outside).unwrap();
        let xdg = Env {
            xdg_state_home: Some(PathBuf::from("/state")),
            home: Some(PathBuf::from("/home/jan")),
            ..Env::default()
        };
        let home = Env {
            xdg_state_home: Some(PathBuf::new()),
            ..xdg.clone()
        };

        assert_eq!(root(&xdg, &outside), Path::new("/state/saaio"));
        assert_eq!(
            root(&home, &outside),
            Path::new("/home/jan/.local/state/saaio")
        );
        assert_eq!(
            xdg.store(None).unwrap().unwrap().root(),
            Path::new("/state/saaio")
        );
    }

    #[test]
    fn finds_the_config_folder_from_the_config_file() {
        let env = Env {
            saaio_config: Some(PathBuf::from("/etc/saaio/config.toml")),
            ..Env::default()
        };

        assert_eq!(env.config_dir(), Some(PathBuf::from("/etc/saaio")));
    }

    #[test]
    fn finds_claude_in_its_config_dir_else_in_home() {
        let home = Env {
            home: Some(PathBuf::from("/home/jan")),
            claude_config_dir: Some(PathBuf::new()),
            ..Env::default()
        };
        let set = Env {
            claude_config_dir: Some(PathBuf::from("/claude")),
            ..home.clone()
        };

        assert_eq!(home.claude_home(), PathBuf::from("/home/jan/.claude"));
        assert_eq!(set.claude_home(), PathBuf::from("/claude"));
        assert_eq!(Env::default().claude_home(), PathBuf::from(".claude"));
    }

    #[test]
    fn an_empty_flow_id_is_none() {
        let empty = Env {
            flow_id: Some(String::new()),
            ..Env::default()
        };
        let set = Env {
            flow_id: Some("7f3c".to_string()),
            ..Env::default()
        };

        assert_eq!(empty.flow_id(), None);
        assert_eq!(set.flow_id(), Some("7f3c"));
    }

    #[test]
    fn a_person_has_a_terminal_and_no_session_marker() {
        let person = Env {
            terminal: true,
            ..Env::default()
        };
        let piped = Env::default();
        let agent = Env {
            agent_shell: true,
            ..person.clone()
        };
        let master = Env {
            saaio_master: true,
            ..person.clone()
        };
        let worker = Env {
            flow_id: Some("7f3c".to_string()),
            ..person.clone()
        };
        let empty_id = Env {
            flow_id: Some(String::new()),
            ..person.clone()
        };

        assert!(person.is_person());
        for env in [piped, agent, master, worker, empty_id] {
            assert!(!env.is_person(), "{env:?}");
        }
    }

    #[test]
    fn writes_the_binary_with_a_tilde_only_inside_home() {
        let exe = env::current_exe().unwrap();
        let folder = exe.parent().unwrap();
        let inside = Env {
            home: Some(folder.to_path_buf()),
            ..Env::default()
        };
        let empty = Env {
            home: Some(PathBuf::new()),
            ..Env::default()
        };

        let name = exe.file_name().unwrap().to_str().unwrap();
        assert_eq!(inside.bin_path(), format!("~/{name}"));
        assert_eq!(empty.bin_path(), exe.display().to_string());
    }
}
