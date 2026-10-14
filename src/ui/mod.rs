pub mod app;
pub mod input;
pub mod layout;
pub mod outer;
pub mod screen;
pub mod theme;

use std::io::{self, Read};
use std::path::Path;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use anyhow::{Context, Result};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use signal_hook::consts::SIGWINCH;
use signal_hook::iterator::Signals;

use crate::client::Client;
use crate::proto::{Role, ServerMsg};
use app::{App, Effect};
use input::Router;
use outer::Outer;
use theme::Theme;

enum Event {
    Input(Vec<u8>),
    Server(ServerMsg),
    ServerGone,
    Resize,
}

/// Attaches, then loops on stdin, server and resize events until detach; what to tell the user.
pub fn run(socket: &Path, theme: Theme) -> Result<String> {
    let mut outer = Outer::enter()?;
    let mut app = App::new(outer.size()?);
    let client = Client::connect(
        socket,
        Role::Attach {
            size: app.areas().pane_size(),
        },
    )?
    .context("no flow server answers")?;
    let mut writer = client.try_clone()?;
    let (events, inbox) = mpsc::channel();
    read_stdin(events.clone());
    read_server(client, events.clone());
    watch_resize(events)?;

    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    terminal.clear()?;
    let mut router = Router::default();
    loop {
        let mut effects = Vec::new();
        for event in next_batch(&inbox)? {
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
            });
        }
        for effect in effects {
            match effect {
                Effect::Send(message) => writer.send(&message)?,
                Effect::Mirror(modes) => outer.mirror(&modes)?,
                Effect::Quit(message) => return Ok(message),
            }
        }
        terminal.draw(|frame| app.draw(frame, &theme))?;
    }
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
