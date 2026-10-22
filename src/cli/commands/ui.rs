//! Bare `saaio` in a person's terminal: starts the server and runs the terminal UI

use std::path::{Path, PathBuf};

use crate::cli::Env;
use crate::cli::output::{Document, Failure};
use crate::client;
use crate::server_log;
use crate::store::Store;
use crate::trust::Trust;
use crate::ui;
use crate::update::UpdateCheck;

/// Opens the terminal UI, starting the server first; what fails, a panic too, goes to server.log.
pub fn run(store: &Store, env: &Env, clock: fn() -> u64) -> Result<Document, Failure> {
    // A broken config.toml stops the UI here: there is no last good one to keep yet.
    env.config()?;
    let log = env.server_log(store);
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
    let socket = env.socket(store);
    if let Some(earlier) = client::earlier_socket(store).filter(|earlier| *earlier != socket) {
        client::refuse_earlier(&earlier)?;
    }
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
        update: env
            .registry()
            .map(|registry| UpdateCheck::in_folder(registry.root())),
        releases: env.releases(),
        trust: env.registry().as_ref().map(Trust::at),
    };
    ui::run(&socket, ui_env, clock)
}
