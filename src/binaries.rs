//! The saaio binaries that can disagree: this one, the first on PATH, a server's; `Mismatch`

use std::env;
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::system::{self, Probe};

/// This binary: `0.1.0 (2904734)`, the commit from build.rs, `unknown` outside a git checkout.
pub const BUILD: &str = concat!(env!("CARGO_PKG_VERSION"), " (", env!("SAAIO_COMMIT"), ")");
/// How long the saaio first on PATH gets to print its version.
const DEADLINE: Duration = Duration::from_secs(1);
/// The build of a saaio whose `--version` said nothing usable.
const UNKNOWN: &str = "unknown";

/// A saaio executable and the build it is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Binary {
    pub exe: PathBuf,
    /// BUILD's form: `0.1.2 (45630ee)`; `unknown` when --version said nothing usable.
    pub build: String,
}

impl Binary {
    /// This process: current_exe and BUILD.
    pub fn this() -> io::Result<Binary> {
        Ok(Binary {
            exe: env::current_exe()?,
            build: BUILD.to_string(),
        })
    }

    /// The version part of the build: `0.1.2`.
    pub fn version(&self) -> &str {
        self.build.split(' ').next().unwrap_or_default()
    }

    /// Whether `other` is the same build; a build without its commit (`0.2.0`) matches on the
    /// version.
    fn same_build(&self, other: &Binary) -> bool {
        let bare = |build: &str| !build.contains(' ');
        self.build == other.build
            || ((bare(&self.build) || bare(&other.build))
                && self.build != UNKNOWN
                && other.build != UNKNOWN
                && self.version() == other.version())
    }
}

/// How the saaio binaries disagree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mismatch {
    /// Another saaio of another build comes first on PATH.
    Shadowed(Binary),
    /// The running server is another build.
    Server(Binary),
}

/// The first executable file named saaio in `path`'s folders; empty and relative entries skipped.
pub fn first_on_path(path: &OsStr) -> Option<PathBuf> {
    env::split_paths(path)
        .filter(|dir| dir.is_absolute())
        .map(|dir| dir.join("saaio"))
        .find(|candidate| is_executable(candidate))
}

/// The first saaio on PATH when it is another file than `this` (canonicalised) and its
/// `--version` (1 s) names another build or none.
pub fn shadowing(this: &Binary, path: &OsStr) -> Option<Binary> {
    shadowing_within(this, path, DEADLINE)
}

/// `shadowing` with `limit` for `--version`: the first exec of a fresh script can take a while.
fn shadowing_within(this: &Binary, path: &OsStr, limit: Duration) -> Option<Binary> {
    let first = first_on_path(path)?;
    if canonical(&first) == canonical(&this.exe) {
        return None;
    }
    let build = match system::version(&first, Instant::now() + limit) {
        Probe::Version(build) => build,
        Probe::Missing | Probe::Failed(_) => UNKNOWN.to_string(),
    };
    let other = Binary { exe: first, build };
    (!this.same_build(&other)).then_some(other)
}

/// `shadowed` (what `shadowing` found) first, then Server when `server`'s build is not `this`'s.
pub fn mismatches(
    this: &Binary,
    server: Option<&Binary>,
    shadowed: Option<Binary>,
) -> Vec<Mismatch> {
    let shadowed = shadowed.map(Mismatch::Shadowed);
    let server = server
        .filter(|server| server.build != this.build)
        .map(|server| Mismatch::Server(server.clone()));
    shadowed.into_iter().chain(server).collect()
}

fn is_executable(path: &Path) -> bool {
    fs::metadata(path).is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}

fn canonical(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::os::unix::fs::symlink;

    use tempfile::TempDir;

    use super::*;
    use crate::fixtures::executable;

    const THIS: &str = "0.1.2 (45630ee)";

    /// A folder holding `saaio`, a script whose `--version` runs `body`.
    fn saaio(root: &Path, folder: &str, body: &str) -> PathBuf {
        let dir = root.join(folder);
        fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("saaio");
        executable(&exe, &format!("#!/bin/sh\n{body}\n"));
        exe
    }

    fn this(root: &Path) -> Binary {
        Binary {
            exe: saaio(root, "local", &format!("echo 'saaio {THIS}'")),
            build: THIS.to_string(),
        }
    }

    fn shadowed(this: &Binary, path: &OsStr) -> Option<Binary> {
        shadowing_within(this, path, Duration::from_secs(30))
    }

    fn path(dirs: &[PathBuf]) -> OsString {
        env::join_paths(dirs).unwrap()
    }

    #[test]
    fn the_version_is_the_build_before_its_commit() {
        let binary = Binary {
            exe: PathBuf::from("/x/saaio"),
            build: THIS.to_string(),
        };
        assert_eq!(binary.version(), "0.1.2");
    }

    #[test]
    fn finds_the_first_executable_saaio_skipping_empty_and_relative_folders() {
        let root = TempDir::new().unwrap();
        let plain = root.path().join("plain");
        fs::create_dir_all(&plain).unwrap();
        fs::write(plain.join("saaio"), "not executable").unwrap();
        let first = saaio(root.path(), "first", "true");
        saaio(root.path(), "second", "true");
        let found = first_on_path(&path(&[
            PathBuf::new(),
            PathBuf::from("first"),
            root.path().join("missing"),
            plain,
            root.path().join("first"),
            root.path().join("second"),
        ]));
        assert_eq!(found, Some(first));
        assert_eq!(first_on_path(OsStr::new("")), None);
    }

    #[test]
    fn this_binary_first_on_path_is_no_shadow() {
        let root = TempDir::new().unwrap();
        let this = this(root.path());
        assert_eq!(
            shadowed(&this, &path(&[this.exe.parent().unwrap().into()])),
            None
        );
    }

    #[test]
    fn a_link_to_this_binary_is_no_shadow() {
        let root = TempDir::new().unwrap();
        let this = this(root.path());
        let bin = root.path().join("bin");
        fs::create_dir_all(&bin).unwrap();
        symlink(&this.exe, bin.join("saaio")).unwrap();
        assert_eq!(shadowed(&this, &path(&[bin])), None);
    }

    #[test]
    fn another_file_of_the_same_build_is_no_shadow() {
        let root = TempDir::new().unwrap();
        let this = this(root.path());
        let other = saaio(root.path(), "cargo", &format!("echo 'saaio {THIS}'"));
        assert_eq!(
            shadowed(&this, &path(&[other.parent().unwrap().into()])),
            None
        );
    }

    #[test]
    fn another_build_first_on_path_shadows() {
        let root = TempDir::new().unwrap();
        let this = this(root.path());
        let other = saaio(root.path(), "cargo", "echo 'saaio 0.1.0 (9b77d8c)'");
        let dirs = [
            other.parent().unwrap().into(),
            this.exe.parent().unwrap().into(),
        ];
        assert_eq!(
            shadowed(&this, &path(&dirs)),
            Some(Binary {
                exe: other,
                build: "0.1.0 (9b77d8c)".to_string()
            })
        );
    }

    #[test]
    fn a_saaio_without_a_version_shadows_as_unknown() {
        let root = TempDir::new().unwrap();
        let this = this(root.path());
        let other = saaio(root.path(), "cargo", "exit 1");
        assert_eq!(
            shadowed(&this, &path(&[other.parent().unwrap().into()])),
            Some(Binary {
                exe: other,
                build: UNKNOWN.to_string()
            })
        );
    }

    #[test]
    fn a_build_without_its_commit_matches_on_the_version() {
        let root = TempDir::new().unwrap();
        let target = Binary {
            exe: root.path().join("local/saaio"),
            build: "0.1.2".to_string(),
        };
        let same = saaio(root.path(), "same", &format!("echo 'saaio {THIS}'"));
        assert_eq!(
            shadowed(&target, &path(&[same.parent().unwrap().into()])),
            None
        );
        let older = saaio(root.path(), "older", "echo 'saaio 0.1.0 (9b77d8c)'");
        assert!(shadowed(&target, &path(&[older.parent().unwrap().into()])).is_some());
    }

    #[test]
    fn mismatches_name_the_shadow_first_then_the_server() {
        let root = TempDir::new().unwrap();
        let this = this(root.path());
        let first = Binary {
            exe: "/cargo/saaio".into(),
            build: "0.1.0 (9b77d8c)".to_string(),
        };
        let server = Binary {
            exe: "/cargo/saaio".into(),
            build: "0.1.1 (6ded49e)".to_string(),
        };
        let found = mismatches(&this, Some(&server), Some(first.clone()));
        assert_eq!(
            found,
            vec![Mismatch::Shadowed(first), Mismatch::Server(server)]
        );
        assert_eq!(mismatches(&this, Some(&this.clone()), None), vec![]);
        assert_eq!(mismatches(&this, None, None), vec![]);
    }
}
