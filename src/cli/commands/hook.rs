//! `saaio hook`: one harness event into its flow or the master's turn, news handed over at Stop

use std::fmt;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use serde_json::Value;

use super::{of_own_repo, repo_dir, views};
use crate::adapter::{Adapter, LineMatch, Turn};
use crate::cli::Env;
use crate::cli::output::Document;
use crate::config::Config;
use crate::config::kind::DEFAULT_KIND;
use crate::deliver::{self, WorkerNews};
use crate::event::{Event, EventKind, ServiceStatus, is_uuid};
use crate::fold;
use crate::identity;
use crate::pause;
use crate::server_log;
use crate::store::{Busy, Store, Waiter};
use crate::view::{FlowView, MasterTurn, State};
use crate::wake;
use crate::workflow::Workflow;

#[derive(Debug, clap::Args)]
pub struct Args {
    /// The harness: claude-code or codex
    adapter: String,
}

/// Whose hook ran: the master's ($SAAIO_MASTER), a flow's session ($SAAIO_FLOW_ID), or neither.
#[derive(Debug)]
enum Hooked {
    Flow(String),
    Master,
    Nobody,
}

impl Hooked {
    /// The session it claims to be, None for Nobody.
    fn waiter(&self) -> Option<Waiter> {
        match self {
            Hooked::Flow(id) => Some(Waiter::Flow(id.clone())),
            Hooked::Master => Some(Waiter::Master),
            Hooked::Nobody => None,
        }
    }
}

/// the master, flow 7f3c, nobody
impl fmt::Display for Hooked {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Hooked::Flow(id) => write!(f, "flow {id}"),
            Hooked::Master => f.write_str("the master"),
            Hooked::Nobody => f.write_str("nobody"),
        }
    }
}

/// `saaio hook`: never prints to stdout (a hook's stdout can reach the model) and never fails (a
/// failing hook shows the user a notice); what goes wrong goes to the store's server.log. A Stop
/// that hands news over wakes the session with it (exit 2, the news on stderr).
pub fn run(args: &Args, env: &Env, clock: fn() -> u64) -> Document {
    let cwd = std::env::current_dir().ok();
    // Without a store there is no server.log to say it in.
    let Ok(Some(store)) = env.store(cwd.as_deref()) else {
        return Document::Silent;
    };
    let hooked = match env.flow_id() {
        _ if env.is_master() => Hooked::Master,
        Some(id) if !is_uuid(id) => {
            Log::new(
                env.server_log(&store),
                &args.adapter,
                &Hooked::Nobody,
                clock,
            )
            .error(&format!("SAAIO_FLOW_ID {id:?} is no flow id"));
            return Document::Silent;
        }
        Some(id) => Hooked::Flow(id.to_string()),
        None => Hooked::Nobody,
    };
    let log = Log::new(env.server_log(&store), &args.adapter, &hooked, clock);
    if hooked.waiter().is_some() && env.key().is_none() {
        log.error("no SAAIO_PROOF: an earlier saaio server opened this pane; saaio --stop");
        return Document::Silent;
    }
    if let Some(claimed) = hooked.waiter()
        && !identity::proves(&store, &claimed, env.key())
    {
        log.error(&format!("SAAIO_PROOF does not prove {hooked}"));
        return Document::Silent;
    }
    // A broken config must not stop running workers from reporting; the server logs it, start and
    // ui say it loudly.
    let config = env.config().unwrap_or_else(|_| Config::builtin());
    let Some(adapter) = Adapter::named(&args.adapter) else {
        log.error(&format!("no adapter named {}", args.adapter));
        return Document::Silent;
    };
    let text = match read_event(std::io::stdin().lock()) {
        Ok(text) => text,
        Err(error) => {
            log.error(&format!("{error:#}"));
            return Document::Silent;
        }
    };
    let payload: Value = match serde_json::from_str(&text) {
        Ok(payload) => payload,
        Err(error) => {
            log.error(&format!("the event is not JSON: {error}"));
            return Document::Silent;
        }
    };
    let news = hook(&store, &config, adapter, &hooked, &payload, &log, clock());
    if let Hooked::Flow(id) = &hooked
        && adapter.before_tool(&payload)
    {
        wait_for_services(&store, id, !adapter.asks(&payload), &log, clock);
    }
    match news {
        Some(news) => Document::Wake(news),
        None => Document::Silent,
    }
}

/// The most of an event saaio hook reads.
const MAX_EVENT: u64 = 16 << 20;

/// The event `input` carries; Err when it is past MAX_EVENT, not UTF-8, or cannot be read.
fn read_event(input: impl Read) -> Result<String> {
    let mut bytes = Vec::new();
    input
        .take(MAX_EVENT + 1)
        .read_to_end(&mut bytes)
        .context("could not read the event")?;
    if bytes.len() as u64 > MAX_EVENT {
        bail!("the event is over {} MiB", MAX_EVENT >> 20);
    }
    String::from_utf8(bytes).map_err(|_| anyhow!("the event is not UTF-8"))
}

/// How long a tool call waits for services to come back.
const RESUME_WAIT: Duration = Duration::from_mins(2);
/// How long a tool call waits for the server to start a resume of paused services.
const PICKUP_WAIT: Duration = Duration::from_secs(2);
/// How often the wait reads the flow's log.
const WAIT_STEP: Duration = Duration::from_millis(200);

/// Before a tool call of a flow whose services pause::holds_tools: waits until they run again,
/// fail, or the wait is over; logs a wait that ran out. A call that `works` (does not ask the user)
/// marks a resting flow working first (a worker the master woke reports no prompt), so the server
/// begins the resume. Paused with no resume begun, it waits only PICKUP_WAIT, so a store without a
/// server never stalls tools.
fn wait_for_services(store: &Store, id: &str, works: bool, log: &Log, clock: fn() -> u64) {
    let deadline = Instant::now() + RESUME_WAIT;
    let mut paused_since = None;
    loop {
        let Ok(events) = store.events(id) else {
            return;
        };
        let Some(flow) = fold::fold(id, &events, None, clock()) else {
            return;
        };
        if !pause::holds_tools(&flow) {
            return;
        }
        if works
            && pause::wakes_at_tool_call(&flow)
            && let Err(error) = store.append(id, &Event::new(clock(), EventKind::Working))
        {
            log.error(&format!("{error:#}"));
            return;
        }
        let paused = flow
            .services
            .is_some_and(|services| services.status == ServiceStatus::Paused);
        if paused {
            let since = *paused_since.get_or_insert_with(Instant::now);
            if since.elapsed() >= PICKUP_WAIT {
                log.error("services paused and no resume began; the tool runs");
                return;
            }
        } else {
            paused_since = None;
        }
        if Instant::now() >= deadline {
            log.error("services not back within 2m; the tool runs");
            return;
        }
        thread::sleep(WAIT_STEP);
    }
}

/// Where `saaio hook` says what went wrong: the store's server.log, never stdout or stderr (a
/// hook's output reaches the harness, and the model).
#[derive(Debug)]
struct Log {
    path: PathBuf,
    /// `hook claude-code flow 3f2a9c` or `hook codex master`.
    source: String,
    clock: fn() -> u64,
}

impl Log {
    fn new(path: PathBuf, adapter: &str, hooked: &Hooked, clock: fn() -> u64) -> Log {
        let source = match hooked {
            Hooked::Flow(flow) => format!("hook {adapter} flow {flow}"),
            Hooked::Master => format!("hook {adapter} master"),
            Hooked::Nobody => format!("hook {adapter}"),
        };
        Log {
            path,
            source,
            clock,
        }
    }

    /// Appends `<source>: <text>`; a log that cannot be written is let go.
    fn error(&self, text: &str) {
        let line = format!("{}: {text}", self.source);
        let _ = server_log::append(&self.path, (self.clock)(), &line);
    }
}

/// Translates one hook event and appends what changes the flow; marks the session busy or idle
/// and, at the Stop of one that does not wake itself, takes the news it is handed.
fn hook(
    store: &Store,
    config: &Config,
    adapter: Adapter,
    hooked: &Hooked,
    payload: &Value,
    log: &Log,
    now: u64,
) -> Option<String> {
    let approver = adapter.approver(payload, |file, matches| rollout_line(file, matches, log));
    let kinds = adapter.translate(payload, &workflow_for(store, hooked, config, now), approver);
    let waiter = match hooked {
        Hooked::Nobody => return None,
        Hooked::Master => {
            record_master_session(store, &kinds, log);
            record_master_turn(store, &kinds, log, now);
            Waiter::Master
        }
        Hooked::Flow(flow) => {
            let id = report(store, flow, kinds, log, now)?;
            Waiter::Flow(id)
        }
    };
    match adapter.turn(payload)? {
        Turn::Began => mark_busy(store, &waiter, log, Busy::Turn),
        Turn::Idle => mark_busy(store, &waiter, log, Busy::Idle),
        Turn::Ended if adapter.wakes_itself() => mark_busy(store, &waiter, log, Busy::Idle),
        Turn::Ended => {
            let wakes = !matches!(waiter, Waiter::Master) || config.master.wake;
            let news = if wakes {
                hand_over(store, &waiter, log, now)
            } else {
                None
            };
            if news.is_none() {
                mark_busy(store, &waiter, log, Busy::Idle);
            }
            return news;
        }
    }
    None
}

/// The last line of a rollout that `matches`; a read that fails goes to server.log and counts as
/// none.
fn rollout_line(file: &Path, matches: &LineMatch, log: &Log) -> Option<String> {
    match last_line(file, matches, ROLLOUT_CHUNK) {
        Ok(line) => line,
        Err(error) => {
            log.error(&format!(
                "could not read the rollout {}: {error}",
                file.display()
            ));
            None
        }
    }
}

/// How much of a rollout is read at a time, from its end back.
const ROLLOUT_CHUNK: u64 = 64 * 1024;

/// The last line of `file` that `matches`, read back from the end `chunk` bytes at a time, so a
/// line near the end costs only the bytes after it.
fn last_line(file: &Path, matches: &LineMatch, chunk: u64) -> std::io::Result<Option<String>> {
    let mut reader = File::open(file)?;
    let mut end = reader.metadata()?.len();
    // The bytes from `end` up to the first line already searched: a line cut by a chunk start.
    let mut rest = Vec::new();
    while end > 0 {
        let start = end.saturating_sub(chunk);
        let mut bytes = vec![0; (end - start) as usize];
        reader.seek(SeekFrom::Start(start))?;
        reader.read_exact(&mut bytes)?;
        bytes.extend_from_slice(&rest);
        let whole = match bytes.iter().position(|byte| *byte == b'\n') {
            _ if start == 0 => 0,
            Some(newline) => newline + 1,
            None => bytes.len(),
        };
        let found = bytes[whole..]
            .split(|byte| *byte == b'\n')
            .rev()
            .filter_map(|line| std::str::from_utf8(line).ok())
            .find(|line| matches(line));
        if let Some(line) = found {
            return Ok(Some(line.to_string()));
        }
        bytes.truncate(whole);
        rest = bytes;
        end = start;
    }
    Ok(None)
}

/// The workflow the hooked flow recorded; for one from before kinds, which was a feature flow, the
/// feature kind's; for the master and nobody the config's default kind's.
fn workflow_for(store: &Store, hooked: &Hooked, config: &Config, now: u64) -> Workflow {
    let Hooked::Flow(flow) = hooked else {
        return config.workflow();
    };
    let recorded = if let Ok(events) = store.events(flow) {
        fold::fold(flow, &events, None, now).and_then(|view| view.workflow)
    } else {
        let (flows, _) = views(store, now);
        fold::resolve(&flows, flow).and_then(|view| view.workflow.clone())
    };
    recorded.unwrap_or_else(|| {
        config
            .kind(Some(DEFAULT_KIND))
            .map_or_else(Workflow::builtin, |(_, kind)| kind.phases.clone())
    })
}

/// Appends the kinds that change flow `flow` (a name or id); its id, None when there is no such
/// flow or its log cannot be read.
fn report(store: &Store, flow: &str, kinds: Vec<EventKind>, log: &Log, now: u64) -> Option<String> {
    let id = if store.events(flow).is_ok() {
        flow.to_string()
    } else {
        let (flows, _) = views(store, now);
        let Some(view) = fold::resolve(&flows, flow) else {
            log.error(&format!("no flow {flow}"));
            return None;
        };
        view.id.clone()
    };
    for kind in kinds {
        if let EventKind::MergeRequest { url } = &kind
            && !of_flow_repo(store, &id, url, now)
        {
            log.error(&format!(
                "{url} is not a merge request of this flow's repository"
            ));
            continue;
        }
        let events = match store.events(&id) {
            Ok(events) => events,
            Err(error) => {
                log.error(&format!("{error:#}"));
                return None;
            }
        };
        if fold::changes(&id, &events, &kind, now)
            && let Err(error) = store.append(&id, &Event::new(now, kind))
        {
            log.error(&format!("{error:#}"));
        }
    }
    Some(id)
}

/// True when `url` is a merge request of the flow's own repository.
fn of_flow_repo(store: &Store, id: &str, url: &str, now: u64) -> bool {
    let events = store.events(id).unwrap_or_default();
    fold::fold(id, &events, None, now)
        .as_ref()
        .and_then(repo_dir)
        .is_some_and(|dir| of_own_repo(&dir, url))
}

/// The news a session's Stop hands over: a worker's messages from the master and saaio's notes (its flow working
/// again, since the session goes on with them), the flows that stopped for the master.
fn hand_over(store: &Store, waiter: &Waiter, log: &Log, now: u64) -> Option<String> {
    match waiter {
        Waiter::Flow(id) => {
            let WorkerNews::News(notes) = deliver::worker_news(store, id, now) else {
                return None;
            };
            if let Err(error) = store.append(id, &Event::new(now, EventKind::Working)) {
                log.error(&format!("{error:#}"));
            }
            Some(wake::for_worker(&notes))
        }
        Waiter::Master => {
            let changed = deliver::master_news_now(store, &views(store, now).0);
            let changed: Vec<&FlowView> = changed.iter().collect();
            (!changed.is_empty()).then(|| wake::for_master(&changed))
        }
    }
}

/// Keeps the master's session id, which its harness picked, so the server can resume it.
fn record_master_session(store: &Store, kinds: &[EventKind], log: &Log) {
    let session = kinds.iter().find_map(|kind| match kind {
        EventKind::Session { id, .. } => Some(id),
        _ => None,
    });
    if let Some(id) = session
        && store.master_session().as_ref() != Some(id)
        && let Err(error) = store.set_master_session(id)
    {
        log.error(&format!("{error:#}"));
    }
}

/// Keeps the state the master's kinds put it in, with `now`: always when it comes to rest (a
/// turn woken by `saaio wait` runs no prompt hook, so Idle can follow Idle), else when it differs
/// from the one kept: tool calls repeat Working, and an unchanged file keeps the server from
/// resending.
fn record_master_turn(store: &Store, kinds: &[EventKind], log: &Log, now: u64) {
    let Some(state) = fold::master_state(kinds) else {
        return;
    };
    let busy = matches!(state, State::Working | State::Delegated);
    if busy && store.master_turn().map(|turn| turn.state) == Some(state) {
        return;
    }
    if let Err(error) = store.set_master_turn(&MasterTurn { state, at: now }) {
        log.error(&format!("{error:#}"));
    }
}

fn mark_busy(store: &Store, waiter: &Waiter, log: &Log, busy: Busy) {
    if let Err(error) = store.set_busy(waiter, busy) {
        log.error(&format!("{error:#}"));
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;

    #[test]
    fn finds_the_last_matching_line_across_chunks() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("rollout.jsonl");
        let lines: Vec<String> = (0..500).map(|n| format!("line {n:03}")).collect();
        fs::write(&file, lines.join("\n") + "\n").unwrap();

        for chunk in [1, 7, 64, 1 << 20] {
            let last_with =
                |digit: char| last_line(&file, &move |line| line.ends_with(digit), chunk).unwrap();
            assert_eq!(last_with('7').as_deref(), Some("line 497"), "{chunk}");
            let first = last_line(&file, &|line| line == "line 000", chunk).unwrap();
            assert_eq!(first.as_deref(), Some("line 000"), "{chunk}");
            assert_eq!(
                last_line(&file, &|line| line == "line", chunk).unwrap(),
                None
            );
        }
    }

    #[test]
    fn a_rollout_without_its_last_newline_still_ends_in_a_line() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("rollout.jsonl");
        fs::write(&file, "first\nsecond").unwrap();

        let last = last_line(&file, &|line| !line.is_empty(), 4).unwrap();

        assert_eq!(last.as_deref(), Some("second"));
        assert!(last_line(&dir.path().join("missing"), &|_| true, 4).is_err());
    }

    #[test]
    fn logs_an_error_with_its_source() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("server.log");
        let log = Log::new(
            path.clone(),
            "codex",
            &Hooked::Flow("3f2a9c".to_string()),
            || 0,
        );

        log.error("no flow 3f2a9c");
        Log::new(path, "codex", &Hooked::Master, || 0).error("broken");

        let lines = fs::read_to_string(dir.path().join("server.log")).unwrap();
        let ends: Vec<_> = lines
            .lines()
            .map(|line| line.split_once("] ").unwrap().1)
            .collect();
        assert_eq!(
            ends,
            [
                "hook codex flow 3f2a9c: no flow 3f2a9c",
                "hook codex master: broken"
            ]
        );
    }
}
