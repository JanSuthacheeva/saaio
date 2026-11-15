//! Bare `saaio` run by an agent or a script: the binary, the description and every flow

use super::views;
use crate::cli::context::{Context, Opened};
use crate::cli::output::{Document, Failure, status};
use crate::fold;

/// `saaio` alone: the binary, the description and every flow.
pub fn run(ctx: &Context) -> Result<Document, Failure> {
    let Opened { store, .. } = ctx.open()?;
    let now = ctx.now();
    let (flows, warnings) = views(&store, now);
    Ok(status::home(
        &fold::board(flows, now),
        &ctx.env.bin_path(),
        &warnings,
    ))
}
