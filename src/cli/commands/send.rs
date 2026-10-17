use super::{no_flow, views};
use crate::cli::Env;
use crate::cli::output::{self, Document, Failure};
use crate::event::{Event, EventKind, LONG_TEXT_LIMIT};
use crate::fold;
use crate::store::Store;
use crate::view::FlowView;
use crate::wake;

#[derive(Debug, clap::Args)]
pub struct Args {
    /// Flow name or id
    flow: String,
    /// What to tell it, or with --type the line to type
    text: String,
    /// Type the text and Enter into the worker's pane once it is idle, as if the user typed it
    #[arg(long = "type")]
    typed: bool,
}

/// Appends the master's message to a flow its worker reads when flow wakes it, or with --type a
/// line flow server types into its pane; only the master sends (no FLOW_ID in its env).
pub fn run(args: &Args, store: &Store, env: &Env, now: u64) -> Result<Document, Failure> {
    if env.flow_id().is_some() {
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
    if args.typed && text.contains(['\n', '\r']) {
        return Err(Failure::error(
            "a typed line is one line",
            vec!["Queue one `flow send --type` per line".to_string()],
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
    if args.typed {
        return queue(store, env, flow, text, now);
    }
    let message = EventKind::Message {
        text: text.to_string(),
    };
    store.append(&flow.id, &Event::new(now, message))?;
    Ok(output::flow::sent(flow))
}

/// Appends a line for flow server to type, once the worker's agent reports its turns to flow;
/// only this path reads config.toml, so a broken one leaves plain messages working.
fn queue(
    store: &Store,
    env: &Env,
    flow: &FlowView,
    text: &str,
    now: u64,
) -> Result<Document, Failure> {
    let agent = flow.workspace.as_ref().map(|space| space.agent.as_str());
    let reports = env
        .config()?
        .agent(agent)
        .is_some_and(|(_, agent)| agent.adapter.is_some());
    if !reports {
        return Err(Failure::error(
            format!(
                "{} runs {}, which reports no turns to flow",
                flow.name,
                agent.unwrap_or_default()
            ),
            vec!["Use `flow send` for a message".to_string()],
        ));
    }
    let events = store.events(&flow.id)?;
    let ahead = wake::untyped(&events, store.typed(&flow.id)).len();
    let line = EventKind::Typed {
        text: text.to_string(),
    };
    store.append(&flow.id, &Event::new(now, line))?;
    Ok(output::flow::queued(flow, text, ahead))
}
