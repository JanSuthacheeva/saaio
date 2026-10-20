use std::path::{Path, PathBuf};

use crate::cli::Env;
use crate::cli::output::{Document, Failure};
use crate::client;
use crate::server_log;
use crate::store::Store;
use crate::ui;

/// Opens the terminal UI, starting the server first; what fails, a panic too, goes to server.log.
pub fn run(store: &Store, env: &Env, clock: fn() -> u64) -> Result<Document, Failure> {
    // A broken config.toml stops the UI here: there is no last good one to keep yet.
    env.config()?;
    let log = client::log_path(store);
    server_log::append_on_panic(log.clone(), "ui", clock);
    match attach(store, env, log.clone(), clock) {
        Ok(message) => Ok(Document::Text(message)),
        Err(error) => {
            let _ = server_log::append(&log, clock(), &format!("ui: failed: {error:#}"));
            Err(error.into())
        }
    }
}

/// Starts the server unless one runs, then runs the UI until it detaches; what to tell the user.
fn attach(store: &Store, env: &Env, log: PathBuf, clock: fn() -> u64) -> anyhow::Result<String> {
    let socket = client::socket_path(store);
    let exe = std::env::current_exe()?;
    let pinned = [("SAAIO_HOME".to_string(), store.root().display().to_string())];
    client::ensure_server(&socket, &exe, &pinned)?;
    let ui_env = ui::UiEnv {
        exe,
        claude_home: env.claude_home(),
        home: env.user_home().map(Path::to_path_buf),
        store: store.root().to_path_buf(),
        theme_dir: env.config_dir().unwrap_or_else(|| PathBuf::from(".")),
        config_file: env.config_file(),
        log,
        clipboard: env.clipboard(),
    };
    ui::run(&socket, ui_env, clock)
}
