use std::fs::{self, DirBuilder, OpenOptions, Permissions};
use std::io::{self, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::Path;

/// What every folder saaio creates for the user alone is made with.
const DIR_MODE: u32 = 0o700;

/// What every file saaio creates for the user alone is made with.
const FILE_MODE: u32 = 0o600;

/// Creates `dir` and its missing parents 0700; makes `dir` itself 0700 when it was already there.
pub fn create_dir(dir: &Path) -> io::Result<()> {
    DirBuilder::new()
        .recursive(true)
        .mode(DIR_MODE)
        .create(dir)?;
    fs::set_permissions(dir, Permissions::from_mode(DIR_MODE))
}

/// OpenOptions whose create makes the file 0600.
pub fn options() -> OpenOptions {
    let mut options = OpenOptions::new();
    options.mode(FILE_MODE);
    options
}

/// `contents` in a new file 0600 at `path`; Err(AlreadyExists) when anything, a symlink too, is
/// there, so it never writes through one.
pub fn write_new(path: &Path, contents: &[u8]) -> io::Result<()> {
    options()
        .write(true)
        .create_new(true)
        .open(path)?
        .write_all(contents)
}

/// Writes `contents` to a hidden 0600 file beside `path` and renames it over, so a reader never
/// sees half a file.
pub fn write_atomic(path: &Path, contents: &[u8]) -> io::Result<()> {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let partial = path.with_file_name(format!(".{name}.{}", std::process::id()));
    // One left by a crashed process of the same pid, or a link someone put there.
    let _ = fs::remove_file(&partial);
    write_new(&partial, contents)?;
    fs::rename(&partial, path)
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::{PermissionsExt, symlink};

    use tempfile::TempDir;

    use super::*;

    fn mode(path: &Path) -> u32 {
        fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[test]
    fn creates_folders_for_the_user_alone_and_closes_one_already_there() {
        let dir = TempDir::new().unwrap();
        let nested = dir.path().join("state/saaio/flows");
        let open = dir.path().join("open");
        fs::create_dir(&open).unwrap();
        fs::set_permissions(&open, fs::Permissions::from_mode(0o755)).unwrap();

        create_dir(&nested).unwrap();
        create_dir(&nested).unwrap();
        create_dir(&open).unwrap();

        for folder in ["state", "state/saaio", "state/saaio/flows", "open"] {
            assert_eq!(mode(&dir.path().join(folder)), 0o700, "{folder}");
        }
    }

    #[test]
    fn opens_a_new_file_for_the_user_alone() {
        let dir = TempDir::new().unwrap();
        let log = dir.path().join("server.log");

        options().create(true).append(true).open(&log).unwrap();

        assert_eq!(mode(&log), 0o600);
    }

    #[test]
    fn writes_a_new_file_for_the_user_alone() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("report.txt");

        write_new(&file, b"text").unwrap();

        assert_eq!(fs::read_to_string(&file).unwrap(), "text");
        assert_eq!(mode(&file), 0o600);
        let again = write_new(&file, b"other").unwrap_err();
        assert_eq!(again.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read_to_string(&file).unwrap(), "text");
    }

    #[test]
    fn never_writes_through_a_link() {
        let dir = TempDir::new().unwrap();
        let target = dir.path().join("target");
        let link = dir.path().join("link");
        symlink(&target, &link).unwrap();

        let error = write_new(&link, b"text").unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert!(!target.exists(), "a dangling link was followed");
    }

    #[test]
    fn replaces_a_file_whole_for_the_user_alone() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("claim");
        fs::write(&file, "old").unwrap();
        fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).unwrap();
        let target = dir.path().join("target");
        let partial = dir.path().join(format!(".claim.{}", std::process::id()));
        symlink(&target, &partial).unwrap();

        write_atomic(&file, b"new").unwrap();

        assert_eq!(fs::read_to_string(&file).unwrap(), "new");
        assert_eq!(mode(&file), 0o600);
        assert!(!target.exists(), "the partial file's link was followed");
        assert!(!partial.exists());
    }
}
