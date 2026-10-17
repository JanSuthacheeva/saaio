use std::path::{Path, PathBuf};

use crate::cli::Env;
use crate::cli::output::{Document, Failure};
use crate::client;
use crate::store::Store;
use crate::ui;

#[derive(Debug, Default, clap::Args)]
pub struct Args {
    /// Stop the server and every session in it
    #[arg(long)]
    stop: bool,
}

/// Opens the terminal UI, starting the server first; --stop stops the server instead.
pub fn run(args: &Args, store: &Store, env: &Env, clock: fn() -> u64) -> Result<Document, Failure> {
    refuse_inside_own_flow(store, env)?;
    let socket = client::socket_path(store);
    if args.stop {
        return stop_server(&socket, clock);
    }
    let config = env.config()?;
    let exe = std::env::current_exe()?;
    client::ensure_server(&socket, &exe, &[])?;
    let ui_env = ui::UiEnv {
        agents: config
            .worker_agents()
            .into_iter()
            .map(String::from)
            .collect(),
        exe,
        claude_home: env.claude_home(),
        home: env.user_home().map(Path::to_path_buf),
        store: store.root().to_path_buf(),
        theme_dir: env.config_dir().unwrap_or_else(|| PathBuf::from(".")),
        prefix: config.ui.prefix,
    };
    let message = ui::run(&socket, ui_env, clock)?;
    Ok(Document::Text(message))
}

/// Err when $SAAIO_FLOW_ID names a flow in `store`: a worker's flow ui would take over the user's, and
/// its --stop would end every session in it.
fn refuse_inside_own_flow(store: &Store, env: &Env) -> Result<(), Failure> {
    let Some(id) = env.flow_id() else {
        return Ok(());
    };
    if !store.ids()?.iter().any(|known| known == id) {
        return Ok(());
    }
    Err(Failure::error(
        "flow ui runs inside a flow's worker, on the store that flow lives in: it would take over the user's flow ui",
        vec![
            "Set SAAIO_HOME to a scratch folder for a flow ui of your own: SAAIO_HOME=$(mktemp -d) flow ui"
                .to_string(),
        ],
    ))
}

fn stop_server(socket: &Path, clock: fn() -> u64) -> Result<Document, Failure> {
    let message = if client::stop_server(socket, clock)? {
        "stopped the flow server and every session in it"
    } else {
        "no flow server runs"
    };
    Ok(Document::Text(message.to_string()))
}
