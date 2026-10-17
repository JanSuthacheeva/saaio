use anyhow::Context;

use crate::cli::Env;
use crate::cli::output::{Document, Failure};
use crate::client;
use crate::config;
use crate::launch::pane::LaunchEnv;
use crate::server::{self, ServerPaths};
use crate::server_log;
use crate::store::Store;
use crate::ui::theme_file;

/// The session server flow ui starts on its own; runs until StopServer or SIGTERM. Its stderr is
/// server.log, so it answers nothing on success and logs why it failed.
pub fn run(store: Store, env: &Env, clock: fn() -> u64) -> Result<Document, Failure> {
    server_log::exit_on_panic(clock);
    let config = env.config()?;
    let config_file = config::config_file(
        env.flow_config.clone(),
        env.xdg_config_home.clone(),
        env.home.clone(),
    );
    let theme = env.config_dir().map(|dir| dir.join(theme_file::THEME_FILE));
    let config_dir = env
        .config_dir()
        .unwrap_or_else(|| store.root().to_path_buf());
    let paths = ServerPaths {
        socket: client::socket_path(&store),
        master: store.root().join("master.json"),
        instructions: config_dir.join("master.md"),
        config: config_file,
        theme,
    };
    let launch_env = LaunchEnv {
        flow_home: env.flow_home.clone(),
        flow_exe: std::env::current_exe()?,
    };
    let cwd = std::env::current_dir().context("could not read the current directory")?;
    if let Err(error) = server::run(&paths, store, config, launch_env, &cwd, clock) {
        server_log::write(clock(), &format!("failed: {error:#}"));
        return Err(error.into());
    }
    Ok(Document::Silent)
}
