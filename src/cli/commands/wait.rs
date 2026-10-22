//! `saaio wait`: polls the store for news as Claude Code's `asyncRewake` hook

use std::thread;
use std::time::Duration;

use crate::cli::Env;
use crate::cli::output::Document;
use crate::config::watch::ConfigWatch;
use crate::deliver::{self, WorkerNews};
use crate::event::is_uuid;
use crate::identity;
use crate::store::{Busy, Store, Waiter};
use crate::view::FlowView;
use crate::wake;

/// How often `saaio wait` looks for news, as often as the server looks at the store.
const WAIT_EVERY: Duration = Duration::from_millis(500);

#[derive(Debug, clap::Args)]
pub struct Args {
    /// The master's news: flows that changed [default: $SAAIO_FLOW_ID's messages]
    #[arg(long)]
    master: bool,
}

/// `saaio wait`: the news to wake the session with; Silent when the wait ends without news, or
/// at once when the session's key does not prove it.
pub fn run(args: &Args, env: &Env, clock: fn() -> u64) -> Document {
    let cwd = std::env::current_dir().ok();
    let Ok(Some(store)) = env.store(cwd.as_deref()) else {
        return Document::Silent;
    };
    let waiter = match env.flow_id() {
        _ if args.master => Waiter::Master,
        Some(id) if is_uuid(id) => Waiter::Flow(id.to_string()),
        _ => return Document::Silent,
    };
    if !identity::proves(&store, &waiter, env.key()) {
        return Document::Silent;
    }
    let mut configs = ConfigWatch::new(env.config_file());
    match wait(&store, &waiter, &mut configs, clock) {
        Some(news) => Document::Wake(news),
        None => Document::Silent,
    }
}

/// Sleeps until there is news for the session, marked busy with the turn it wakes; None once
/// another waiter took over, the session ended (the parent changed), or a worker's flow is
/// finished or gone. The master gets none while [master] wake is false; a broken config keeps
/// the last value it read.
fn wait(
    store: &Store,
    waiter: &Waiter,
    configs: &mut ConfigWatch,
    clock: fn() -> u64,
) -> Option<String> {
    let me = std::process::id();
    store.claim_waiter(waiter, me).ok()?;
    let parent = std::os::unix::process::parent_id();
    let is_mine =
        || store.waiter(waiter) == Some(me) && std::os::unix::process::parent_id() == parent;
    let mut wakes = true;
    while is_mine() {
        if let Some(Ok(config)) = configs.changed() {
            wakes = config.master.wake;
        }
        let news = match waiter {
            Waiter::Flow(id) => match deliver::worker_news(store, id, clock()) {
                WorkerNews::Gone => return None,
                WorkerNews::Nothing => None,
                WorkerNews::News(messages) => {
                    let messages: Vec<&str> = messages.iter().map(String::as_str).collect();
                    Some(wake::for_worker(&messages))
                }
            },
            Waiter::Master if !wakes => None,
            Waiter::Master => {
                let changed = deliver::master_news(store, clock, &is_mine);
                let changed: Vec<&FlowView> = changed.iter().collect();
                (!changed.is_empty()).then(|| wake::for_master(&changed))
            }
        };
        if news.is_some() {
            // The turn it wakes runs no prompt hook: its Stop marks it idle again.
            if let Err(error) = store.set_busy(waiter, Busy::Turn) {
                eprintln!("saaio wait: {error:#}");
            }
            return news;
        }
        thread::sleep(WAIT_EVERY);
    }
    None
}
