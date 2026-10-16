use anyhow::Context;

use crate::cli::Env;
use crate::cli::output::{Document, Failure};
use crate::client;
use crate::launch::pane::LaunchEnv;
use crate::server::{self, ServerPaths};
use crate::store::Store;

/// The session server flow ui starts on its own; runs until StopServer.
pub fn run(store: Store, env: &Env, clock: fn() -> u64) -> Result<Document, Failure> {
    let config = env.config()?;
    let config_dir = env
        .config_dir()
        .unwrap_or_else(|| store.root().to_path_buf());
    let paths = ServerPaths {
        socket: client::socket_path(&store),
        master: store.root().join("master.json"),
        instructions: config_dir.join("master.md"),
    };
    let launch_env = LaunchEnv {
        flow_home: env.flow_home.clone(),
        flow_exe: std::env::current_exe()?,
    };
    let cwd = std::env::current_dir().context("could not read the current directory")?;
    server::run(&paths, store, config, launch_env, &cwd, clock)?;
    Ok(Document::Text("flow server stopped".to_string()))
}
