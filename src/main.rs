mod adapter;
mod branching;
mod client;
mod config;
mod event;
mod fold;
mod launch;
mod output;
mod proto;
mod registry;
mod server;
mod slots;
mod store;
mod ui;
mod view;
mod workspace;

use std::env;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use clap::error::ErrorKind;
use clap::{Parser, Subcommand};

use client::{Client, Closing};
use config::Config;
use event::{Event, EventKind, Mode, Phase};
use output::{Document, Failure, Opening};
use proto::{ClientMsg, Role};
use registry::Registry;
use slots::{Claim, Slots};
use store::Store;
use view::FlowView;
use workspace::Hook;

/// How long `flow remove` and `flow clear` wait for a flow's session to end (SIGKILL comes after 3 s).
const CLOSE_WAIT: std::time::Duration = std::time::Duration::from_secs(5);

/// Tracks parallel feature flows: what each is doing, where it is blocked, how far it got.
#[derive(Debug, Parser)]
#[command(version, about)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Track a new flow, or return the unfinished flow of that name
    New {
        #[arg(long)]
        name: String,
        #[arg(long)]
        request: String,
        #[arg(long, value_enum, default_value = "guided")]
        mode: Mode,
    },
    /// Append one event to a flow
    Report {
        /// Flow name or id [default: $FLOW_ID]
        #[arg(long, env = "FLOW_ID")]
        flow: Option<String>,
        #[command(subcommand)]
        event: ReportEvent,
    },
    /// Every flow, or one flow with its phases, timeline and decisions
    Status {
        /// Flow name or id
        flow: Option<String>,
        /// Print the versioned JSON programs read, instead of TOON
        #[arg(long)]
        json: bool,
        /// Do not truncate long texts
        #[arg(long)]
        full: bool,
        /// Every project's flows, from every registered store
        #[arg(long, conflicts_with = "flow")]
        all: bool,
    },
    /// Start a worker: a flow, a worktree, the setup hook and an agent flow ui opens
    Start {
        /// What to build, e.g. "SHOP-217 add CSV export"
        request: String,
        /// Flow name [default: from the ticket key or the first words]
        #[arg(long)]
        name: Option<String>,
        #[arg(long, value_enum, default_value = "guided")]
        mode: Mode,
        /// Agent from the config [default: the config's agent, claude]
        #[arg(long)]
        agent: Option<String>,
        /// Branch type from the repo's .flow/config.toml [default: its first]
        #[arg(long = "type")]
        kind: Option<String>,
    },
    /// Stop a flow's worker, tear down its worktree and forget it
    Remove {
        /// Flow name or id
        flow: String,
        /// Also remove a worktree with uncommitted work
        #[arg(long)]
        force: bool,
        /// Also delete the flow's branch when it has commits on no other branch
        #[arg(long)]
        delete_unmerged: bool,
    },
    /// Forget flows that are done or ended
    Clear,
    /// Open the terminal UI: the master, every worker and the board
    Ui {
        /// Stop the server and every session in it
        #[arg(long)]
        stop: bool,
    },
    /// The session server flow ui starts on its own
    #[command(hide = true)]
    Server,
    /// What a harness hook runs: reports the event on stdin into $FLOW_ID's flow
    #[command(hide = true)]
    Hook {
        /// The harness: claude-code
        adapter: String,
    },
}

#[derive(Debug, Subcommand)]
enum ReportEvent {
    /// The flow entered a phase
    Phase { phase: Phase },
    /// Work goes on, after a stop or a subagent
    Working,
    /// A subagent works on the flow
    Delegated { description: String },
    /// The flow waits for the person's answer
    Waiting { question: String },
    /// The turn ended without a question; nothing to do for the person
    Idle { note: String },
    /// The flow cannot go on
    Blocked { reason: String },
    /// A decision made on the person's behalf
    Decision { text: String },
    /// Tasks done out of the plan's total
    Tasks { done: u32, total: u32 },
    /// Review findings fixed and still open
    Review {
        #[arg(long)]
        fixed: u32,
        #[arg(long)]
        open: u32,
    },
    /// The merge request is open
    MergeRequest { url: String },
    /// The worker session ended
    Ended,
    /// Mark the flow alive without adding an event
    Heartbeat,
}

impl ReportEvent {
    /// The event to append; None for Heartbeat.
    fn into_kind(self) -> Option<EventKind> {
        Some(match self {
            ReportEvent::Phase { phase } => EventKind::Phase { phase },
            ReportEvent::Working => EventKind::Working,
            ReportEvent::Delegated { description } => EventKind::Delegated { description },
            ReportEvent::Waiting { question } => EventKind::Waiting { question },
            ReportEvent::Idle { note } => EventKind::Idle { note },
            ReportEvent::Blocked { reason } => EventKind::Blocked { reason },
            ReportEvent::Decision { text } => EventKind::Decision { text },
            ReportEvent::Tasks { done, total } => EventKind::Tasks { done, total },
            ReportEvent::Review { fixed, open } => EventKind::Review { fixed, open },
            ReportEvent::MergeRequest { url } => EventKind::MergeRequest { url },
            ReportEvent::Ended => EventKind::Ended,
            ReportEvent::Heartbeat => return None,
        })
    }
}

fn main() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error)
            if matches!(
                error.kind(),
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
            ) =>
        {
            let _ = error.print();
            return ExitCode::SUCCESS;
        }
        Err(error) => return fail(&Failure::usage(&error)),
    };
    if let Some(Command::Hook { adapter }) = &cli.command {
        run_hook(adapter);
        return ExitCode::SUCCESS;
    }
    let store = match Store::locate(
        env_path("FLOW_HOME"),
        env_path("XDG_STATE_HOME"),
        env_path("HOME"),
    ) {
        Ok(store) => store,
        Err(_) => {
            return fail(&Failure::error(
                "cannot find a state directory",
                vec!["Set FLOW_HOME to a directory".to_string()],
            ));
        }
    };
    let Ok(registry) = Registry::locate(env_path("XDG_STATE_HOME"), env_path("HOME")) else {
        return fail(&Failure::error(
            "cannot find a state directory",
            vec!["Set HOME or XDG_STATE_HOME to a directory".to_string()],
        ));
    };
    // Only for the overview and the slots; a command works without it.
    let _ = registry.register(&store);
    match run(cli, &store, &registry, now_ms()) {
        Ok(Document::Silent) => ExitCode::SUCCESS,
        Ok(document) => {
            println!("{}", document.text());
            ExitCode::SUCCESS
        }
        Err(failure) => fail(&failure),
    }
}

fn run(cli: Cli, store: &Store, registry: &Registry, now: u64) -> Result<Document, Failure> {
    match cli.command {
        None => home(store, now),
        Some(Command::New {
            name,
            request,
            mode,
        }) => new_flow(store, name, request, mode, now),
        Some(Command::Report { flow, event }) => report(store, flow, event, now),
        Some(Command::Status {
            all: true,
            json,
            full,
            ..
        }) => status_all(registry, json, full, now),
        Some(Command::Status {
            flow, json, full, ..
        }) => status(store, flow, json, full, now),
        Some(Command::Start {
            request,
            name,
            mode,
            agent,
            kind,
        }) => {
            let config = Config::load(
                env_path("FLOW_CONFIG"),
                env_path("XDG_CONFIG_HOME"),
                env_path("HOME"),
            )
            .map_err(Failure::from_io)?;
            let cwd = env::current_dir().map_err(|error| Failure::from_io(error.into()))?;
            start(
                store,
                registry,
                &config,
                &cwd,
                Launch {
                    request,
                    name,
                    mode,
                    agent,
                    kind,
                },
                now,
            )
        }
        Some(Command::Remove {
            flow,
            force,
            delete_unmerged,
        }) => remove(store, &flow, force, delete_unmerged, now),
        Some(Command::Clear) => clear(store, now),
        Some(Command::Ui { stop }) => ui(store, stop),
        Some(Command::Server) => server(store.clone()),
        Some(Command::Hook { .. }) => Ok(Document::Silent),
    }
}

/// `flow hook`: never prints to stdout (a hook's stdout can reach the model) and never fails (a
/// failing hook shows the user a notice); what goes wrong goes to stderr, Claude Code's debug log.
fn run_hook(adapter: &str) {
    let store = match Store::locate(
        env_path("FLOW_HOME"),
        env_path("XDG_STATE_HOME"),
        env_path("HOME"),
    ) {
        Ok(store) => store,
        Err(error) => return eprintln!("flow hook: {error:#}"),
    };
    let config = match Config::load(
        env_path("FLOW_CONFIG"),
        env_path("XDG_CONFIG_HOME"),
        env_path("HOME"),
    ) {
        Ok(config) => config,
        Err(error) => return eprintln!("flow hook: {error:#}"),
    };
    let mut payload = String::new();
    if let Err(error) = std::io::Read::read_to_string(&mut std::io::stdin(), &mut payload) {
        return eprintln!("flow hook: could not read the event: {error}");
    }
    let flow = env::var("FLOW_ID").ok();
    hook(&store, &config, adapter, flow, &payload, now_ms());
}

/// Translates one hook event with the named adapter and appends what changes the flow.
fn hook(
    store: &Store,
    config: &Config,
    adapter: &str,
    flow: Option<String>,
    payload: &str,
    now: u64,
) -> Document {
    let Some(flow) = flow.filter(|flow| !flow.is_empty()) else {
        return Document::Silent;
    };
    let Some(named) = adapter::Adapter::named(adapter) else {
        eprintln!("flow hook: no adapter named {adapter}");
        return Document::Silent;
    };
    let payload: serde_json::Value = match serde_json::from_str(payload) {
        Ok(payload) => payload,
        Err(error) => {
            eprintln!("flow hook: the event is not JSON: {error}");
            return Document::Silent;
        }
    };
    let kinds = named.translate(&payload, &config.phases);
    if kinds.is_empty() {
        return Document::Silent;
    }
    let id = if store.events(&flow).is_ok() {
        flow
    } else {
        let (flows, _) = views(store, now);
        match fold::resolve(&flows, &flow) {
            Some(view) => view.id.clone(),
            None => {
                eprintln!("flow hook: no flow {flow}");
                return Document::Silent;
            }
        }
    };
    for kind in kinds {
        let events = match store.events(&id) {
            Ok(events) => events,
            Err(error) => {
                eprintln!("flow hook: {error:#}");
                return Document::Silent;
            }
        };
        if fold::changes(&id, &events, &kind, now)
            && let Err(error) = store.append(&id, &Event::new(now, kind))
        {
            eprintln!("flow hook: {error:#}");
        }
    }
    Document::Silent
}

fn ui(store: &Store, stop: bool) -> Result<Document, Failure> {
    let socket = client::socket_path(store);
    if stop {
        return stop_server(&socket);
    }
    let exe = env::current_exe().map_err(|error| Failure::from_io(error.into()))?;
    client::ensure_server(&socket, &exe, &[]).map_err(Failure::from_io)?;
    let claude_home = env_path("CLAUDE_CONFIG_DIR")
        .filter(|dir| !dir.as_os_str().is_empty())
        .or_else(|| env_path("HOME").map(|home| home.join(".claude")))
        .unwrap_or_else(|| PathBuf::from(".claude"));
    let env = ui::UiEnv {
        exe: exe.clone(),
        claude_home,
        home: env_path("HOME").filter(|home| !home.as_os_str().is_empty()),
        store: store.root().to_path_buf(),
        theme_dir: config::config_file(
            env_path("FLOW_CONFIG"),
            env_path("XDG_CONFIG_HOME"),
            env_path("HOME"),
        )
        .and_then(|file| file.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from(".")),
    };
    let message = ui::run(&socket, env).map_err(Failure::from_io)?;
    Ok(Document::Text(message))
}

fn stop_server(socket: &Path) -> Result<Document, Failure> {
    let Some(mut control) = Client::connect(socket, Role::Control).map_err(Failure::from_io)?
    else {
        return Ok(Document::Text("no flow server runs".to_string()));
    };
    control
        .send(&ClientMsg::StopServer)
        .map_err(Failure::from_io)?;
    while let Ok(Some(_)) = control.recv() {}
    Ok(Document::Text(
        "stopped the flow server and every session in it".to_string(),
    ))
}

fn server(store: Store) -> Result<Document, Failure> {
    let config = Config::load(
        env_path("FLOW_CONFIG"),
        env_path("XDG_CONFIG_HOME"),
        env_path("HOME"),
    )
    .map_err(Failure::from_io)?;
    let config_dir = config::config_file(
        env_path("FLOW_CONFIG"),
        env_path("XDG_CONFIG_HOME"),
        env_path("HOME"),
    )
    .and_then(|file| file.parent().map(Path::to_path_buf))
    .unwrap_or_else(|| store.root().to_path_buf());
    let paths = server::ServerPaths {
        socket: client::socket_path(&store),
        master: store.root().join("master.json"),
        instructions: config_dir.join("master.md"),
    };
    let env = launch::LaunchEnv {
        flow_home: env_path("FLOW_HOME"),
        flow_exe: env::current_exe().map_err(|error| Failure::from_io(error.into()))?,
    };
    server::run(&paths, store, config, env, now_ms).map_err(Failure::from_io)?;
    Ok(Document::Text("flow server stopped".to_string()))
}

fn home(store: &Store, now: u64) -> Result<Document, Failure> {
    let (flows, warnings) = views(store, now);
    Ok(output::home(
        &fold::board(flows, now),
        &bin_path(),
        &warnings,
    ))
}

fn new_flow(
    store: &Store,
    name: String,
    request: String,
    mode: Mode,
    now: u64,
) -> Result<Document, Failure> {
    let (flows, _) = views(store, now);
    if let Some(existing) = fold::resolve(&flows, &name).filter(|flow| !flow.is_finished()) {
        return Ok(output::created(existing, true));
    }
    let first = Event::new(
        now,
        EventKind::Started {
            name,
            request,
            mode,
        },
    );
    let id = store.create(&first).map_err(Failure::from_io)?;
    let created = fold::fold(&id, &[first], None, now).expect("a started flow folds");
    Ok(output::created(&created, false))
}

/// What `flow start` was asked for.
struct Launch {
    request: String,
    name: Option<String>,
    mode: Mode,
    agent: Option<String>,
    /// The branch type, from the repo's .flow/config.toml.
    kind: Option<String>,
}

fn start(
    store: &Store,
    registry: &Registry,
    config: &Config,
    cwd: &Path,
    launch: Launch,
    now: u64,
) -> Result<Document, Failure> {
    let Some((agent_name, _)) = config.agent(launch.agent.as_deref()) else {
        let known: Vec<&str> = config.agents.keys().map(String::as_str).collect();
        return Err(Failure::error(
            format!(
                "no agent named {}",
                launch.agent.as_deref().unwrap_or(&config.agent)
            ),
            vec![format!("Agents: {}", known.join(", "))],
        ));
    };
    let repo = workspace::repo_root(cwd).map_err(|_| {
        Failure::error(
            "flow start needs a git repository",
            vec!["Run it inside the repository the flow works on".to_string()],
        )
    })?;
    let branching = branching::Branching::load(&repo).map_err(Failure::from_io)?;
    let kind = match &branching {
        Some(rules) => rules.pick_type(launch.kind.as_deref()),
        None if launch.kind.is_some() => Err(anyhow::anyhow!(
            "this repo has no {}, so no branch types",
            branching::CONFIG_FILE
        )),
        None => Ok(None),
    }
    .map_err(|error| {
        Failure::error(
            format!("{error:#}"),
            vec![format!(
                "Name the types in {}: [branch] types = [\"feature\", \"bugfix\"]",
                branching::CONFIG_FILE
            )],
        )
    })?;
    let (flows, _) = views(store, now);
    let taken: Vec<String> = flows
        .iter()
        .filter(|flow| !flow.is_finished())
        .map(|flow| flow.name.clone())
        .collect();
    let name = launch::flow_name(launch.name.as_deref(), &launch.request, &taken);
    let first = Event::new(
        now,
        EventKind::Started {
            name: name.clone(),
            request: launch.request.clone(),
            mode: launch.mode,
        },
    );
    let id = store.create(&first).map_err(Failure::from_io)?;

    // Claimed before the worktree and setup, so a second start cannot take the same slot meanwhile.
    let claim = Claim {
        store: store.root().to_path_buf(),
        flow: id.clone(),
    };
    let mut held = held_slots(registry, now);
    held.extend(launch::taken_slots(&flows));
    let slot = match Slots::at(registry).claim(&claim, &held, |claim| claim_is_live(claim, now)) {
        Ok(Some(slot)) => slot,
        Ok(None) => {
            let _ = store.remove(&id);
            return Err(Failure::error(
                format!("all {} slots are in use", slots::MAX_SLOTS),
                vec![
                    "Run `flow status --all` to see which flows hold them".to_string(),
                    "Run `flow clear` in projects with finished flows".to_string(),
                ],
            ));
        }
        Err(error) => {
            let _ = store.remove(&id);
            return Err(Failure::from_io(error));
        }
    };
    let base = branching::branch_name(branching.as_ref(), kind.as_deref(), &launch.request, &name);
    let branch = std::iter::once(base.clone())
        .chain((2..).map(|number| format!("{base}-{number}")))
        .find(|branch| !workspace::branch_exists(&repo, branch))
        .expect("an unused suffix always exists");
    let space = launch::workspace_for(&repo, &name, branch, slot, agent_name);
    if let Err(error) = workspace::create_worktree(&repo, &space) {
        let _ = Slots::at(registry).release(slot, &claim);
        let _ = store.remove(&id);
        return Err(Failure::from_io(error));
    }

    let setup = match workspace::run_hook(
        &repo,
        Hook::Setup,
        &space,
        &launch::hook_env(&id, &name, &repo, &space),
    ) {
        Ok(true) => "ran".to_string(),
        Ok(false) => "none".to_string(),
        Err(error) => {
            let reason = format!("setup failed: {error}");
            store
                .append(&id, &Event::new(now, EventKind::Blocked { reason }))
                .map_err(Failure::from_io)?;
            format!("failed: {error}")
        }
    };

    // Only now, so flow server opens the worker once setup is done.
    append_workspace(store, &id, &space, now)?;

    let events = store.events(&id).map_err(Failure::from_io)?;
    let flow = fold::fold(&id, &events, None, now).expect("a started flow folds");
    let opening = match Client::connect(&client::socket_path(store), Role::Control) {
        Ok(None) => Opening::WhenUiRuns,
        Ok(Some(_)) | Err(_) => Opening::Running,
    };
    Ok(output::started(&flow, &setup, opening))
}

fn append_workspace(
    store: &Store,
    id: &str,
    space: &view::Workspace,
    now: u64,
) -> Result<(), Failure> {
    let kind = EventKind::Workspace {
        path: space.path.clone(),
        branch: space.branch.clone(),
        slot: space.slot,
        window: space.window.clone(),
        agent: space.agent.clone(),
    };
    store
        .append(id, &Event::new(now, kind))
        .map_err(Failure::from_io)
}

fn report(
    store: &Store,
    flow: Option<String>,
    event: ReportEvent,
    now: u64,
) -> Result<Document, Failure> {
    let Some(name) = flow.filter(|name| !name.is_empty()) else {
        return Ok(output::skipped());
    };
    let (flows, _) = views(store, now);
    let id = fold::resolve(&flows, &name)
        .ok_or_else(|| no_flow(&name))?
        .id
        .clone();
    match event.into_kind() {
        Some(kind) => store.append(&id, &Event::new(now, kind)),
        None => store.beat(&id),
    }
    .map_err(Failure::from_io)?;
    let events = store.events(&id).map_err(Failure::from_io)?;
    let beat = store.last_beat(&id).map_err(Failure::from_io)?;
    let reported = fold::fold(&id, &events, beat, now).expect("a resolved flow folds");
    Ok(output::reported(&reported))
}

fn remove(
    store: &Store,
    name: &str,
    force: bool,
    delete_unmerged: bool,
    now: u64,
) -> Result<Document, Failure> {
    let (flows, _) = views(store, now);
    let flow = fold::resolve(&flows, name).ok_or_else(|| no_flow(name))?;
    if !force {
        refuse_uncommitted_work(flow)?;
    }
    if !delete_unmerged {
        refuse_unmerged_commits(flow)?;
    }
    let pane = match close_pane(store, flow)? {
        Closing::Closed => "closed",
        Closing::NoServer | Closing::NoPane => "none",
        Closing::TimedOut => "timed out",
    };
    let retired = retire_worktree(flow, force);
    let branch = match retired {
        Retired::Kept(_) => "kept",
        Retired::None | Retired::Removed => drop_branch(flow, delete_unmerged),
    };
    let worktree = match retired {
        Retired::None => "none".to_string(),
        Retired::Removed => "removed".to_string(),
        Retired::Kept(path) => format!("kept: {path}"),
    };
    store.remove(&flow.id).map_err(|error| {
        Failure::from_io(error.context(format!("could not remove {}", flow.name)))
    })?;
    Ok(output::removed(&flow.name, pane, &worktree, branch))
}

/// Fails before anything is touched when the flow's branch has commits on no other branch.
fn refuse_unmerged_commits(flow: &FlowView) -> Result<(), Failure> {
    let Some((repo, branch)) = branch_of(flow) else {
        return Ok(());
    };
    match workspace::commits_only_on(&repo, &branch) {
        Ok(Some(count)) if count > 0 => Err(Failure::error(
            format!(
                "{branch} has {count} commit{} {}",
                if count == 1 { "" } else { "s" },
                output::UNMERGED
            ),
            vec![
                format!(
                    "Merge or push {branch}, then run `flow remove {}` again",
                    flow.name
                ),
                format!(
                    "Run `flow remove {} --delete-unmerged` to delete it with its commits",
                    flow.name
                ),
            ],
        )),
        _ => Ok(()),
    }
}

/// Deletes the flow's branch once its worktree is gone: always with `unmerged`, else only when all
/// its commits are on another branch. deleted, kept or none.
fn drop_branch(flow: &FlowView, unmerged: bool) -> &'static str {
    let Some((repo, branch)) = branch_of(flow) else {
        return "none";
    };
    match workspace::commits_only_on(&repo, &branch) {
        Ok(None) => "none",
        Ok(Some(0)) => deleted(workspace::delete_branch(&repo, &branch)),
        Ok(Some(_)) if unmerged => deleted(workspace::delete_branch(&repo, &branch)),
        Ok(Some(_)) | Err(_) => "kept",
    }
}

fn deleted(result: anyhow::Result<()>) -> &'static str {
    if result.is_ok() { "deleted" } else { "kept" }
}

/// The repository and branch of a started flow.
fn branch_of(flow: &FlowView) -> Option<(PathBuf, String)> {
    let space = flow.workspace.as_ref()?;
    Some((launch::repo_of(space)?, space.branch.clone()))
}

/// Ends the flow's session in flow ui before its worktree goes.
fn close_pane(store: &Store, flow: &FlowView) -> Result<Closing, Failure> {
    client::close_flow(&client::socket_path(store), &flow.id, CLOSE_WAIT).map_err(Failure::from_io)
}

/// Fails before anything is touched when the flow's worktree holds uncommitted work.
fn refuse_uncommitted_work(flow: &FlowView) -> Result<(), Failure> {
    let Some(space) = &flow.workspace else {
        return Ok(());
    };
    if !Path::new(&space.path).exists() || !workspace::has_uncommitted_work(space).unwrap_or(false)
    {
        return Ok(());
    }
    Err(Failure::error(
        format!("{} {} in {}", flow.name, output::UNCOMMITTED, space.path),
        vec![
            format!(
                "Commit or stash it in {}, then run `flow remove {}` again",
                space.path, flow.name
            ),
            format!(
                "Run `flow remove {} --force` to discard the uncommitted work",
                flow.name
            ),
        ],
    ))
}

fn clear(store: &Store, now: u64) -> Result<Document, Failure> {
    let (flows, _) = views(store, now);
    let mut names = Vec::new();
    let mut kept = Vec::new();
    for flow in flows.iter().filter(|flow| flow.is_finished()) {
        close_pane(store, flow)?;
        match retire_worktree(flow, false) {
            Retired::Kept(path) => kept.push((flow.name.clone(), path)),
            Retired::None | Retired::Removed => {
                drop_branch(flow, false);
            }
        }
        store.remove(&flow.id).map_err(|error| {
            Failure::from_io(error.context(format!("could not clear {}", flow.name)))
        })?;
        names.push(flow.name.clone());
    }
    Ok(output::cleared(&names, &kept))
}

/// What happened to a flow's worktree when it was cleared or removed.
enum Retired {
    None,
    Removed,
    Kept(String),
}

/// Runs the teardown hook and removes the flow's worktree; keeps it with uncommitted work unless forced.
fn retire_worktree(flow: &FlowView, force: bool) -> Retired {
    let Some(space) = &flow.workspace else {
        return Retired::None;
    };
    let Some(repo) = launch::repo_of(space) else {
        return Retired::None;
    };
    if !Path::new(&space.path).exists() {
        return Retired::None;
    }
    let env = launch::hook_env(&flow.id, &flow.name, &repo, space);
    let _ = workspace::run_hook(&repo, Hook::Teardown, space, &env);
    if workspace::remove_worktree(&repo, space, force).unwrap_or(false) {
        Retired::Removed
    } else {
        Retired::Kept(space.path.clone())
    }
}

fn status(
    store: &Store,
    flow: Option<String>,
    json: bool,
    full: bool,
    now: u64,
) -> Result<Document, Failure> {
    let (flows, warnings) = views(store, now);
    let Some(name) = flow else {
        let board = fold::board(flows, now);
        return Ok(if json {
            Document::Json(board.to_json())
        } else {
            output::status_list(&board, &warnings, full)
        });
    };
    let one = fold::resolve(&flows, &name)
        .cloned()
        .ok_or_else(|| no_flow(&name))?;
    Ok(if json {
        Document::Json(fold::board(vec![one], now).to_json())
    } else {
        let worktree_exists = one
            .workspace
            .as_ref()
            .map(|space| Path::new(&space.path).exists());
        output::status_detail(&one, now, full, worktree_exists)
    })
}

/// Every registered project with flows, each as its board; unreadable logs become warnings.
fn status_all(registry: &Registry, json: bool, full: bool, now: u64) -> Result<Document, Failure> {
    let mut projects: Vec<view::Project> = Vec::new();
    let mut warnings = Vec::new();
    for store in registry.stores() {
        let (flows, unread) = views(&store, now);
        let base = registry::project_name(&store, registry);
        let name = (1..)
            .map(|number| match number {
                1 => base.clone(),
                _ => format!("{base} ({number})"),
            })
            .find(|name| projects.iter().all(|project| &project.name != name))
            .expect("an unused suffix always exists");
        warnings.extend(
            unread
                .into_iter()
                .map(|warning| format!("{name}: {warning}")),
        );
        if flows.is_empty() {
            continue;
        }
        projects.push(view::Project {
            name,
            root: store.root().display().to_string(),
            board: fold::board(flows, now),
        });
    }
    let projects = view::Projects {
        version: view::VIEW_VERSION,
        now,
        projects,
    };
    Ok(if json {
        Document::Json(projects.to_json())
    } else {
        output::status_all(&projects, &warnings, full)
    })
}

/// Whether a claim's flow is still unfinished in its store.
fn claim_is_live(claim: &Claim, now: u64) -> bool {
    Store::at(claim.store.clone())
        .events(&claim.flow)
        .ok()
        .and_then(|events| fold::fold(&claim.flow, &events, None, now))
        .is_some_and(|flow| !flow.is_finished())
}

/// The slots unfinished flows hold in every registered store, claimed or not (flows started before
/// claims existed).
fn held_slots(registry: &Registry, now: u64) -> Vec<u8> {
    registry
        .stores()
        .iter()
        .flat_map(|store| launch::taken_slots(&views(store, now).0))
        .collect()
}

/// Every flow folded; unreadable ones become warnings so the rest still shows.
fn views(store: &Store, now: u64) -> (Vec<FlowView>, Vec<String>) {
    let (logs, warnings) = store.logs();
    (fold::views(logs, now), warnings)
}

fn no_flow(name: &str) -> Failure {
    Failure::error(
        format!("no flow named {name}"),
        vec![
            "Run `flow` to list the flows".to_string(),
            format!("Run `flow new --name {name} --request <text>` to start it"),
        ],
    )
}

fn fail(failure: &Failure) -> ExitCode {
    println!("{}", failure.to_toon());
    ExitCode::from(failure.code)
}

fn env_path(name: &str) -> Option<PathBuf> {
    env::var_os(name).map(PathBuf::from)
}

fn bin_path() -> String {
    let bin = env::current_exe()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|_| "flow".to_string());
    match env::var("HOME") {
        Ok(home) if !home.is_empty() && bin.starts_with(&home) => {
            format!("~{}", &bin[home.len()..])
        }
        _ => bin,
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
