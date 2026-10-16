pub mod app;
pub mod board;
pub mod input;
pub mod layout;
pub mod newflow;
pub mod outer;
pub mod peek;
pub mod rail;
pub mod screen;
pub mod sidebar;
pub mod theme;
pub mod theme_file;

use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{BeginSynchronizedUpdate, EndSynchronizedUpdate};
use signal_hook::consts::SIGWINCH;
use signal_hook::iterator::Signals;

use crate::branching::Branching;
use crate::client::Client;
use crate::proto::{Role, ServerMsg};
use app::{App, Effect, Job};
use input::Router;
use outer::Outer;
use peek::PeekCache;
use theme::Theme;
use theme_file::ThemeWatch;

#[derive(Debug)]
enum Event {
    Input(Vec<u8>),
    Server(ServerMsg),
    ServerGone,
    Resize,
    Tick,
    Ran(Job, Result<String, String>),
}

/// How often spinners turn.
const TICK_EVERY: Duration = Duration::from_millis(100);
/// Ticks between two looks at the peeked transcript.
const TICKS_PER_PEEK: u32 = 10;
/// Ticks between two looks at theme.toml and the theme it extends.
const TICKS_PER_THEME: u32 = 10;

/// What flow ui reads from where it runs: Claude's home for the peek, the user's home for ~.
#[derive(Debug)]
pub struct UiEnv {
    pub claude_home: PathBuf,
    pub home: Option<PathBuf>,
    /// This flow binary, for the flow commands the UI runs (start, remove, status --all).
    pub exe: PathBuf,
    /// This project's store, as `flow status --all` names its root.
    pub store: PathBuf,
    /// Where theme.toml lives: config.toml's folder.
    pub theme_dir: PathBuf,
}

/// Attaches, then loops on stdin, server and resize events until detach; what to tell the user.
pub fn run(socket: &Path, env: UiEnv) -> Result<String> {
    let mut outer = Outer::enter()?;
    // Before any thread reads stdin: creating the terminal asks for the cursor position.
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    terminal.clear()?;
    let home = env.home.map(|home| home.display().to_string());
    let mut app = App::new(outer.size()?)
        .with_home(home)
        .with_store(env.store.display().to_string());
    let client = Client::connect(
        socket,
        Role::Attach {
            size: app.areas().pane_size(),
        },
    )?
    .context("no flow server answers")?;
    let mut writer = client.try_clone()?;
    let (events, inbox) = mpsc::channel();
    let jobs = events.clone();
    read_stdin(events.clone());
    read_server(client, events.clone());
    watch_resize(events.clone())?;
    tick(events);

    let mut router = Router::default();
    let mut peek = PeekCache::new(env.claude_home);
    let mut ticks: u32 = 0;
    let mut themes = ThemeWatch::new(env.theme_dir);
    let mut theme = Theme::default();
    reload_theme(&mut themes, &mut theme, &mut app);
    app.on_tick(now_ms());
    loop {
        let mut effects = Vec::new();
        let mut redraw = false;
        for event in next_batch(&inbox)? {
            redraw |= !matches!(event, Event::Tick);
            effects.extend(match event {
                Event::Input(bytes) => {
                    let pane = app.areas().pane;
                    let modal = app.is_modal();
                    let mut effects = Vec::new();
                    for routed in router.route(&bytes, modal, pane) {
                        effects.extend(app.on_input(routed));
                    }
                    effects
                }
                Event::Server(message) => app.on_server(message),
                Event::ServerGone => vec![Effect::Quit("the flow server stopped".to_string())],
                Event::Resize => {
                    let size = outer.size()?;
                    terminal.autoresize()?;
                    app.on_resize(size)
                }
                Event::Ran(job, result) => {
                    app.on_ran(job, result);
                    vec![]
                }
                Event::Tick => {
                    redraw |= app.on_tick(now_ms());
                    ticks = ticks.wrapping_add(1);
                    if ticks.is_multiple_of(TICKS_PER_PEEK) {
                        redraw |= peek.refresh(app.peeked());
                    }
                    if ticks.is_multiple_of(TICKS_PER_THEME) {
                        redraw |= reload_theme(&mut themes, &mut theme, &mut app);
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
                Effect::Quit(message) => return Ok(message),
            }
        }
        if redraw {
            peek.refresh(app.peeked());
            // The terminal holds the frame until it is complete, so it never shows half of one.
            execute!(terminal.backend_mut(), BeginSynchronizedUpdate)?;
            terminal.draw(|frame| app.draw(frame, &theme, peek.lines()))?;
            execute!(terminal.backend_mut(), EndSynchronizedUpdate)?;
        }
    }
}

/// Takes the theme again when its files changed; a broken one keeps the last and says why. True when
/// something changed on screen.
fn reload_theme(themes: &mut ThemeWatch, theme: &mut Theme, app: &mut App) -> bool {
    match themes.changed() {
        None => false,
        Some(Ok(loaded)) => {
            *theme = loaded;
            true
        }
        Some(Err(error)) => {
            app.say_about_file(format!("theme: {error:#}"));
            true
        }
    }
}

/// Runs a flow command on its own thread; what it printed comes back as Event::Ran.
fn run_job(exe: &Path, job: Job, events: Sender<Event>) {
    let mut command = std::process::Command::new(exe);
    command
        .args(job.args())
        .env_remove("FLOW_ID")
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
            Err(error) => Err(format!("could not run flow: {error}")),
        };
        let _ = events.send(Event::Ran(job, result));
    });
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_millis() as u64)
        .unwrap_or_default()
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
        loop {
            match client.recv() {
                Ok(Some(message)) => {
                    if events.send(Event::Server(message)).is_err() {
                        return;
                    }
                }
                Ok(None) | Err(_) => {
                    let _ = events.send(Event::ServerGone);
                    return;
                }
            }
        }
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
