mod commands;
mod env;
pub mod output;

use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use anyhow::Context;
use clap::{Parser, Subcommand};

use commands::{clear, home, new, remove, report, send, start, status, views};

pub use crate::event::unix_ms;
pub use env::Env;

use crate::cli::output::{Document, Failure};
use crate::client::{self, Client};
use crate::config::Config;
use crate::deliver::{self, WorkerNews};
use crate::event::Event;
use crate::proto::{ClientMsg, Role};
use crate::store::{Store, Waiter};
use crate::view::FlowView;
use crate::{adapter, fold, launch, server, ui};

/// How often `flow wait` looks for news, as often as the server looks at the store.
const WAIT_EVERY: Duration = Duration::from_millis(500);
/// Tracks parallel feature flows: what each is doing, where it is blocked, how far it got.
#[derive(Debug, Parser)]
#[command(version, about)]
pub struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Track a new flow, or return the unfinished flow of that name
    New(new::Args),
    /// Append one event to a flow
    Report(report::Args),
    /// Every flow, or one flow with its phases, timeline and decisions
    Status(status::Args),
    /// Start a worker: a flow, a worktree, the setup hook and an agent flow ui opens
    Start(start::Args),
    /// Stop a flow's worker, tear down its worktree and forget it
    Remove(remove::Args),
    /// Tell a flow's worker something: an answer, an instruction, a correction
    Send(send::Args),
    /// Forget flows that are done or ended
    Clear,
    /// Open the terminal UI: the master, every worker and the board
    Ui {
        /// Stop the server and every session in it
        #[arg(long)]
        stop: bool,
    },
    /// What a session's asyncRewake hook runs: sleeps until there is news for it, prints it, exits 2
    #[command(hide = true)]
    Wait {
        /// The master's news: flows that changed [default: $FLOW_ID's messages]
        #[arg(long)]
        master: bool,
    },
    /// The session server flow ui starts on its own
    #[command(hide = true)]
    Server,
    /// What a harness hook runs: reports the event on stdin into $FLOW_ID's flow
    #[command(hide = true)]
    Hook {
        /// The harness: claude-code
        adapter: String,
    },
}

/// Runs the command `cli` names: what to print, or the failure to print instead.
///
/// # Errors
///
/// The command's failure, with its message, next steps and exit code. Hook and Wait run without
/// a store; every other command fails without one.
pub fn run(cli: Cli, env: &Env, clock: fn() -> u64) -> Result<Document, Failure> {
    if let Some(Command::Hook { adapter }) = &cli.command {
        run_hook(adapter, env, clock);
        return Ok(Document::Silent);
    }
    if let Some(Command::Wait { master }) = &cli.command {
        return Ok(run_wait(*master, env, clock));
    }
    let Some(store) = env.store() else {
        return Err(Failure::error(
            "cannot find a state directory",
            vec!["Set FLOW_HOME to a directory".to_string()],
        ));
    };
    let Some(registry) = env.registry() else {
        return Err(Failure::error(
            "cannot find a state directory",
            vec!["Set HOME or XDG_STATE_HOME to a directory".to_string()],
        ));
    };
    // Only for the overview and the slots; a command works without it.
    let _ = registry.register(&store);
    let now = clock();
    let store = &store;
    let registry = &registry;
    match cli.command {
        None => home::run(store, &env.bin_path(), now),
        Some(Command::New(args)) => new::run(args, store, now),
        Some(Command::Report(args)) => report::run(args, store, now),
        Some(Command::Status(args)) => status::run(&args, store, registry, now),
        Some(Command::Start(args)) => {
            let config = env.config()?;
            let cwd = std::env::current_dir()?;
            start::run(&args, store, registry, &config, &cwd, now)
        }
        Some(Command::Remove(args)) => remove::run(&args, store, now),
        Some(Command::Send(args)) => send::run(&args, store, env.flow_id(), now),
        Some(Command::Clear) => clear::run(store, now),
        Some(Command::Ui { stop: true }) => stop_server(&client::socket_path(store)),
        Some(Command::Ui { stop: false }) => ui(store, env, clock),
        Some(Command::Server) => server(store.clone(), env, clock),
        Some(Command::Hook { .. } | Command::Wait { .. }) => Ok(Document::Silent),
    }
}

/// `flow hook`: never prints to stdout (a hook's stdout can reach the model) and never fails (a
/// failing hook shows the user a notice); what goes wrong goes to stderr, Claude Code's debug log.
fn run_hook(adapter: &str, env: &Env, clock: fn() -> u64) {
    let Some(store) = env.store() else {
        return eprintln!("flow hook: cannot find a state directory");
    };
    // A broken config must not stop running workers from reporting; start and ui say it loudly.
    let config = env.config().unwrap_or_else(|error| {
        eprintln!("flow hook: {error:#}; using the built-in config");
        Config::builtin()
    });
    let mut payload = String::new();
    if let Err(error) = std::io::Read::read_to_string(&mut std::io::stdin(), &mut payload) {
        return eprintln!("flow hook: could not read the event: {error}");
    }
    let flow = env.flow_id.clone();
    hook(&store, &config, adapter, flow, &payload, clock());
}

/// `flow wait`: the news to wake the session with; Silent when the wait ends without news.
fn run_wait(master: bool, env: &Env, clock: fn() -> u64) -> Document {
    let Some(store) = env.store() else {
        return Document::Silent;
    };
    let waiter = match env.flow_id() {
        _ if master => Waiter::Master,
        Some(id) => Waiter::Flow(id.to_string()),
        None => return Document::Silent,
    };
    match wait(&store, &waiter, clock) {
        Some(news) => Document::Wake(news),
        None => Document::Silent,
    }
}

/// Sleeps until there is news for the session; None once another waiter took over, the session
/// ended (the parent changed), or a worker's flow is finished or gone.
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
                    Some(output::news_for_worker(&messages))
                }
            },
            Waiter::Master => {
                let changed = deliver::master_news(store, clock, &is_mine);
                let changed: Vec<&FlowView> = changed.iter().collect();
                (!changed.is_empty()).then(|| output::news_for_master(&changed))
            }
        };
        if news.is_some() {
            return news;
        }
        thread::sleep(WAIT_EVERY);
    }
    None
}

/// Translates one hook event with the named adapter and appends what changes the flow.
fn hook(
    store: &Store,
    config: &Config,
    adapter: &str,
    flow: Option<String>,
    payload: &str,
    now: u64,
) -> Document {
    let Some(flow) = flow.filter(|flow| !flow.is_empty()) else {
        return Document::Silent;
    };
    let Some(named) = adapter::Adapter::named(adapter) else {
        eprintln!("flow hook: no adapter named {adapter}");
        return Document::Silent;
    };
    let payload: serde_json::Value = match serde_json::from_str(payload) {
        Ok(payload) => payload,
        Err(error) => {
            eprintln!("flow hook: the event is not JSON: {error}");
            return Document::Silent;
        }
    };
    let kinds = named.translate(&payload, &config.phases);
    if kinds.is_empty() {
        return Document::Silent;
    }
    let id = if store.events(&flow).is_ok() {
        flow
    } else {
        let (flows, _) = views(store, now);
        let Some(view) = fold::resolve(&flows, &flow) else {
            eprintln!("flow hook: no flow {flow}");
            return Document::Silent;
        };
        view.id.clone()
    };
    for kind in kinds {
        let events = match store.events(&id) {
            Ok(events) => events,
            Err(error) => {
                eprintln!("flow hook: {error:#}");
                return Document::Silent;
            }
        };
        if fold::changes(&id, &events, &kind, now)
            && let Err(error) = store.append(&id, &Event::new(now, kind))
        {
            eprintln!("flow hook: {error:#}");
        }
    }
    Document::Silent
}

fn ui(store: &Store, env: &Env, clock: fn() -> u64) -> Result<Document, Failure> {
    let socket = client::socket_path(store);
    let exe = std::env::current_exe()?;
    client::ensure_server(&socket, &exe, &[])?;
    let ui_env = ui::UiEnv {
        exe,
        claude_home: env.claude_home(),
        home: env.user_home().map(Path::to_path_buf),
        store: store.root().to_path_buf(),
        theme_dir: env.config_dir().unwrap_or_else(|| PathBuf::from(".")),
    };
    let message = ui::run(&socket, ui_env, clock)?;
    Ok(Document::Text(message))
}

fn stop_server(socket: &Path) -> Result<Document, Failure> {
    let Some(mut control) = Client::connect(socket, Role::Control)? else {
        return Ok(Document::Text("no flow server runs".to_string()));
    };
    control.send(&ClientMsg::StopServer)?;
    while let Ok(Some(_)) = control.recv() {}
    Ok(Document::Text(
        "stopped the flow server and every session in it".to_string(),
    ))
}

fn server(store: Store, env: &Env, clock: fn() -> u64) -> Result<Document, Failure> {
    let config = env.config()?;
    let config_dir = env
        .config_dir()
        .unwrap_or_else(|| store.root().to_path_buf());
    let paths = server::ServerPaths {
        socket: client::socket_path(&store),
        master: store.root().join("master.json"),
        instructions: config_dir.join("master.md"),
    };
    let launch_env = launch::LaunchEnv {
        flow_home: env.flow_home.clone(),
        flow_exe: std::env::current_exe()?,
    };
    let cwd = std::env::current_dir().context("could not read the current directory")?;
    server::run(&paths, store, config, launch_env, &cwd, clock)?;
    Ok(Document::Text("flow server stopped".to_string()))
}
