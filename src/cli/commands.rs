pub(super) mod clear;
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
pub(super) mod ui;
pub(super) mod wait;

use crate::cli::output::Failure;
use crate::fold;
use crate::store::Store;
use crate::view::FlowView;

/// Every flow folded; unreadable ones become warnings so the rest still shows.
fn views(store: &Store, now: u64) -> (Vec<FlowView>, Vec<String>) {
    let (logs, warnings) = store.logs();
    (fold::views(&logs, now), warnings)
}

fn no_flow(name: &str) -> Failure {
    Failure::error(
        format!("no flow named {name}"),
        vec![
            "Run `flow` to list the flows".to_string(),
            format!("Run `flow new --name {name} --request <text>` to start it"),
        ],
    )
}
