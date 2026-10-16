use super::{no_flow, views};
use crate::cli::output::{self, Document, Failure};
use crate::event::{Event, EventKind, LONG_TEXT_LIMIT};
use crate::fold;
use crate::store::Store;

#[derive(Debug, clap::Args)]
pub struct Args {
    /// Flow name or id
    flow: String,
    /// What to tell it
    text: String,
}

/// Appends the master's message to a flow its worker reads when flow wakes it; `sender` is the
/// FLOW_ID of the session that sends, set only in a worker's.
pub fn run(
    args: &Args,
    store: &Store,
    sender: Option<&str>,
    now: u64,
) -> Result<Document, Failure> {
    if sender.is_some() {
        return Err(Failure::error(
            "only the master sends",
            vec!["End your turn with the question; flow wakes the master".to_string()],
        ));
    }
    let text = args.text.trim();
    if text.is_empty() {
        return Err(Failure::error("nothing to send", Vec::new()));
    }
    let length = text.chars().count();
    if length > LONG_TEXT_LIMIT {
        return Err(Failure::error(
            format!("the message is {length} chars, over {LONG_TEXT_LIMIT}"),
            vec!["Say it shorter, or point to a file in the worktree".to_string()],
        ));
    }
    let (flows, _) = views(store, now);
    let flow = fold::resolve(&flows, &args.flow).ok_or_else(|| no_flow(&args.flow))?;
    if flow.is_finished() {
        return Err(Failure::error(
            format!("{} is finished", flow.name),
            vec!["Run `flow` for the flows still running".to_string()],
        ));
    }
    if flow.workspace.is_none() {
        return Err(Failure::error(
            format!(
                "{} has no worker; it was not started with flow start",
                flow.name
            ),
            Vec::new(),
        ));
    }
    if flow.is_plain() {
        return Err(Failure::error(
            format!("{} is a plain session; it is not the master's", flow.name),
            vec!["Plain sessions are the user's own".to_string()],
        ));
    }
    let message = EventKind::Message {
        text: text.to_string(),
    };
    store.append(&flow.id, &Event::new(now, message))?;
    Ok(output::flow::sent(flow))
}
