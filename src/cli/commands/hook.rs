use std::io::Read;

use super::views;
use crate::adapter::Adapter;
use crate::cli::Env;
use crate::cli::output::Document;
use crate::config::Config;
use crate::event::Event;
use crate::fold;
use crate::store::Store;

#[derive(Debug, clap::Args)]
pub struct Args {
    /// The harness: claude-code
    adapter: String,
}

/// `flow hook`: never prints to stdout (a hook's stdout can reach the model) and never fails (a
/// failing hook shows the user a notice); what goes wrong goes to stderr, Claude Code's debug log.
pub fn run(args: &Args, env: &Env, clock: fn() -> u64) -> Document {
    let Some(store) = env.store() else {
        eprintln!("flow hook: cannot find a state directory");
        return Document::Silent;
    };
    // A broken config must not stop running workers from reporting; start and ui say it loudly.
    let config = env.config().unwrap_or_else(|error| {
        eprintln!("flow hook: {error:#}; using the built-in config");
        Config::builtin()
    });
    let mut payload = String::new();
    if let Err(error) = std::io::stdin().read_to_string(&mut payload) {
        eprintln!("flow hook: could not read the event: {error}");
        return Document::Silent;
    }
    hook(
        &store,
        &config,
        &args.adapter,
        env.flow_id(),
        &payload,
        clock(),
    );
    Document::Silent
}

/// Translates one hook event with the named adapter and appends what changes the flow.
fn hook(
    store: &Store,
    config: &Config,
    adapter: &str,
    flow: Option<&str>,
    payload: &str,
    now: u64,
) {
    let Some(flow) = flow else {
        return;
    };
    let Some(named) = Adapter::named(adapter) else {
        eprintln!("flow hook: no adapter named {adapter}");
        return;
    };
    let payload: serde_json::Value = match serde_json::from_str(payload) {
        Ok(payload) => payload,
        Err(error) => {
            eprintln!("flow hook: the event is not JSON: {error}");
            return;
        }
    };
    let kinds = named.translate(&payload, &config.phases);
    if kinds.is_empty() {
        return;
    }
    let id = if store.events(flow).is_ok() {
        flow.to_string()
    } else {
        let (flows, _) = views(store, now);
        let Some(view) = fold::resolve(&flows, flow) else {
            eprintln!("flow hook: no flow {flow}");
            return;
        };
        view.id.clone()
    };
    for kind in kinds {
        let events = match store.events(&id) {
            Ok(events) => events,
            Err(error) => {
                eprintln!("flow hook: {error:#}");
                return;
            }
        };
        if fold::changes(&id, &events, &kind, now)
            && let Err(error) = store.append(&id, &Event::new(now, kind))
        {
            eprintln!("flow hook: {error:#}");
        }
    }
}
