use std::thread;
use std::time::Duration;

use crate::cli::Env;
use crate::cli::output::{Document, news};
use crate::deliver::{self, WorkerNews};
use crate::store::{Busy, Store, Waiter};
use crate::view::FlowView;

/// How often `saaio wait` looks for news, as often as the server looks at the store.
const WAIT_EVERY: Duration = Duration::from_millis(500);

#[derive(Debug, clap::Args)]
pub struct Args {
    /// The master's news: flows that changed [default: $SAAIO_FLOW_ID's messages]
    #[arg(long)]
    master: bool,
}

/// `saaio wait`: the news to wake the session with; Silent when the wait ends without news.
pub fn run(args: &Args, env: &Env, clock: fn() -> u64) -> Document {
    let cwd = std::env::current_dir().ok();
    let Ok(Some(store)) = env.store(cwd.as_deref()) else {
        return Document::Silent;
    };
    let waiter = match env.flow_id() {
        _ if args.master => Waiter::Master,
        Some(id) => Waiter::Flow(id.to_string()),
        None => return Document::Silent,
    };
    match wait(&store, &waiter, clock) {
        Some(news) => Document::Wake(news),
        None => Document::Silent,
    }
}

/// Sleeps until there is news for the session, marked busy with the turn it wakes; None once
/// another waiter took over, the session ended (the parent changed), or a worker's flow is
/// finished or gone.
fn wait(store: &Store, waiter: &Waiter, clock: fn() -> u64) -> Option<String> {
    let me = std::process::id();
    store.claim_waiter(waiter, me).ok()?;
    let parent = std::os::unix::process::parent_id();
    let is_mine =
        || store.waiter(waiter) == Some(me) && std::os::unix::process::parent_id() == parent;
    while is_mine() {
        let news = match waiter {
            Waiter::Flow(id) => match deliver::worker_news(store, id, clock()) {
                WorkerNews::Gone => return None,
                WorkerNews::Nothing => None,
                WorkerNews::News(messages) => {
                    let messages: Vec<&str> = messages.iter().map(String::as_str).collect();
                    Some(news::for_worker(&messages))
                }
            },
            Waiter::Master => {
                let changed = deliver::master_news(store, clock, &is_mine);
                let changed: Vec<&FlowView> = changed.iter().collect();
                (!changed.is_empty()).then(|| news::for_master(&changed))
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
