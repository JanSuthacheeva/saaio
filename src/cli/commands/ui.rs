//! Bare `saaio` in a person's terminal: starts the server and runs the terminal UI

use std::path::{Path, PathBuf};

use crate::cli::Env;
use crate::cli::context::{Context, Opened};
use crate::cli::output::{Document, Failure};
use crate::client;
use crate::server_log;
use crate::store::Store;
use crate::trust::Trust;
use crate::ui;
use crate::update::UpdateCheck;

/// Opens the terminal UI, starting the server first; what fails, a panic too, goes to server.log.
pub fn run(ctx: &Context) -> Result<Document, Failure> {
    let Opened { store, .. } = ctx.open()?;
    // A broken config.toml stops the UI here: there is no last good one to keep yet.
    ctx.env.config()?;
    let log = ctx.env.server_log(&store);
    server_log::append_and_exit_on_panic(log.clone(), "ui", ctx.clock);
    match attach(&store, ctx.env, log.clone(), ctx.clock) {
        Ok(message) => Ok(Document::Text(message)),
        Err(error) => {
            let _ = server_log::append(&log, ctx.now(), &format!("ui: failed: {error:#}"));
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
        codex_home: env.codex_home(),
        home: env.user_home().map(Path::to_path_buf),
        store: store.root().to_path_buf(),
        theme_dir: env.config_dir().unwrap_or_else(|| PathBuf::from(".")),
        config_file: env.config_file(),
        log,
        clipboard: env.clipboard(),
        notifier: env.notifier(),
        update: env
            .registry()
            .map(|registry| UpdateCheck::in_folder(registry.root())),
        releases: env.releases(),
        trust: env.registry().as_ref().map(Trust::at),
        path: env.path.clone(),
    };
    ui::run(&socket, ui_env, clock)
}
