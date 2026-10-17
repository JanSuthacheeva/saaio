use std::fs;
use std::io::ErrorKind;
use std::path::Path;

use anyhow::{Context, Result};

/// A file flow writes with its default text: when it is missing, or still holds an earlier default
/// nobody edited. An edited file is never overwritten.
#[derive(Debug, Clone, Copy)]
pub struct DefaultFile {
    pub text: &'static str,
    /// Every earlier default, oldest first.
    pub earlier: &'static [&'static str],
}

impl DefaultFile {
    /// Writes `text` to `path` when it is missing or an earlier default, creating its folder.
    pub fn ensure(&self, path: &Path) -> Result<()> {
        match fs::read_to_string(path) {
            Ok(text) if text == self.text || !self.earlier.contains(&text.as_str()) => {
                return Ok(());
            }
            Ok(_) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => {
                return Err(error).with_context(|| format!("could not read {}", path.display()));
            }
        }
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)
                .with_context(|| format!("could not create {}", dir.display()))?;
        }
        fs::write(path, self.text).with_context(|| format!("could not write {}", path.display()))
    }
}

/// An example's settings uncommented: every `#key` and `#[section]` line loses its `#`.
#[cfg(test)]
pub fn uncommented(text: &str) -> String {
    text.lines()
        .map(|line| match line.strip_prefix('#') {
            Some(rest) if rest.starts_with(|c: char| c != ' ' && c != '#') => rest,
            _ => line,
        })
        .fold(String::new(), |mut out, line| {
            out.push_str(line);
            out.push('\n');
            out
        })
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    const FILE: DefaultFile = DefaultFile {
        text: "new",
        earlier: &["first", "second"],
    };

    #[test]
    fn writes_a_missing_file_and_its_folder() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("saaio/file");

        FILE.ensure(&path).unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), "new");
    }

    #[test]
    fn replaces_an_earlier_default_nobody_edited() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("file");
        for earlier in FILE.earlier {
            fs::write(&path, earlier).unwrap();

            FILE.ensure(&path).unwrap();

            assert_eq!(fs::read_to_string(&path).unwrap(), "new");
        }
    }

    #[test]
    fn keeps_an_edited_file() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("file");
        fs::write(&path, "mine").unwrap();

        FILE.ensure(&path).unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), "mine");
    }

    #[test]
    fn names_the_file_it_cannot_read() {
        let dir = TempDir::new().unwrap();

        let error = FILE.ensure(dir.path()).unwrap_err();

        assert!(
            format!("{error:#}").contains(&dir.path().display().to_string()),
            "{error:#}"
        );
    }

    #[test]
    fn uncomments_settings_and_sections_but_not_explanations() {
        let text = "# A theme.\n#extends = \"saaio\"\n\n#[ui]\n## heading\n# For example: x = 1\n#[[phases]]\nkey = 2\n";

        assert_eq!(
            uncommented(text),
            "# A theme.\nextends = \"saaio\"\n\n[ui]\n## heading\n# For example: x = 1\n[[phases]]\nkey = 2\n"
        );
    }
}
