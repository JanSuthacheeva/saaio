use std::io::Read;

use serde_json::Value;

use super::views;
use crate::adapter::Adapter;
use crate::cli::Env;
use crate::cli::output::{Document, news};
use crate::config::Config;
use crate::deliver::{self, WorkerNews};
use crate::event::{Event, EventKind};
use crate::fold;
use crate::store::{Store, Waiter};
use crate::view::FlowView;

#[derive(Debug, clap::Args)]
pub struct Args {
    /// The harness: claude-code or codex
    adapter: String,
    /// The master's own hooks: record its session, hand it its news
    #[arg(long)]
    master: bool,
}

/// Whose hook ran: a flow's session ($FLOW_ID), the master's (--master), or neither.
enum Hooked {
    Flow(String),
    Master,
    Nobody,
}

/// `flow hook`: never prints to stdout (a hook's stdout can reach the model) and never fails (a
/// failing hook shows the user a notice); what goes wrong goes to stderr, the harness's debug log.
/// A Stop that hands news over wakes the session with it (exit 2, the news on stderr).
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
    let hooked = match env.flow_id() {
        _ if args.master => Hooked::Master,
        Some(id) => Hooked::Flow(id.to_string()),
        None => Hooked::Nobody,
    };
    let Some(adapter) = Adapter::named(&args.adapter) else {
        eprintln!("flow hook: no adapter named {}", args.adapter);
        return Document::Silent;
    };
    let mut text = String::new();
    if let Err(error) = std::io::stdin().read_to_string(&mut text) {
        eprintln!("flow hook: could not read the event: {error}");
        return Document::Silent;
    }
    let payload: Value = match serde_json::from_str(&text) {
        Ok(payload) => payload,
        Err(error) => {
            eprintln!("flow hook: the event is not JSON: {error}");
            return Document::Silent;
        }
    };
    match hook(&store, &config, adapter, &hooked, &payload, clock()) {
        Some(news) => Document::Wake(news),
        None => Document::Silent,
    }
}

/// Translates one hook event and appends what changes the flow; for a session that does not wake
/// itself, also marks it busy or idle and, at its Stop, takes the news it is handed.
fn hook(
    store: &Store,
    config: &Config,
    adapter: Adapter,
    hooked: &Hooked,
    payload: &Value,
    now: u64,
) -> Option<String> {
    let kinds = adapter.translate(payload, &config.phases);
    let waiter = match hooked {
        Hooked::Nobody => return None,
        Hooked::Master => {
            record_master_session(store, &kinds);
            Waiter::Master
        }
        Hooked::Flow(flow) => {
            let id = report(store, flow, kinds, now)?;
            Waiter::Flow(id)
        }
    };
    if adapter.wakes_itself() {
        return None;
    }
    match payload["hook_event_name"].as_str() {
        Some("UserPromptSubmit") => mark_busy(store, &waiter, true),
        Some("Interrupt") => mark_busy(store, &waiter, false),
        Some("Stop") => {
            let wakes = !matches!(waiter, Waiter::Master) || config.master.wake;
            let news = if wakes {
                hand_over(store, &waiter, now)
            } else {
                None
            };
            if news.is_none() {
                mark_busy(store, &waiter, false);
            }
            return news;
        }
        _ => {}
    }
    None
}

/// Appends the kinds that change flow `flow` (a name or id); its id, None when there is no such
/// flow or its log cannot be read.
fn report(store: &Store, flow: &str, kinds: Vec<EventKind>, now: u64) -> Option<String> {
    let id = if store.events(flow).is_ok() {
        flow.to_string()
    } else {
        let (flows, _) = views(store, now);
        let Some(view) = fold::resolve(&flows, flow) else {
            eprintln!("flow hook: no flow {flow}");
            return None;
        };
        view.id.clone()
    };
    for kind in kinds {
        let events = match store.events(&id) {
            Ok(events) => events,
            Err(error) => {
                eprintln!("flow hook: {error:#}");
                return None;
            }
        };
        if fold::changes(&id, &events, &kind, now)
            && let Err(error) = store.append(&id, &Event::new(now, kind))
        {
            eprintln!("flow hook: {error:#}");
        }
    }
    Some(id)
}

/// The news a session's Stop hands over: a worker's messages from the master (its flow working
/// again, since the session goes on with them), the flows that stopped for the master.
fn hand_over(store: &Store, waiter: &Waiter, now: u64) -> Option<String> {
    match waiter {
        Waiter::Flow(id) => {
            let WorkerNews::News(messages) = deliver::worker_news(store, id, now) else {
                return None;
            };
            if let Err(error) = store.append(id, &Event::new(now, EventKind::Working)) {
                eprintln!("flow hook: {error:#}");
            }
            let messages: Vec<&str> = messages.iter().map(String::as_str).collect();
            Some(news::for_worker(&messages))
        }
        Waiter::Master => {
            let changed = deliver::master_news_now(store, &views(store, now).0);
            let changed: Vec<&FlowView> = changed.iter().collect();
            (!changed.is_empty()).then(|| news::for_master(&changed))
        }
    }
}

/// Keeps the master's session id, which its harness picked, so the server can resume it.
fn record_master_session(store: &Store, kinds: &[EventKind]) {
    let session = kinds.iter().find_map(|kind| match kind {
        EventKind::Session { id, .. } => Some(id),
        _ => None,
    });
    if let Some(id) = session
        && store.master_session().as_ref() != Some(id)
        && let Err(error) = store.set_master_session(id)
    {
        eprintln!("flow hook: {error:#}");
    }
}

fn mark_busy(store: &Store, waiter: &Waiter, busy: bool) {
    if let Err(error) = store.set_busy(waiter, busy) {
        eprintln!("flow hook: {error:#}");
    }
}
