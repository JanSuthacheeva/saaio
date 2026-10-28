//! One module per command, and what several share: `views`, `check_request`, `no_flow`,
//! `repo_dir`, `of_own_repo`

pub(super) mod bug_report;
pub(super) mod clear;
pub(super) mod completions;
pub(super) mod home;
pub(super) mod hook;
pub(super) mod new;
pub(super) mod remove;
pub(super) mod report;
mod retire;
pub(super) mod send;
pub(super) mod server;
pub(super) mod start;
pub(super) mod status;
pub(super) mod stop;
pub(super) mod trust;
pub(super) mod ui;
pub(super) mod update;
pub(super) mod wait;

use std::path::{Path, PathBuf};

use crate::adapter::calls;
use crate::cli::output::Failure;
use crate::config::Config;
use crate::config::kind::Kind;
use crate::event::REQUEST_LIMIT;
use crate::fold;
use crate::launch::worktree::repo_of;
use crate::store::Store;
use crate::view::FlowView;
use crate::workspace;

/// Every flow folded; unreadable ones become warnings so the rest still shows.
fn views(store: &Store, now: u64) -> (Vec<FlowView>, Vec<String>) {
    let (logs, warnings) = store.logs();
    (fold::views(&logs, now), warnings)
}

/// Refuses a request over REQUEST_LIMIT chars, so it is never cut on the way to the worker.
fn check_request(request: &str) -> Result<(), Failure> {
    let length = request.chars().count();
    if length > REQUEST_LIMIT {
        return Err(Failure::error(
            format!("the request is {length} chars, over {REQUEST_LIMIT}"),
            vec!["Put the details in a file in the repo and name it in the request".to_string()],
        ));
    }
    Ok(())
}

/// The config's kind `asked` names (or its default); the failure lists the kinds.
fn pick_kind<'a>(config: &'a Config, asked: Option<&str>) -> Result<(&'a str, &'a Kind), Failure> {
    config.kind(asked).ok_or_else(|| {
        let known: Vec<&str> = config.kinds.keys().map(String::as_str).collect();
        Failure::error(
            format!("no kind named {}", asked.unwrap_or(&config.worker.kind)),
            vec![format!("Kinds: {}", known.join(", "))],
        )
    })
}

fn no_flow(name: &str) -> Failure {
    Failure::error(
        format!("no flow named {name}"),
        vec![
            "Run `saaio` to list the flows".to_string(),
            format!("Run `saaio new --name {name} --request <text>` to start it"),
        ],
    )
}

/// Where a flow's remotes are read: its main checkout, never the worktree whose .git the worker
/// controls; a plain session's own folder; None without a folder.
fn repo_dir(flow: &FlowView) -> Option<PathBuf> {
    let space = flow.workspace.as_ref()?;
    Some(repo_of(space).unwrap_or_else(|| PathBuf::from(&space.path)))
}

/// True when `url` is a merge request of the repository whose remotes git reads in `dir`: origin,
/// or the upstream a fork's `gh pr create` opens it in.
fn of_own_repo(dir: &Path, url: &str) -> bool {
    workspace::git::remote_urls(dir)
        .iter()
        .any(|remote| calls::in_repo(url, remote, workspace::git::ssh_hostname))
}
