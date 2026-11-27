//! `ClaudeSkills`: whether Claude Code can call a skill or command by name

use std::fs;
use std::path::{Path, PathBuf};

/// Where Claude Code finds skills by name: its home and the repository.
#[derive(Debug)]
pub struct ClaudeSkills {
    home: PathBuf,
    repo: PathBuf,
}

impl ClaudeSkills {
    pub fn new(home: PathBuf, repo: PathBuf) -> ClaudeSkills {
        ClaudeSkills { home, repo }
    }

    /// True when `/name` calls a skill or command Claude Code has: in its home, the repository's
    /// `.claude` (a worktree inside the repository finds it there too), or any installed plugin
    /// version, which Claude Code calls without its namespace. A skill answers to its folder's
    /// name and to the `name` its frontmatter gives.
    pub fn has(&self, name: &str) -> bool {
        let mut roots = vec![self.home.clone(), self.repo.join(".claude")];
        roots.extend(plugin_versions(&self.home.join("plugins").join("cache")));
        roots.iter().any(|root| defines(root, name))
    }
}

/// Whether `root` holds `skills/<name>/SKILL.md`, a skill whose frontmatter names it, or
/// `commands/<name>.md`.
fn defines(root: &Path, name: &str) -> bool {
    let skills = root.join("skills");
    skills.join(name).join("SKILL.md").is_file()
        || root.join("commands").join(format!("{name}.md")).is_file()
        || subdirs(&skills).iter().any(|dir| {
            fs::read_to_string(dir.join("SKILL.md"))
                .is_ok_and(|text| frontmatter_name(&text) == Some(name))
        })
}

/// The `name:` of a SKILL.md's frontmatter, quotes taken off.
fn frontmatter_name(text: &str) -> Option<&str> {
    let mut lines = text.lines();
    if lines.next()?.trim() != "---" {
        return None;
    }
    lines
        .take_while(|line| line.trim() != "---")
        .find_map(|line| line.strip_prefix("name:"))
        .map(|value| value.trim().trim_matches(['"', '\'']))
}

/// The plugin folders in Claude Code's cache: `<marketplace>/<plugin>/<version>`.
fn plugin_versions(cache: &Path) -> Vec<PathBuf> {
    (0..3).fold(vec![cache.to_path_buf()], |dirs, _| {
        dirs.iter().flat_map(|dir| subdirs(dir)).collect()
    })
}

fn subdirs(dir: &Path) -> Vec<PathBuf> {
    fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn touch(path: PathBuf) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "---\nname: x\n---\n").unwrap();
    }

    fn skills(dir: &TempDir) -> ClaudeSkills {
        ClaudeSkills::new(dir.path().join("home"), dir.path().join("repo"))
    }

    #[test]
    fn finds_a_skill_in_the_home() {
        let dir = TempDir::new().unwrap();
        touch(dir.path().join("home/skills/ship-fix/SKILL.md"));

        assert!(skills(&dir).has("ship-fix"));
    }

    #[test]
    fn finds_a_command_in_the_home() {
        let dir = TempDir::new().unwrap();
        touch(dir.path().join("home/commands/ship-fix.md"));

        assert!(skills(&dir).has("ship-fix"));
    }

    #[test]
    fn finds_a_skill_or_command_in_the_repository() {
        let dir = TempDir::new().unwrap();
        touch(dir.path().join("repo/.claude/skills/ship-fix/SKILL.md"));
        touch(dir.path().join("repo/.claude/commands/ship-feature.md"));

        assert!(skills(&dir).has("ship-fix"));
        assert!(skills(&dir).has("ship-feature"));
    }

    #[test]
    fn finds_a_plugins_skill_or_command_in_any_version() {
        let dir = TempDir::new().unwrap();
        let cache = dir.path().join("home/plugins/cache/market");
        touch(cache.join("superpowers/5.0.1/skills/brainstorming/SKILL.md"));
        touch(cache.join("tools/1.2.0/commands/ship-fix.md"));

        assert!(skills(&dir).has("brainstorming"));
        assert!(skills(&dir).has("ship-fix"));
    }

    #[test]
    fn finds_a_skill_by_the_name_its_frontmatter_gives() {
        let dir = TempDir::new().unwrap();
        let skill = dir.path().join("repo/.claude/skills/folder-aaa/SKILL.md");
        fs::create_dir_all(skill.parent().unwrap()).unwrap();
        fs::write(
            &skill,
            "---\ndescription: x\nname: \"frontmatter-bbb\"\n---\n\nname: body-ccc\n",
        )
        .unwrap();

        assert!(skills(&dir).has("frontmatter-bbb"));
        assert!(skills(&dir).has("folder-aaa"));
        assert!(!skills(&dir).has("body-ccc"));
    }

    #[test]
    fn a_name_found_nowhere_is_missing() {
        let dir = TempDir::new().unwrap();
        touch(dir.path().join("home/skills/ship-fix/SKILL.md"));
        touch(dir.path().join("home/skills/ship-feature/notes.md"));

        assert!(!skills(&dir).has("ship-feature"));
        assert!(!skills(&dir).has("no-such-skill"));
    }
}
