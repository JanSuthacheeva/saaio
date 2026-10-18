use std::path::PathBuf;

use anyhow::Result;

use crate::config::Config;
use crate::store::Stamp;

/// Knows the config file by size and time; a missing file counts too.
#[derive(Debug)]
pub struct ConfigWatch {
    file: Option<PathBuf>,
    stamp: Option<Option<Stamp>>,
}

impl ConfigWatch {
    pub fn new(file: Option<PathBuf>) -> ConfigWatch {
        ConfigWatch { file, stamp: None }
    }

    /// The config again when the file appeared, changed or went away; always on the first call.
    pub fn changed(&mut self) -> Option<Result<Config>> {
        let stamp = self.file.as_deref().and_then(Stamp::of);
        if self.stamp == Some(stamp) {
            return None;
        }
        self.stamp = Some(stamp);
        Some(Config::read(self.file.as_deref()))
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;

    #[test]
    fn reads_the_file_again_once_it_changed() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("config.toml");
        let mut watch = ConfigWatch::new(Some(file.clone()));

        let first = watch.changed().expect("the first look reads");
        assert_eq!(first.unwrap(), Config::builtin());
        assert!(watch.changed().is_none());

        fs::write(&file, "[worker]\nagent = \"codex\"\n").unwrap();
        assert_eq!(watch.changed().unwrap().unwrap().worker.agent, "codex");
        assert!(watch.changed().is_none());

        // A new length, so the change shows on filesystems whose mtimes are a second coarse.
        fs::write(&file, "[worker]\nagnet = \"codex\"\n").unwrap();
        let broken = format!("{:#}", watch.changed().unwrap().unwrap_err());
        assert!(
            broken.contains("config.toml:2: unknown field `agnet`"),
            "{broken}"
        );
        assert!(watch.changed().is_none(), "a broken file is said once");

        fs::write(&file, "[ui]\nprefix = \"C-b\"\n").unwrap();
        assert_eq!(watch.changed().unwrap().unwrap().ui.prefix.name(), "C-b");

        fs::remove_file(&file).unwrap();
        assert_eq!(watch.changed().unwrap().unwrap(), Config::builtin());
    }

    #[test]
    fn reads_the_built_in_config_once_without_a_file() {
        let mut watch = ConfigWatch::new(None);

        assert_eq!(watch.changed().unwrap().unwrap(), Config::builtin());
        assert!(watch.changed().is_none());
    }
}
