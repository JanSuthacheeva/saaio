//! `RepoConfig`: a repository's `.saaio/config.toml`

use std::fs;
use std::io::ErrorKind;
use std::path::Path;

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::branching::Branching;
use crate::exclusive::Exclusive;

pub const CONFIG_FILE: &str = ".saaio/config.toml";

/// What a repository says about its flows, from `.saaio/config.toml`.
#[derive(Debug, Default, Deserialize)]
pub struct RepoConfig {
    /// `[branch]`: the branch convention.
    pub branch: Option<Branching>,
    /// `[exclusive]`: the heavy commands, one run at a time across the project.
    pub exclusive: Option<Exclusive>,
}

impl RepoConfig {
    /// <repo>/.saaio/config.toml; the default without the file; Err when it cannot be read or
    /// parsed.
    pub fn load(repo: &Path) -> Result<RepoConfig> {
        let path = repo.join(CONFIG_FILE);
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(RepoConfig::default()),
            Err(error) => {
                return Err(error).with_context(|| format!("could not read {}", path.display()));
            }
        };
        toml::from_str(&text).with_context(|| format!("{} is not valid", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    fn write(dir: &TempDir, text: &str) {
        fs::create_dir_all(dir.path().join(".saaio")).unwrap();
        fs::write(dir.path().join(CONFIG_FILE), text).unwrap();
    }

    #[test]
    fn is_empty_without_the_file() {
        let dir = TempDir::new().unwrap();
        let config = RepoConfig::load(dir.path()).unwrap();
        assert_eq!(config.branch, None);
        assert_eq!(config.exclusive, None);
    }

    #[test]
    fn loads_the_branch_section() {
        let dir = TempDir::new().unwrap();
        write(
            &dir,
            "[branch]\nname = \"{type}/{ticket}-{slug}\"\ntypes = [\"feature\", \"bugfix\"]\n",
        );
        let branch = RepoConfig::load(dir.path()).unwrap().branch.unwrap();
        assert_eq!(branch.name, "{type}/{ticket}-{slug}");
        assert_eq!(branch.types, ["feature", "bugfix"]);
    }

    #[test]
    fn loads_the_exclusive_section_beside_the_branch() {
        let dir = TempDir::new().unwrap();
        write(
            &dir,
            "[branch]\nname = \"{name}\"\n\n[exclusive]\ncommands = [\"artisan test\", \"pint\"]\n",
        );
        let config = RepoConfig::load(dir.path()).unwrap();
        assert_eq!(config.branch.unwrap().name, "{name}");
        assert_eq!(config.exclusive.unwrap().commands, ["artisan test", "pint"]);
    }

    #[test]
    fn refuses_an_empty_command() {
        let dir = TempDir::new().unwrap();
        write(&dir, "[exclusive]\ncommands = [\"pint\", \"\"]\n");
        let error = format!("{:#}", RepoConfig::load(dir.path()).unwrap_err());
        assert!(error.contains("a command needs a word"), "{error}");
    }

    #[test]
    fn ignores_other_sections() {
        let dir = TempDir::new().unwrap();
        write(&dir, "[other]\nx = 1\n");
        assert_eq!(RepoConfig::load(dir.path()).unwrap().branch, None);
    }

    #[test]
    fn names_the_bad_key_and_the_file() {
        let dir = TempDir::new().unwrap();
        write(&dir, "[branch]\nname = \"x\"\ntipes = []\n");
        let error = format!("{:#}", RepoConfig::load(dir.path()).unwrap_err());
        assert!(error.contains("tipes"), "{error}");
        assert!(error.contains(CONFIG_FILE), "{error}");
    }
}
