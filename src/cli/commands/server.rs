//! `saaio server`: the session server's paths and environment, run until it stops

use anyhow::Context as _;

use crate::cli::context::{Context, Opened};
use crate::cli::output::{Document, Failure};
use crate::launch::pane::LaunchEnv;
use crate::server::{self, ServerPaths, Timing};
use crate::server_log;
use crate::system::System;
use crate::trust::Trust;
use crate::ui::theme_file;

/// The session server saaio ui starts on its own; runs until StopServer or SIGTERM. Its stderr is
/// server.log, so it answers nothing on success and logs why it failed.
pub fn run(ctx: &Context) -> Result<Document, Failure> {
    let Opened { store, .. } = ctx.open()?;
    server_log::exit_on_panic(ctx.clock);
    // A session of its own, so closing the terminal saaio ui started it from does not hang it up.
    // A server a shell started as a job leads its process group already and keeps its session.
    let _ = rustix::process::setsid();
    let config = ctx.env.config()?;
    let config_file = ctx.env.config_file();
    let theme = ctx
        .env
        .config_dir()
        .map(|dir| dir.join(theme_file::THEME_FILE));
    let config_dir = ctx
        .env
        .config_dir()
        .unwrap_or_else(|| store.root().to_path_buf());
    let paths = ServerPaths {
        socket: ctx.env.socket(&store),
        master: store.root().join("master.json"),
        instructions: config_dir.join("master.md"),
        config: config_file,
        theme,
        cwd: std::env::current_dir().context("could not read the current directory")?,
        trust: ctx.env.registry().as_ref().map(Trust::at),
    };
    let launch_env = LaunchEnv {
        store: store.root().to_path_buf(),
        flow_exe: std::env::current_exe()?,
    };
    let system = System::probe();
    if let Err(error) = server::run(
        &paths,
        store,
        config,
        launch_env,
        &system,
        Timing::default(),
        ctx.clock,
    ) {
        server_log::write(ctx.now(), &format!("failed: {error:#}"));
        return Err(error.into());
    }
    Ok(Document::Silent)
}
