use std::path::{Path, PathBuf};

use crate::cli::Env;
use crate::cli::output::{Document, Failure};
use crate::client::{self, Client};
use crate::proto::{ClientMsg, Role};
use crate::store::Store;
use crate::ui;

#[derive(Debug, clap::Args)]
pub struct Args {
    /// Stop the server and every session in it
    #[arg(long)]
    stop: bool,
}

/// Opens the terminal UI, starting the server first; --stop stops the server instead.
pub fn run(args: &Args, store: &Store, env: &Env, clock: fn() -> u64) -> Result<Document, Failure> {
    let socket = client::socket_path(store);
    if args.stop {
        return stop_server(&socket);
    }
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
