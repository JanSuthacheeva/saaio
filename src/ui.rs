//! The terminal UI bare `saaio` opens: its threads and the loop over input, server and ticks

pub mod app;
pub mod board;
pub mod clipboard;
pub mod dialog;
pub mod fit;
pub mod focus;
pub mod hints;
pub mod input;
pub mod layout;
pub mod markdown;
pub mod newflow;
pub mod outer;
pub mod peek;
pub mod rail;
pub mod screen;
pub mod selection;
pub mod sidebar;
pub mod theme;
pub mod theme_file;
mod update_watch;

use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::buffer::Buffer;
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{BeginSynchronizedUpdate, EndSynchronizedUpdate};
use ratatui::layout::Rect;
use signal_hook::consts::SIGWINCH;
use signal_hook::iterator::Signals;

use crate::branching::Branching;
use crate::client::Client;
use crate::config::prefix::Prefix;
use crate::launch::pane;
use crate::proto::{Role, ServerMsg};
use crate::release::source::Releases;
use crate::release::{Build, Tag};
use crate::server_log;
use crate::update::UpdateCheck;
use app::{App, Effect, Job, JobError};
use clipboard::Clipboard;
use input::{Router, Target};
use outer::Outer;
use peek::PeekCache;
use theme::Theme;
use theme_file::ThemeWatch;
use update_watch::UpdateWatch;

use crate::config::watch::ConfigWatch;

#[derive(Debug)]
enum Event {
    Input(Vec<u8>),
    Server(ServerMsg),
    ServerGone,
    Resize,
    Tick,
    Ran(Job, Result<String, JobError>),
    /// The latest release a look found, None when it found none.
    Latest(Option<Tag>),
}

/// How often spinners turn.
const TICK_EVERY: Duration = Duration::from_millis(100);
/// Ticks between two looks at the peeked transcript.
const TICKS_PER_PEEK: u32 = 10;
/// Ticks between two looks at theme.toml and the theme it extends, and at config.toml.
const TICKS_PER_THEME: u32 = 10;
/// Ticks between two looks at whether a day passed since the last look for a release: an hour.
const TICKS_PER_UPDATE: u32 = 36_000;

/// What saaio ui reads from where it runs: Claude's home for the peek, the user's home for ~.
#[derive(Debug)]
pub struct UiEnv {
    pub claude_home: PathBuf,
    pub home: Option<PathBuf>,
    /// This flow binary, for the flow commands the UI runs (start, remove, status --all).
    pub exe: PathBuf,
    /// This project's store, as `saaio status --all` names its root.
    pub store: PathBuf,
    /// Where theme.toml lives: config.toml's folder.
    pub theme_dir: PathBuf,
    /// config.toml, watched while the ui runs.
    pub config_file: Option<PathBuf>,
    /// server.log, where the UI says what failed.
    pub log: PathBuf,
    /// Where selected text goes.
    pub clipboard: Clipboard,
    /// update.json in the shared folder; None without one.
    pub update: Option<UpdateCheck>,
    /// Where the latest release is read.
    pub releases: Releases,
}

/// Attaches, then loops on stdin, server and resize events until detach; what to tell the user.
#[expect(
    clippy::too_many_lines,
    reason = "the one loop over every event the UI waits on"
)]
pub fn run(socket: &Path, env: UiEnv, clock: fn() -> u64) -> Result<String> {
    let mut outer = Outer::enter()?;
    // Before any thread reads stdin: creating the terminal asks for the cursor position.
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    terminal.clear()?;
    let home = env.home.map(|home| home.display().to_string());
    let mut app = App::new(outer.size()?)
        .with_offset(crate::text::local_offset_secs())
        .with_home(home)
        .with_store(env.store.display().to_string());
    let mut router = Router::new(Prefix::default());
    let mut configs = ConfigWatch::new(env.config_file);
    let mut updates = UpdateWatch::new(env.update, env.releases, Build::running());
    reload_config(&mut configs, &mut app, &mut router, &mut updates);
    let client = Client::connect(
        socket,
        Role::Attach {
            size: app.areas().pane_size(),
        },
    )?
    .context("no saaio server answers")?;
    let mut writer = client.try_clone()?;
    let (events, inbox) = mpsc::channel();
    let jobs = events.clone();
    let looks = events.clone();
    read_stdin(events.clone());
    read_server(client, events.clone());
    watch_resize(events.clone())?;
    tick(events);

    let mut peek = PeekCache::new(env.claude_home);
    let mut ticks: u32 = 0;
    let mut themes = ThemeWatch::new(env.theme_dir);
    let mut theme = Theme::default();
    reload_theme(&mut themes, &mut theme, &mut app, &env.log, clock);
    updates.ask_if_due(clock(), &looks);
    app.on_tick(clock());
    // What the screen shows, which a selection copies.
    let mut drawn = Buffer::empty(Rect::default());
    loop {
        let mut effects = Vec::new();
        let mut redraw = false;
        for event in next_batch(&inbox)? {
            redraw |= !matches!(event, Event::Tick);
            effects.extend(match event {
                Event::Input(bytes) => {
                    let modes = app.pane_modes();
                    let target = Target {
                        pane: app.areas().pane,
                        modal: app.is_modal(),
                        mouse: modes.mouse,
                        sgr: modes.sgr_mouse,
                    };
                    let mut effects = Vec::new();
                    for routed in router.route(&bytes, target) {
                        effects.extend(app.on_input(routed));
                    }
                    effects
                }
                Event::Server(message) => app.on_server(message),
                Event::ServerGone => vec![Effect::Quit("the saaio server stopped".to_string())],
                Event::Resize => {
                    let size = outer.size()?;
                    terminal.autoresize()?;
                    app.on_resize(size)
                }
                Event::Ran(job, result) => {
                    if let Err(error) = &result
                        && error.refusal.is_none()
                        && job != Job::Overview
                    {
                        let text = format!("ui: {} failed: {}", job.summary(), error.message);
                        log(&env.log, clock, &text);
                    }
                    app.on_ran(job, result);
                    vec![]
                }
                Event::Latest(latest) => {
                    updates.answered(clock(), latest);
                    app.set_update(updates.notice());
                    vec![]
                }
                Event::Tick => {
                    redraw |= app.on_tick(clock());
                    ticks = ticks.wrapping_add(1);
                    if ticks.is_multiple_of(TICKS_PER_PEEK) {
                        redraw |= peek.refresh(app.peeked());
                    }
                    if ticks.is_multiple_of(TICKS_PER_THEME) {
                        redraw |= reload_theme(&mut themes, &mut theme, &mut app, &env.log, clock);
                        redraw |= reload_config(&mut configs, &mut app, &mut router, &mut updates);
                    }
                    if ticks.is_multiple_of(TICKS_PER_UPDATE) {
                        // Another UI may have looked meanwhile.
                        redraw |= app.set_update(updates.notice());
                        updates.ask_if_due(clock(), &looks);
                    }
                    app.overview_due()
                }
            });
        }
        for effect in effects {
            match effect {
                Effect::Send(message) => writer.send(&message)?,
                Effect::Mirror(modes) => outer.mirror(modes)?,
                Effect::LoadBranching(repo) => {
                    let branching = Branching::load(&repo).map_err(|error| format!("{error:#}"));
                    app.on_branching(&repo, branching);
                }
                Effect::Run(job) => run_job(&env.exe, job, jobs.clone()),
                Effect::CopySelection(selection) => {
                    copy(&env.clipboard, &env.log, &selection.text(&drawn), clock);
                }
                Effect::Copy(text) => copy(&env.clipboard, &env.log, &text, clock),
                Effect::Quit(message) => return Ok(message),
            }
        }
        if redraw {
            peek.refresh(app.peeked());
            // The terminal holds the frame until it is complete, so it never shows half of one.
            execute!(terminal.backend_mut(), BeginSynchronizedUpdate)?;
            let frame = terminal.draw(|frame| app.draw(frame, &theme, peek.lines()))?;
            drawn = frame.buffer.clone();
            execute!(terminal.backend_mut(), EndSynchronizedUpdate)?;
        }
    }
}

/// Puts `text` on the clipboard; a failure goes to server.log, since nothing on screen asked.
fn copy(clipboard: &Clipboard, log_file: &Path, text: &str, clock: fn() -> u64) {
    if let Err(error) = clipboard.copy(&mut io::stdout(), text) {
        log(log_file, clock, &format!("ui: copy failed: {error:#}"));
    }
}

/// How a notice about the theme begins.
const THEME_NOTICE: &str = "theme: ";

/// Takes the theme again when its files changed; a broken one keeps the last, says why until it
/// reads again and logs it once. True when something changed on screen.
fn reload_theme(
    themes: &mut ThemeWatch,
    theme: &mut Theme,
    app: &mut App,
    log_file: &Path,
    clock: fn() -> u64,
) -> bool {
    match themes.changed() {
        None => false,
        Some(Ok(loaded)) => {
            *theme = loaded;
            app.take_back(THEME_NOTICE);
            true
        }
        Some(Err(error)) => {
            log(log_file, clock, &format!("ui: {THEME_NOTICE}{error:#}"));
            app.say_about_file(format!("{THEME_NOTICE}{error:#}"));
            true
        }
    }
}

/// Appends a line to server.log; one that cannot be written is let go.
fn log(file: &Path, clock: fn() -> u64, text: &str) {
    let _ = server_log::append(file, clock(), text);
}

/// How a notice about the config begins.
const CONFIG_NOTICE: &str = "config: ";

/// Takes config.toml again when it changed; a broken one keeps the last and says why until it
/// reads again. True when something changed on screen.
fn reload_config(
    configs: &mut ConfigWatch,
    app: &mut App,
    router: &mut Router,
    updates: &mut UpdateWatch,
) -> bool {
    match configs.changed() {
        None => false,
        Some(Ok(config)) => {
            router.set_prefix(config.ui.prefix);
            app.set_prefix(config.ui.prefix);
            app.set_agents(
                config
                    .worker_agents()
                    .into_iter()
                    .map(String::from)
                    .collect(),
            );
            updates.set_enabled(config.ui.update_check);
            app.set_update(updates.notice());
            app.take_back(CONFIG_NOTICE);
            true
        }
        Some(Err(error)) => {
            app.say_about_file(format!("{CONFIG_NOTICE}{error:#}"));
            true
        }
    }
}

/// Runs a flow command on its own thread; what it printed comes back as Event::Ran.
fn run_job(exe: &Path, job: Job, events: Sender<Event>) {
    let mut command = std::process::Command::new(exe);
    command
        .args(job.args())
        .env_remove("SAAIO_FLOW_ID")
        .env_remove(pane::PROOF_ENV)
        .stdin(std::process::Stdio::null());
    if let Some(dir) = job.dir() {
        command.current_dir(dir);
    }
    thread::spawn(move || {
        let result = match command.output() {
            Ok(output) if output.status.success() && job == Job::Overview => {
                Ok(String::from_utf8_lossy(&output.stdout).into_owned())
            }
            Ok(output) => app::outcome(
                output.status.success(),
                &String::from_utf8_lossy(&output.stdout),
            ),
            Err(error) => Err(JobError::other(format!("could not run flow: {error}"))),
        };
        let _ = events.send(Event::Ran(job, result));
    });
}

fn tick(events: Sender<Event>) {
    thread::spawn(move || {
        loop {
            thread::sleep(TICK_EVERY);
            if events.send(Event::Tick).is_err() {
                return;
            }
        }
    });
}

/// Blocks for one event, then takes every other one already waiting, so a burst draws once.
fn next_batch(inbox: &Receiver<Event>) -> Result<Vec<Event>> {
    let first = inbox.recv().context("the ui lost its event channel")?;
    let mut batch = vec![first];
    batch.extend(inbox.try_iter());
    Ok(batch)
}

fn read_stdin(events: Sender<Event>) {
    thread::spawn(move || {
        let mut stdin = io::stdin();
        let mut buf = [0u8; 4096];
        loop {
            match stdin.read(&mut buf) {
                Ok(0) | Err(_) => return,
                Ok(read) => {
                    if events.send(Event::Input(buf[..read].to_vec())).is_err() {
                        return;
                    }
                }
            }
        }
    });
}

fn read_server(mut client: Client, events: Sender<Event>) {
    thread::spawn(move || {
        while let Ok(Some(message)) = client.recv() {
            if events.send(Event::Server(message)).is_err() {
                return;
            }
        }
        let _ = events.send(Event::ServerGone);
    });
}

fn watch_resize(events: Sender<Event>) -> Result<()> {
    let mut signals = Signals::new([SIGWINCH]).context("could not watch for resizes")?;
    thread::spawn(move || {
        for _ in signals.forever() {
            if events.send(Event::Resize).is_err() {
                return;
            }
        }
    });
    Ok(())
}
