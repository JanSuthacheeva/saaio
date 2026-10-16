use std::env;
use std::path::{Path, PathBuf};

use crate::config::{self, Config};
use crate::registry::Registry;
use crate::store::Store;

/// The environment variables flow reads, read once by main so commands get them as values.
#[derive(Debug, Clone, Default)]
pub struct Env {
    pub flow_home: Option<PathBuf>,
    pub flow_config: Option<PathBuf>,
    pub flow_id: Option<String>,
    pub home: Option<PathBuf>,
    pub xdg_state_home: Option<PathBuf>,
    pub xdg_config_home: Option<PathBuf>,
    pub claude_config_dir: Option<PathBuf>,
}

impl Env {
    /// The store $FLOW_HOME, $XDG_STATE_HOME or $HOME point to.
    pub(crate) fn store(&self) -> Option<Store> {
        Store::locate(
            self.flow_home.clone(),
            self.xdg_state_home.clone(),
            self.home.clone(),
        )
    }

    /// The shared folder every store registers in, from $XDG_STATE_HOME or $HOME.
    pub(crate) fn registry(&self) -> Option<Registry> {
        Registry::locate(self.xdg_state_home.clone(), self.home.clone())
    }

    /// The config file $FLOW_CONFIG, $XDG_CONFIG_HOME or $HOME point to.
    pub(crate) fn config(&self) -> anyhow::Result<Config> {
        Config::load(
            self.flow_config.clone(),
            self.xdg_config_home.clone(),
            self.home.clone(),
        )
    }

    /// The folder of the config file, where master.md and theme.toml live.
    pub(crate) fn config_dir(&self) -> Option<PathBuf> {
        config::config_file(
            self.flow_config.clone(),
            self.xdg_config_home.clone(),
            self.home.clone(),
        )
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

    /// $FLOW_ID, unless it is empty: set in a worker's session, not in the master's.
    pub(crate) fn flow_id(&self) -> Option<&str> {
        self.flow_id.as_deref().filter(|id| !id.is_empty())
    }

    /// This binary, with ~ for the user's home.
    pub(crate) fn bin_path(&self) -> String {
        let Ok(exe) = env::current_exe() else {
            return "flow".to_string();
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
    use super::*;

    #[test]
    fn finds_the_store_in_flow_home_first() {
        let env = Env {
            flow_home: Some(PathBuf::from("/flows")),
            home: Some(PathBuf::from("/home/jan")),
            ..Env::default()
        };

        assert_eq!(env.store().unwrap().root(), Path::new("/flows"));
        assert!(Env::default().store().is_none());
    }

    #[test]
    fn finds_the_config_folder_from_the_config_file() {
        let env = Env {
            flow_config: Some(PathBuf::from("/etc/flow/config.toml")),
            ..Env::default()
        };

        assert_eq!(env.config_dir(), Some(PathBuf::from("/etc/flow")));
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
