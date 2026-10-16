use super::views;
use crate::cli::output::{Document, Failure, flow};
use crate::event::{Event, EventKind, Mode};
use crate::fold;
use crate::store::Store;

#[derive(Debug, clap::Args)]
pub struct Args {
    #[arg(long)]
    name: String,
    #[arg(long)]
    request: String,
    #[arg(long, value_enum, default_value_t)]
    mode: Mode,
}

/// Tracks a new flow, or returns the unfinished flow of that name.
pub fn run(args: Args, store: &Store, now: u64) -> Result<Document, Failure> {
    let (flows, _) = views(store, now);
    if let Some(existing) = fold::resolve(&flows, &args.name).filter(|flow| !flow.is_finished()) {
        return Ok(flow::created(existing, true));
    }
    let first = Event::new(
        now,
        EventKind::Started {
            name: args.name,
            request: args.request,
            mode: args.mode,
        },
    );
    let id = store.create(&first)?;
    let created = fold::fold(&id, &[first], None, now).expect("a started flow folds");
    Ok(flow::created(&created, false))
}
