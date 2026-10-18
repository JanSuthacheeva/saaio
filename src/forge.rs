use anyhow::{Context, Result};
use serde::Deserialize;

use crate::adapter::calls;

/// Who hosts a repository's merge requests, as its remote says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Forge {
    GitHub,
    GitLab,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Open,
    Merged,
    Closed,
}

/// A merge request the forge lists for a branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub url: String,
    pub status: Status,
}

/// One pull request in `gh pr list --json url,state`.
#[derive(Deserialize)]
struct GhPull {
    url: String,
    state: String,
}

/// One merge request in `glab mr list --output json`.
#[derive(Deserialize)]
struct GlabRequest {
    web_url: String,
    state: String,
}

impl Forge {
    /// The forge of the first remote whose web host (an SSH alias resolved by `real_host`) is
    /// github.com or has "gitlab" in it; None for any other host.
    pub fn of(remotes: &[String], real_host: impl Fn(&str) -> Option<String>) -> Option<Forge> {
        remotes.iter().find_map(
            |remote| match calls::web_host_of(remote, &real_host)?.as_str() {
                "github.com" => Some(Forge::GitHub),
                host if host.contains("gitlab") => Some(Forge::GitLab),
                _ => None,
            },
        )
    }

    /// The program that asks it: gh or glab.
    pub fn program(self) -> &'static str {
        match self {
            Forge::GitHub => "gh",
            Forge::GitLab => "glab",
        }
    }

    /// gh pr list --head <branch> --state all --json url,state --limit 10
    /// glab mr list --source-branch <branch> --all --output json --per-page 10
    pub fn argv(self, branch: &str) -> Vec<String> {
        let args: &[&str] = match self {
            Forge::GitHub => &[
                "pr",
                "list",
                "--head",
                branch,
                "--state",
                "all",
                "--json",
                "url,state",
                "--limit",
                "10",
            ],
            Forge::GitLab => &[
                "mr",
                "list",
                "--source-branch",
                branch,
                "--all",
                "--output",
                "json",
                "--per-page",
                "10",
            ],
        };
        std::iter::once(self.program())
            .chain(args.iter().copied())
            .map(String::from)
            .collect()
    }

    /// The merge requests in gh's or glab's JSON (gh: url, state OPEN|MERGED|CLOSED; glab:
    /// web_url, state opened|locked|merged|closed); none for empty output; an error for JSON of
    /// another shape or an unknown state.
    pub fn parse(self, json: &str) -> Result<Vec<Found>> {
        if json.trim().is_empty() {
            return Ok(Vec::new());
        }
        let listed: Vec<(String, String)> = match self {
            Forge::GitHub => serde_json::from_str::<Vec<GhPull>>(json).map(|pulls| {
                pulls
                    .into_iter()
                    .map(|pull| (pull.url, pull.state))
                    .collect()
            }),
            Forge::GitLab => serde_json::from_str::<Vec<GlabRequest>>(json).map(|requests| {
                requests
                    .into_iter()
                    .map(|request| (request.web_url, request.state))
                    .collect()
            }),
        }
        .with_context(|| format!("{} printed no list of merge requests", self.program()))?;
        listed
            .into_iter()
            .map(|(url, state)| {
                let status = match state.to_lowercase().as_str() {
                    "open" | "opened" | "locked" => Status::Open,
                    "merged" => Status::Merged,
                    "closed" => Status::Closed,
                    other => anyhow::bail!("{} named an unknown state {other}", self.program()),
                };
                Ok(Found { url, status })
            })
            .collect()
    }
}

/// The one that speaks for the branch: the first open, else the first merged, else the first
/// closed (both tools list newest first).
pub fn pick(found: Vec<Found>) -> Option<Found> {
    let rank = |status| match status {
        Status::Open => 0,
        Status::Merged => 1,
        Status::Closed => 2,
    };
    found
        .into_iter()
        .enumerate()
        .min_by_key(|(at, one)| (rank(one.status), *at))
        .map(|(_, one)| one)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> &'static str {
        match name {
            "gh-merged" => include_str!("forge/fixtures/gh-merged.json"),
            "gh-open" => include_str!("forge/fixtures/gh-open.json"),
            "gh-closed-then-open" => include_str!("forge/fixtures/gh-closed-then-open.json"),
            "gh-none" => include_str!("forge/fixtures/gh-none.json"),
            "glab-merged" => include_str!("forge/fixtures/glab-merged.json"),
            "glab-opened" => include_str!("forge/fixtures/glab-opened.json"),
            "glab-closed" => include_str!("forge/fixtures/glab-closed.json"),
            _ => panic!("no fixture {name}"),
        }
    }

    fn found(url: &str, status: Status) -> Found {
        Found {
            url: url.into(),
            status,
        }
    }

    fn no_alias(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn knows_the_forge_from_a_remote() {
        let of = |remotes: &[&str]| {
            let remotes: Vec<String> = remotes.iter().map(ToString::to_string).collect();
            Forge::of(&remotes, no_alias)
        };

        assert_eq!(of(&["git@github.com:shop/app.git"]), Some(Forge::GitHub));
        assert_eq!(of(&["https://gitlab.com/shop/app"]), Some(Forge::GitLab));
        assert_eq!(
            of(&["git@gitlab.example.com:shop/app.git"]),
            Some(Forge::GitLab)
        );
        assert_eq!(of(&["https://git.example.com/shop/app"]), None);
        assert_eq!(of(&[]), None);
        assert_eq!(
            of(&[
                "https://git.example.com/shop/app",
                "git@github.com:upstream/app.git"
            ]),
            Some(Forge::GitHub),
            "the first remote of a known forge"
        );
        let github = |alias: &str| (alias == "github-work").then(|| "github.com".to_string());
        assert_eq!(
            Forge::of(&["git@github-work:shop/app.git".to_string()], github),
            Some(Forge::GitHub),
            "an ssh alias"
        );
    }

    #[test]
    fn asks_gh_and_glab_for_every_merge_request_of_the_branch() {
        assert_eq!(
            Forge::GitHub.argv("flow/csv").join(" "),
            "gh pr list --head flow/csv --state all --json url,state --limit 10"
        );
        assert_eq!(
            Forge::GitLab.argv("flow/csv").join(" "),
            "glab mr list --source-branch flow/csv --all --output json --per-page 10"
        );
    }

    #[test]
    fn reads_what_gh_lists() {
        let pull = |number: u32| format!("https://github.com/JanSuthacheeva/saaio/pull/{number}");

        assert_eq!(
            Forge::GitHub.parse(fixture("gh-merged")).unwrap(),
            [found(&pull(42), Status::Merged)]
        );
        assert_eq!(
            Forge::GitHub.parse(fixture("gh-open")).unwrap(),
            [found(&pull(44), Status::Open)]
        );
        assert_eq!(
            Forge::GitHub.parse(fixture("gh-closed-then-open")).unwrap(),
            [
                found(&pull(45), Status::Closed),
                found(&pull(44), Status::Open)
            ]
        );
        assert_eq!(Forge::GitHub.parse(fixture("gh-none")).unwrap(), []);
    }

    #[test]
    fn reads_what_glab_lists() {
        let request =
            |number: u32| format!("https://gitlab.com/acme/shop/-/merge_requests/{number}");

        assert_eq!(
            Forge::GitLab.parse(fixture("glab-merged")).unwrap(),
            [found(&request(418), Status::Merged)]
        );
        assert_eq!(
            Forge::GitLab.parse(fixture("glab-opened")).unwrap(),
            [found(&request(419), Status::Open)]
        );
        assert_eq!(
            Forge::GitLab.parse(fixture("glab-closed")).unwrap(),
            [found(&request(418), Status::Closed)]
        );
        assert_eq!(Forge::GitLab.parse("").unwrap(), []);
    }

    #[test]
    fn refuses_output_of_another_shape() {
        assert!(
            Forge::GitHub
                .parse("To get started with GitHub CLI, please run: gh auth login")
                .is_err()
        );
        assert!(Forge::GitLab.parse(fixture("gh-open")).is_err());
        assert!(
            Forge::GitHub
                .parse(r#"[{"url":"u","state":"DRAFT"}]"#)
                .is_err()
        );
    }

    #[test]
    fn an_open_one_speaks_for_the_branch_before_a_merged_or_closed_one() {
        let closed = found("c", Status::Closed);
        let merged = found("m", Status::Merged);
        let open = found("o", Status::Open);

        assert_eq!(
            pick(vec![closed.clone(), merged.clone(), open.clone()]),
            Some(open)
        );
        assert_eq!(pick(vec![closed.clone(), merged.clone()]), Some(merged));
        assert_eq!(pick(vec![closed.clone()]), Some(closed));
        assert_eq!(pick(Vec::new()), None);
    }
}
