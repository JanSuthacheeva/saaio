use std::fs;
use std::io::ErrorKind;
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use crate::launch;

/// A repo's branch convention, from `[branch]` in its `.flow/config.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Branching {
    /// The template: {type}, {ticket}, {slug} and {name} are filled in.
    pub name: String,
    /// The types a flow can be; the first is the default.
    #[serde(default)]
    pub types: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct RepoConfig {
    branch: Option<Branching>,
}

pub const CONFIG_FILE: &str = ".flow/config.toml";

impl Branching {
    /// <repo>/.flow/config.toml's [branch]; None without the file or the section.
    pub fn load(repo: &Path) -> Result<Option<Branching>> {
        let path = repo.join(CONFIG_FILE);
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(error).with_context(|| format!("could not read {}", path.display()));
            }
        };
        let config: RepoConfig =
            toml::from_str(&text).with_context(|| format!("{} is not valid", path.display()))?;
        Ok(config.branch)
    }

    /// The type a flow gets: `asked` when the repo lists it, else the first listed; None when the
    /// repo lists no types.
    pub fn pick_type(&self, asked: Option<&str>) -> Result<Option<String>> {
        match asked {
            None => Ok(self.types.first().cloned()),
            Some(kind) if self.types.iter().any(|known| known == kind) => {
                Ok(Some(kind.to_string()))
            }
            Some(kind) if self.types.is_empty() => {
                bail!("{CONFIG_FILE} lists no branch types, so there is no type {kind}")
            }
            Some(kind) => bail!(
                "no branch type {kind}; this repo has {}",
                self.types.join(", ")
            ),
        }
    }
}

/// The branch for a flow: the template filled in, the separators next to an empty placeholder
/// dropped; flow/<name> without a convention.
pub fn branch_name(
    branching: Option<&Branching>,
    kind: Option<&str>,
    request: &str,
    name: &str,
) -> String {
    let Some(branching) = branching else {
        return format!("flow/{name}");
    };
    let filled = branching
        .name
        .replace("{type}", kind.unwrap_or(""))
        .replace("{ticket}", launch::ticket(request).unwrap_or(""))
        .replace("{slug}", &launch::slug(request))
        .replace("{name}", name);
    let tidy: Vec<String> = filled
        .split('/')
        .map(tidy_segment)
        .filter(|segment| !segment.is_empty())
        .collect();
    if tidy.is_empty() {
        format!("flow/{name}")
    } else {
        tidy.join("/")
    }
}

/// One path segment without separators at its ends or two in a row.
fn tidy_segment(segment: &str) -> String {
    let separator = |c: char| matches!(c, '-' | '_' | '.');
    let mut tidy = String::new();
    for c in segment.trim_matches(separator).chars() {
        if separator(c) && tidy.ends_with(separator) {
            continue;
        }
        tidy.push(c);
    }
    tidy
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    fn convention() -> Branching {
        Branching {
            name: "{type}/{ticket}-{slug}".into(),
            types: vec!["feature".into(), "bugfix".into()],
        }
    }

    #[test]
    fn fills_the_template_from_the_request() {
        let rules = convention();
        let name = |kind, request| branch_name(Some(&rules), Some(kind), request, "x");

        assert_eq!(
            name("feature", "SHOP-217 add CSV export"),
            "feature/SHOP-217-add-csv-export"
        );
        assert_eq!(
            name("bugfix", "SHOP-230 fix totals on refunds"),
            "bugfix/SHOP-230-fix-totals-on-refunds"
        );
        assert_eq!(
            name("feature", "rate-limit the export endpoint"),
            "feature/rate-limit-the-export-endpoint"
        );
        assert_eq!(name("feature", "SHOP-9"), "feature/SHOP-9");
    }

    #[test]
    fn keeps_flow_branches_without_a_convention() {
        assert_eq!(
            branch_name(None, None, "SHOP-217 add", "shop-217"),
            "flow/shop-217"
        );
        let typeless = Branching {
            name: "{type}/{name}".into(),
            types: vec![],
        };
        assert_eq!(branch_name(Some(&typeless), None, "x", "shop-1"), "shop-1");
    }

    #[test]
    fn picks_the_asked_type_or_the_first() {
        let rules = convention();

        assert_eq!(rules.pick_type(None).unwrap(), Some("feature".into()));
        assert_eq!(
            rules.pick_type(Some("bugfix")).unwrap(),
            Some("bugfix".into())
        );
        let unknown = rules.pick_type(Some("hotfix")).unwrap_err().to_string();
        assert_eq!(
            unknown,
            "no branch type hotfix; this repo has feature, bugfix"
        );
        let none = Branching {
            types: vec![],
            ..rules
        };
        assert_eq!(none.pick_type(None).unwrap(), None);
        assert!(none.pick_type(Some("bugfix")).is_err());
    }

    #[test]
    fn loads_the_branch_section_of_the_repo_config() {
        let dir = TempDir::new().unwrap();
        assert_eq!(Branching::load(dir.path()).unwrap(), None);

        fs::create_dir_all(dir.path().join(".flow")).unwrap();
        let config = dir.path().join(CONFIG_FILE);
        fs::write(
            &config,
            "[branch]\nname = \"{type}/{ticket}-{slug}\"\ntypes = [\"feature\", \"bugfix\"]\n",
        )
        .unwrap();
        assert_eq!(Branching::load(dir.path()).unwrap(), Some(convention()));

        fs::write(&config, "[other]\nx = 1\n").unwrap();
        assert_eq!(Branching::load(dir.path()).unwrap(), None);

        fs::write(&config, "[branch]\nname = \"x\"\ntipes = []\n").unwrap();
        let error = format!("{:#}", Branching::load(dir.path()).unwrap_err());
        assert!(error.contains("tipes"), "{error}");
    }
}
