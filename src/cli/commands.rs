//! One module per command, and what several share: `views`, `check_request`, `no_flow`

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

use crate::cli::output::Failure;
use crate::event::REQUEST_LIMIT;
use crate::fold;
use crate::store::Store;
use crate::view::FlowView;

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

fn no_flow(name: &str) -> Failure {
    Failure::error(
        format!("no flow named {name}"),
        vec![
            "Run `saaio` to list the flows".to_string(),
            format!("Run `saaio new --name {name} --request <text>` to start it"),
        ],
    )
}
