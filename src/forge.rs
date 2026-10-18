use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::adapter::calls;
use crate::event::EventKind;
use crate::view::FlowView;

/// Who hosts a repository's merge requests, as its remote says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Forge {
    GitHub,
    GitLab,
}

/// A flow to ask about: its id, its worktree, its branch and when it started.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ask {
    pub id: String,
    pub dir: PathBuf,
    pub branch: String,
    pub since: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Open,
    Merged,
    Closed,
}

/// A merge request the forge lists for a branch, and when it was opened (Unix ms).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub url: String,
    pub status: Status,
    pub created_at: u64,
}

/// One pull request in `gh pr list --json url,state,createdAt`.
#[derive(Deserialize)]
struct GhPull {
    url: String,
    state: String,
    #[serde(rename = "createdAt")]
    created_at: String,
}

/// One merge request in `glab mr list --output json`.
#[derive(Deserialize)]
struct GlabRequest {
    web_url: String,
    state: String,
    created_at: String,
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

    /// gh pr list --head <branch> --state all --json url,state,createdAt --limit 10
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
                "url,state,createdAt",
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

    /// The merge requests in gh's or glab's JSON (gh: url, state OPEN|MERGED|CLOSED, createdAt;
    /// glab: web_url, state opened|locked|merged|closed, created_at); none for empty output; an
    /// error for JSON of another shape, an unknown state or an unreadable time.
    pub fn parse(self, json: &str) -> Result<Vec<Found>> {
        if json.trim().is_empty() {
            return Ok(Vec::new());
        }
        let listed: Vec<(String, String, String)> = match self {
            Forge::GitHub => serde_json::from_str::<Vec<GhPull>>(json).map(|pulls| {
                pulls
                    .into_iter()
                    .map(|pull| (pull.url, pull.state, pull.created_at))
                    .collect()
            }),
            Forge::GitLab => serde_json::from_str::<Vec<GlabRequest>>(json).map(|requests| {
                requests
                    .into_iter()
                    .map(|request| (request.web_url, request.state, request.created_at))
                    .collect()
            }),
        }
        .with_context(|| format!("{} printed no list of merge requests", self.program()))?;
        listed
            .into_iter()
            .map(|(url, state, created)| {
                let status = match state.to_lowercase().as_str() {
                    "open" | "opened" | "locked" => Status::Open,
                    "merged" => Status::Merged,
                    "closed" => Status::Closed,
                    other => anyhow::bail!("{} named an unknown state {other}", self.program()),
                };
                let created_at = unix_ms(&created).with_context(|| {
                    format!("{} named an unreadable time {created}", self.program())
                })?;
                Ok(Found {
                    url,
                    status,
                    created_at,
                })
            })
            .collect()
    }
}

/// The Unix ms of an RFC 3339 time (2026-10-18T02:00:35Z, 2026-10-15T11:12:44.211+02:00).
fn unix_ms(stamp: &str) -> Option<u64> {
    let (date, rest) = stamp.split_once(['T', 't', ' '])?;
    let mut ymd = date.splitn(3, '-').map(str::parse::<i64>);
    let (year, month, day) = (ymd.next()?.ok()?, ymd.next()?.ok()?, ymd.next()?.ok()?);
    let zone_at = rest.find(['Z', 'z', '+', '-'])?;
    let (time, zone) = rest.split_at(zone_at);
    let (hms, fraction) = time.split_once('.').unwrap_or((time, ""));
    let mut parts = hms.splitn(3, ':').map(str::parse::<i64>);
    let (hour, minute, second) = (
        parts.next()?.ok()?,
        parts.next()?.ok()?,
        parts.next()?.ok()?,
    );
    let millis: i64 = format!("{fraction:0<3}").get(..3)?.parse().ok()?;
    let offset = match zone.split_at(1) {
        ("Z" | "z", "") => 0,
        (sign, offset) => {
            let (hours, minutes) = offset.split_once(':')?;
            let minutes = hours.parse::<i64>().ok()? * 60 + minutes.parse::<i64>().ok()?;
            if sign == "-" { -minutes } else { minutes }
        }
    };
    let valid = (1970..=9999).contains(&year)
        && (1..=12).contains(&month)
        && (1..=31).contains(&day)
        && hour < 24
        && minute < 60
        && second < 61;
    if !valid {
        return None;
    }
    let seconds = days_from_civil(year, month, day) * 86_400 + hour * 3600 + minute * 60 + second
        - offset * 60;
    u64::try_from(seconds * 1000 + millis).ok()
}

/// Days since 1970-01-01 of a date in the proleptic Gregorian calendar (Howard Hinnant's
/// days_from_civil).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// The flows worth asking about: not plain, with a branch, not merged.
pub fn asks(flows: &[FlowView]) -> Vec<Ask> {
    flows
        .iter()
        .filter(|flow| !flow.is_plain() && !flow.merged)
        .filter_map(|flow| {
            let space = flow.workspace.as_ref()?;
            Some(Ask {
                id: flow.id.clone(),
                dir: PathBuf::from(&space.path),
                branch: space.branch.clone()?,
                since: flow.started_at,
            })
        })
        .collect()
}

/// What to append so `flow` shows `found`: merge-request for an open one it does not show yet,
/// merged for a merged one, merge-request-closed for the closed one it shows; None otherwise.
pub fn news(flow: &FlowView, found: Option<&Found>) -> Option<EventKind> {
    let found = found?;
    let shown = flow.merge_request.as_ref() == Some(&found.url);
    let url = found.url.clone();
    match found.status {
        Status::Open => (!shown).then_some(EventKind::MergeRequest { url }),
        Status::Merged => (!flow.merged).then_some(EventKind::Merged { url }),
        Status::Closed => (shown && !flow.merged).then_some(EventKind::MergeRequestClosed { url }),
    }
}

/// The one that speaks for the branch: the first open, else the first merged, else the first
/// closed (both tools list newest first); one opened before `since`, the flow's start, belongs to
/// an earlier branch of the same name and never speaks for it.
pub fn pick(found: Vec<Found>, since: u64) -> Option<Found> {
    let rank = |status| match status {
        Status::Open => 0,
        Status::Merged => 1,
        Status::Closed => 2,
    };
    found
        .into_iter()
        .filter(|one| one.created_at >= since)
        .enumerate()
        .min_by_key(|(at, one)| (rank(one.status), *at))
        .map(|(_, one)| one)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{Event, Mode};
    use crate::fold;

    const A: &str = "https://github.com/acme/shop/pull/7";
    const B: &str = "https://github.com/acme/shop/pull/8";

    fn flow_of(mode: Mode, branch: Option<&str>, more: Vec<EventKind>) -> FlowView {
        let started = EventKind::Started {
            name: "csv".into(),
            request: "add csv".into(),
            mode,
            phases: None,
        };
        let workspace = EventKind::Workspace {
            path: "/work/csv".into(),
            branch: branch.map(String::from),
            slot: None,
            window: None,
            agent: "claude".into(),
        };
        let events: Vec<Event> = [started, workspace]
            .into_iter()
            .chain(more)
            .map(|kind| Event::new(1, kind))
            .collect();
        fold::fold("csv-1", &events, None, 2).unwrap()
    }

    fn flow(more: Vec<EventKind>) -> FlowView {
        flow_of(Mode::Auto, Some("flow/csv"), more)
    }

    fn opened(url: &str) -> EventKind {
        EventKind::MergeRequest { url: url.into() }
    }

    fn merged(url: &str) -> EventKind {
        EventKind::Merged { url: url.into() }
    }

    fn closed(url: &str) -> EventKind {
        EventKind::MergeRequestClosed { url: url.into() }
    }

    #[test]
    fn asks_about_every_flow_with_a_branch_until_it_is_merged() {
        let ids = |flows: &[FlowView]| -> Vec<String> {
            asks(flows).into_iter().map(|ask| ask.id).collect()
        };

        assert_eq!(
            asks(&[flow(vec![])]),
            [Ask {
                id: "csv-1".into(),
                dir: PathBuf::from("/work/csv"),
                branch: "flow/csv".into(),
                since: 1,
            }]
        );
        assert_eq!(ids(&[flow(vec![opened(A)])]), ["csv-1"], "done, not merged");
        assert_eq!(ids(&[flow(vec![EventKind::Ended])]), ["csv-1"], "ended");
        assert!(ids(&[flow(vec![merged(A)])]).is_empty(), "merged");
        assert!(
            ids(&[flow_of(Mode::Auto, None, vec![])]).is_empty(),
            "no branch"
        );
        assert!(
            ids(&[flow_of(Mode::Plain, Some("main"), vec![])]).is_empty(),
            "plain"
        );
    }

    #[test]
    fn records_only_what_the_flow_does_not_show_yet() {
        let open = |url: &str| found(url, Status::Open);
        let shut = |url: &str| found(url, Status::Closed);
        let done = |url: &str| found(url, Status::Merged);

        assert_eq!(news(&flow(vec![]), Some(&open(A))), Some(opened(A)));
        assert_eq!(news(&flow(vec![opened(A)]), Some(&open(A))), None);
        assert_eq!(
            news(&flow(vec![opened(A)]), Some(&open(B))),
            Some(opened(B))
        );
        assert_eq!(news(&flow(vec![]), Some(&done(A))), Some(merged(A)));
        assert_eq!(
            news(&flow(vec![opened(A)]), Some(&done(A))),
            Some(merged(A))
        );
        assert_eq!(news(&flow(vec![merged(A)]), Some(&done(A))), None);
        assert_eq!(
            news(&flow(vec![opened(A)]), Some(&shut(A))),
            Some(closed(A))
        );
        assert_eq!(news(&flow(vec![]), Some(&shut(A))), None);
        assert_eq!(news(&flow(vec![opened(B)]), Some(&shut(A))), None);
        assert_eq!(
            news(&flow(vec![opened(A), closed(A)]), Some(&shut(A))),
            None
        );
        assert_eq!(news(&flow(vec![opened(A)]), None), None);
    }

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
            created_at: 0,
        }
    }

    fn found_at(url: &str, status: Status, created: &str) -> Found {
        Found {
            created_at: unix_ms(created).unwrap(),
            ..found(url, status)
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
            "gh pr list --head flow/csv --state all --json url,state,createdAt --limit 10"
        );
        assert_eq!(
            Forge::GitLab.argv("flow/csv").join(" "),
            "glab mr list --source-branch flow/csv --all --output json --per-page 10"
        );
    }

    #[test]
    fn reads_what_gh_lists() {
        let pull = |number: u32| format!("https://github.com/JanSuthacheeva/saaio/pull/{number}");

        let at = |url: &str, status, created| found_at(url, status, created);

        assert_eq!(
            Forge::GitHub.parse(fixture("gh-merged")).unwrap(),
            [at(&pull(42), Status::Merged, "2026-10-18T02:00:35Z")]
        );
        assert_eq!(
            Forge::GitHub.parse(fixture("gh-open")).unwrap(),
            [at(&pull(44), Status::Open, "2026-10-18T02:22:32Z")]
        );
        assert_eq!(
            Forge::GitHub.parse(fixture("gh-closed-then-open")).unwrap(),
            [
                at(&pull(45), Status::Closed, "2026-10-18T03:05:10Z"),
                at(&pull(44), Status::Open, "2026-10-18T02:22:32Z")
            ]
        );
        assert_eq!(Forge::GitHub.parse(fixture("gh-none")).unwrap(), []);
    }

    #[test]
    fn reads_what_glab_lists() {
        let request =
            |number: u32| format!("https://gitlab.com/acme/shop/-/merge_requests/{number}");

        let at = |url: &str, status, created| found_at(url, status, created);

        assert_eq!(
            Forge::GitLab.parse(fixture("glab-merged")).unwrap(),
            [at(
                &request(418),
                Status::Merged,
                "2026-10-15T09:12:44.211Z"
            )]
        );
        assert_eq!(
            Forge::GitLab.parse(fixture("glab-opened")).unwrap(),
            [at(&request(419), Status::Open, "2026-10-17T08:40:01.117Z")]
        );
        assert_eq!(
            Forge::GitLab.parse(fixture("glab-closed")).unwrap(),
            [at(
                &request(418),
                Status::Closed,
                "2026-10-15T09:12:44.211Z"
            )]
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
                .parse(r#"[{"url":"u","state":"DRAFT","createdAt":"2026-10-18T02:00:35Z"}]"#)
                .is_err()
        );
        assert!(
            Forge::GitHub
                .parse(r#"[{"url":"u","state":"OPEN","createdAt":"yesterday"}]"#)
                .is_err()
        );
    }

    #[test]
    fn an_open_one_speaks_for_the_branch_before_a_merged_or_closed_one() {
        let closed = found("c", Status::Closed);
        let merged = found("m", Status::Merged);
        let open = found("o", Status::Open);

        assert_eq!(
            pick(vec![closed.clone(), merged.clone(), open.clone()], 0),
            Some(open)
        );
        assert_eq!(pick(vec![closed.clone(), merged.clone()], 0), Some(merged));
        assert_eq!(pick(vec![closed.clone()], 0), Some(closed));
        assert_eq!(pick(Vec::new(), 0), None);
    }

    #[test]
    fn one_opened_before_the_flow_started_belongs_to_an_earlier_branch_of_its_name() {
        let old = found_at("old", Status::Merged, "2026-10-16T13:16:07Z");
        let new = found_at("new", Status::Closed, "2026-10-18T09:00:00Z");
        let since = unix_ms("2026-10-18T08:00:00Z").unwrap();

        assert_eq!(pick(vec![old.clone()], since), None);
        assert_eq!(pick(vec![new.clone(), old.clone()], since), Some(new));
        assert_eq!(pick(vec![old.clone()], old.created_at), Some(old));
    }

    #[test]
    fn reads_the_times_gh_and_glab_print() {
        assert_eq!(unix_ms("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(unix_ms("2026-10-18T02:00:35Z"), Some(1_792_288_835_000));
        assert_eq!(unix_ms("2026-10-15T09:12:44.211Z"), Some(1_792_055_564_211));
        assert_eq!(
            unix_ms("2026-10-15T11:12:44.211+02:00"),
            unix_ms("2026-10-15T09:12:44.211Z")
        );
        assert_eq!(
            unix_ms("2024-02-29T23:30:00-01:00"),
            unix_ms("2024-03-01T00:30:00Z")
        );
        assert_eq!(unix_ms("2026-13-01T00:00:00Z"), None);
        assert_eq!(unix_ms("9223372036854775807-01-01T00:00:00Z"), None);
        assert_eq!(unix_ms("2026-10-18"), None);
        assert_eq!(unix_ms(""), None);
    }
}
