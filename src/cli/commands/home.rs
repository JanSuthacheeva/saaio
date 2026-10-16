use super::views;
use crate::cli::output::{Document, Failure, status};
use crate::fold;
use crate::store::Store;

/// `flow` alone: the binary, the description and every flow.
pub fn run(store: &Store, bin: &str, now: u64) -> Result<Document, Failure> {
    let (flows, warnings) = views(store, now);
    Ok(status::home(&fold::board(flows, now), bin, &warnings))
}
