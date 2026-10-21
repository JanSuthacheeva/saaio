//! Bare `saaio` run by an agent or a script: the binary, the description and every flow

use super::views;
use crate::cli::output::{Document, Failure, status};
use crate::fold;
use crate::store::Store;

/// `saaio` alone: the binary, the description and every flow.
#[expect(
    clippy::unnecessary_wraps,
    reason = "every command's run returns Result<Document, Failure>, so dispatch stays alike"
)]
pub fn run(store: &Store, bin: &str, now: u64) -> Result<Document, Failure> {
    let (flows, warnings) = views(store, now);
    Ok(status::home(&fold::board(flows, now), bin, &warnings))
}
