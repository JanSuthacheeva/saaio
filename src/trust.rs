//! `Trust`: the repositories whose hooks saaio runs and whose worktrees Codex trusts

use std::ffi::OsStr;
use std::fs;
use std::io::ErrorKind;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::private::{self, write_atomic};
use crate::registry::{Registry, fnv1a};

/// The repositories the person trusts, one file each in the shared folder: trusted/<hash of the
/// path> holding the path, so every store and project sees the same answer.
#[derive(Debug, Clone)]
pub struct Trust {
    root: PathBuf,
}

impl Trust {
    pub fn at(registry: &Registry) -> Trust {
        Trust {
            root: registry.root().join("trusted"),
        }
    }

    pub fn covers(&self, repo: &Path) -> bool {
        fs::read(self.entry(repo)).is_ok_and(|line| line == line_of(repo))
    }

    pub fn grant(&self, repo: &Path) -> Result<()> {
        private::create_dir(&self.root)
            .with_context(|| format!("could not create {}", self.root.display()))?;
        write_atomic(&self.entry(repo), &line_of(repo))
            .with_context(|| format!("could not trust {}", repo.display()))
    }

    /// Ok(false) when it was not trusted.
    pub fn revoke(&self, repo: &Path) -> Result<bool> {
        if !self.covers(repo) {
            return Ok(false);
        }
        match fs::remove_file(self.entry(repo)) {
            Ok(()) => Ok(true),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
            Err(error) => {
                Err(error).with_context(|| format!("could not revoke {}", repo.display()))
            }
        }
    }

    /// Every trusted repository, sorted.
    pub fn repos(&self) -> Vec<PathBuf> {
        let Ok(entries) = fs::read_dir(&self.root) else {
            return Vec::new();
        };
        let mut repos: Vec<PathBuf> = entries
            .flatten()
            .filter(|entry| !entry.file_name().to_string_lossy().contains('.'))
            .filter_map(|entry| {
                let line = fs::read(entry.path()).ok()?;
                Some(PathBuf::from(OsStr::from_bytes(line.strip_suffix(b"\n")?)))
            })
            .collect();
        repos.sort();
        repos
    }

    fn entry(&self, repo: &Path) -> PathBuf {
        let hash = fnv1a(repo.as_os_str().as_bytes());
        self.root.join(format!("{hash:016x}"))
    }
}

fn line_of(repo: &Path) -> Vec<u8> {
    let mut line = repo.as_os_str().as_bytes().to_vec();
    line.push(b'\n');
    line
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    #[test]
    fn grants_and_revokes_one_repository_at_a_time() {
        let dir = TempDir::new().unwrap();
        let trust = Trust::at(&Registry::at(dir.path().to_path_buf()));
        let shop = Path::new("/Users/jan/projects/shop");
        let blog = Path::new("/Users/jan/projects/blog");

        assert!(!trust.covers(shop));
        trust.grant(shop).unwrap();
        trust.grant(blog).unwrap();
        trust.grant(shop).unwrap();

        assert!(trust.covers(shop));
        assert!(!trust.covers(Path::new("/Users/jan/projects/shop/sub")));
        assert_eq!(trust.repos(), [blog, shop]);
        assert!(trust.revoke(shop).unwrap());
        assert!(!trust.revoke(shop).unwrap());
        assert!(!trust.covers(shop));
        assert_eq!(trust.repos(), [blog]);
    }

    #[test]
    fn nothing_is_trusted_before_the_folder_exists() {
        let dir = TempDir::new().unwrap();
        let trust = Trust::at(&Registry::at(dir.path().join("missing")));

        assert!(!trust.covers(Path::new("/shop")));
        assert_eq!(trust.repos(), [] as [PathBuf; 0]);
        assert!(!trust.revoke(Path::new("/shop")).unwrap());
    }
}
