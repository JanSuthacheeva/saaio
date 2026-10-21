//! The commit a build names: `SAAIO_COMMIT`, else git's in saaio's own checkout, else `unknown`

use std::path::Path;

pub const UNKNOWN: &str = "unknown";

/// `env` is SAAIO_COMMIT, `head` git's short HEAD in saaio's own repository.
pub fn stamp(env: Option<String>, head: Option<String>) -> String {
    env.map(|commit| commit.trim().to_string())
        .filter(|commit| !commit.is_empty())
        .or(head)
        .unwrap_or_else(|| UNKNOWN.to_string())
}

/// Whether `toplevel`, the folder `git rev-parse --show-toplevel` names, is the manifest folder,
/// and not a repository the source was unpacked into.
pub fn is_own(toplevel: &Path, manifest: &Path) -> bool {
    match (toplevel.canonicalize(), manifest.canonicalize()) {
        (Ok(toplevel), Ok(manifest)) => toplevel == manifest,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[expect(
        clippy::unnecessary_wraps,
        reason = "builds the Option the stamp takes"
    )]
    fn some(text: &str) -> Option<String> {
        Some(text.to_string())
    }

    #[test]
    fn the_environment_wins_over_git() {
        assert_eq!(stamp(some(" 1a2b3c4\n"), some("9f8e7d6")), "1a2b3c4");
    }

    #[test]
    fn an_empty_environment_falls_back_to_git() {
        assert_eq!(stamp(some(" "), some("9f8e7d6")), "9f8e7d6");
        assert_eq!(stamp(None, some("9f8e7d6")), "9f8e7d6");
    }

    #[test]
    fn neither_is_unknown() {
        assert_eq!(stamp(None, None), UNKNOWN);
        assert_eq!(stamp(some(""), None), UNKNOWN);
    }

    #[test]
    fn only_the_manifest_folder_is_its_own_repository() {
        let outer = TempDir::new().unwrap();
        let source = outer.path().join("saaio-0.1.0");
        std::fs::create_dir(&source).unwrap();
        assert!(is_own(&source, &source));
        assert!(!is_own(outer.path(), &source));
        assert!(!is_own(
            &outer.path().join("gone"),
            &outer.path().join("gone")
        ));
    }

    #[test]
    fn a_link_to_the_manifest_folder_is_the_same_folder() {
        let outer = TempDir::new().unwrap();
        let source = outer.path().join("source");
        let link = outer.path().join("link");
        std::fs::create_dir(&source).unwrap();
        std::os::unix::fs::symlink(&source, &link).unwrap();
        assert!(is_own(&link, &source));
    }
}
