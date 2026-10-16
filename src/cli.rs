mod commands;
mod env;
pub mod output;

use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use anyhow::Context;
use clap::{Parser, Subcommand};

use commands::{home, new, no_flow, report, send, status, views};

pub use crate::event::unix_ms;
pub use env::Env;

use crate::cli::output::{BranchFate, Document, Failure, Opening, Retired, Setup};
use crate::client::{self, Client, Closing};
use crate::config::Config;
use crate::deliver::{self, WorkerNews};
use crate::event::{Event, EventKind, Mode};
use crate::proto::{ClientMsg, Role};
use crate::registry::Registry;
use crate::slots::{Claim, Slots};
use crate::store::{Store, Waiter};
use crate::view::FlowView;
use crate::workspace::Hook;
use crate::{adapter, branching, fold, launch, server, slots, text, ui, view, workspace};

/// How long `flow remove` and `flow clear` wait for a flow's session to end (SIGKILL comes after 3 s).
const CLOSE_WAIT: Duration = Duration::from_secs(5);
/// How often `flow wait` looks for news, as often as the server looks at the store.
const WAIT_EVERY: Duration = Duration::from_millis(500);
/// Tracks parallel feature flows: what each is doing, where it is blocked, how far it got.
#[derive(Debug, Parser)]
#[command(version, about)]
pub struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Track a new flow, or return the unfinished flow of that name
    New(new::Args),
    /// Append one event to a flow
    Report(report::Args),
    /// Every flow, or one flow with its phases, timeline and decisions
    Status(status::Args),
    /// Start a worker: a flow, a worktree, the setup hook and an agent flow ui opens
    Start {
        /// What to build, e.g. "SHOP-217 add CSV export"
        request: String,
        /// Flow name [default: from the ticket key or the first words]
        #[arg(long)]
        name: Option<String>,
        #[arg(long, value_enum, default_value_t)]
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
    /// Tell a flow's worker something: an answer, an instruction, a correction
    Send(send::Args),
    /// Forget flows that are done or ended
    Clear,
    /// Open the terminal UI: the master, every worker and the board
    Ui {
        /// Stop the server and every session in it
        #[arg(long)]
        stop: bool,
    },
    /// What a session's asyncRewake hook runs: sleeps until there is news for it, prints it, exits 2
    #[command(hide = true)]
    Wait {
        /// The master's news: flows that changed [default: $FLOW_ID's messages]
        #[arg(long)]
        master: bool,
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

/// Runs the command `cli` names: what to print, or the failure to print instead.
///
/// # Errors
///
/// The command's failure, with its message, next steps and exit code. Hook and Wait run without
/// a store; every other command fails without one.
pub fn run(cli: Cli, env: &Env, clock: fn() -> u64) -> Result<Document, Failure> {
    if let Some(Command::Hook { adapter }) = &cli.command {
        run_hook(adapter, env, clock);
        return Ok(Document::Silent);
    }
    if let Some(Command::Wait { master }) = &cli.command {
        return Ok(run_wait(*master, env, clock));
    }
    let Some(store) = env.store() else {
        return Err(Failure::error(
            "cannot find a state directory",
            vec!["Set FLOW_HOME to a directory".to_string()],
        ));
    };
    let Some(registry) = env.registry() else {
        return Err(Failure::error(
            "cannot find a state directory",
            vec!["Set HOME or XDG_STATE_HOME to a directory".to_string()],
        ));
    };
    // Only for the overview and the slots; a command works without it.
    let _ = registry.register(&store);
    let now = clock();
    let store = &store;
    let registry = &registry;
    match cli.command {
        None => home::run(store, &env.bin_path(), now),
        Some(Command::New(args)) => new::run(args, store, now),
        Some(Command::Report(args)) => report::run(args, store, now),
        Some(Command::Status(args)) => status::run(&args, store, registry, now),
        Some(Command::Start {
            request,
            name,
            mode,
            agent,
            kind,
        }) => {
            let config = env.config()?;
            let cwd = std::env::current_dir()?;
            start(
                store,
                registry,
                &config,
                &cwd,
                &Launch {
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
        }) => {
            let uncommitted = if force {
                Uncommitted::Discard
            } else {
                Uncommitted::Keep
            };
            let unmerged = if delete_unmerged {
                Unmerged::Delete
            } else {
                Unmerged::Keep
            };
            remove(store, &flow, uncommitted, unmerged, now)
        }
        Some(Command::Send(args)) => send::run(&args, store, env.flow_id(), now),
        Some(Command::Clear) => clear(store, now),
        Some(Command::Ui { stop: true }) => stop_server(&client::socket_path(store)),
        Some(Command::Ui { stop: false }) => ui(store, env, clock),
        Some(Command::Server) => server(store.clone(), env, clock),
        Some(Command::Hook { .. } | Command::Wait { .. }) => Ok(Document::Silent),
    }
}

/// `flow hook`: never prints to stdout (a hook's stdout can reach the model) and never fails (a
/// failing hook shows the user a notice); what goes wrong goes to stderr, Claude Code's debug log.
fn run_hook(adapter: &str, env: &Env, clock: fn() -> u64) {
    let Some(store) = env.store() else {
        return eprintln!("flow hook: cannot find a state directory");
    };
    // A broken config must not stop running workers from reporting; start and ui say it loudly.
    let config = env.config().unwrap_or_else(|error| {
        eprintln!("flow hook: {error:#}; using the built-in config");
        Config::builtin()
    });
    let mut payload = String::new();
    if let Err(error) = std::io::Read::read_to_string(&mut std::io::stdin(), &mut payload) {
        return eprintln!("flow hook: could not read the event: {error}");
    }
    let flow = env.flow_id.clone();
    hook(&store, &config, adapter, flow, &payload, clock());
}

/// `flow wait`: the news to wake the session with; Silent when the wait ends without news.
fn run_wait(master: bool, env: &Env, clock: fn() -> u64) -> Document {
    let Some(store) = env.store() else {
        return Document::Silent;
    };
    let waiter = match env.flow_id() {
        _ if master => Waiter::Master,
        Some(id) => Waiter::Flow(id.to_string()),
        None => return Document::Silent,
    };
    match wait(&store, &waiter, clock) {
        Some(news) => Document::Wake(news),
        None => Document::Silent,
    }
}

/// Sleeps until there is news for the session; None once another waiter took over, the session
/// ended (the parent changed), or a worker's flow is finished or gone.
fn wait(store: &Store, waiter: &Waiter, clock: fn() -> u64) -> Option<String> {
    let me = std::process::id();
    store.claim_waiter(waiter, me).ok()?;
    let parent = std::os::unix::process::parent_id();
    let is_mine =
        || store.waiter(waiter) == Some(me) && std::os::unix::process::parent_id() == parent;
    while is_mine() {
        let news = match waiter {
            Waiter::Flow(id) => match deliver::worker_news(store, id, clock()) {
                WorkerNews::Gone => return None,
                WorkerNews::Nothing => None,
                WorkerNews::News(messages) => {
                    let messages: Vec<&str> = messages.iter().map(String::as_str).collect();
                    Some(output::news_for_worker(&messages))
                }
            },
            Waiter::Master => {
                let changed = deliver::master_news(store, clock, &is_mine);
                let changed: Vec<&FlowView> = changed.iter().collect();
                (!changed.is_empty()).then(|| output::news_for_master(&changed))
            }
        };
        if news.is_some() {
            return news;
        }
        thread::sleep(WAIT_EVERY);
    }
    None
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
        let Some(view) = fold::resolve(&flows, &flow) else {
            eprintln!("flow hook: no flow {flow}");
            return Document::Silent;
        };
        view.id.clone()
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

fn ui(store: &Store, env: &Env, clock: fn() -> u64) -> Result<Document, Failure> {
    let socket = client::socket_path(store);
    let exe = std::env::current_exe()?;
    client::ensure_server(&socket, &exe, &[])?;
    let ui_env = ui::UiEnv {
        exe,
        claude_home: env.claude_home(),
        home: env.user_home().map(Path::to_path_buf),
        store: store.root().to_path_buf(),
        theme_dir: env.config_dir().unwrap_or_else(|| PathBuf::from(".")),
    };
    let message = ui::run(&socket, ui_env, clock)?;
    Ok(Document::Text(message))
}

fn stop_server(socket: &Path) -> Result<Document, Failure> {
    let Some(mut control) = Client::connect(socket, Role::Control)? else {
        return Ok(Document::Text("no flow server runs".to_string()));
    };
    control.send(&ClientMsg::StopServer)?;
    while let Ok(Some(_)) = control.recv() {}
    Ok(Document::Text(
        "stopped the flow server and every session in it".to_string(),
    ))
}

fn server(store: Store, env: &Env, clock: fn() -> u64) -> Result<Document, Failure> {
    let config = env.config()?;
    let config_dir = env
        .config_dir()
        .unwrap_or_else(|| store.root().to_path_buf());
    let paths = server::ServerPaths {
        socket: client::socket_path(&store),
        master: store.root().join("master.json"),
        instructions: config_dir.join("master.md"),
    };
    let launch_env = launch::LaunchEnv {
        flow_home: env.flow_home.clone(),
        flow_exe: std::env::current_exe()?,
    };
    let cwd = std::env::current_dir().context("could not read the current directory")?;
    server::run(&paths, store, config, launch_env, &cwd, clock)?;
    Ok(Document::Text("flow server stopped".to_string()))
}

/// What `flow start` was asked for.
#[derive(Debug)]
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
    launch: &Launch,
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
    let branching = branching::Branching::load(&repo)?;
    let kind = pick_kind(branching.as_ref(), launch.kind.as_deref())?;
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
    let id = store.create(&first)?;
    let mut rollback = Rollback::new(store, registry, &id);

    // Claimed before the worktree and setup, so a second start cannot take the same slot meanwhile.
    let claim = Claim {
        store: store.root().to_path_buf(),
        flow: id.clone(),
    };
    let slot = claim_slot(registry, &claim, &flows, now)?;
    rollback.slot = Some((slot, claim));
    let base = branching::branch_name(branching.as_ref(), kind.as_deref(), &launch.request, &name);
    let space = launch::workspace_for(&repo, &name, free_branch(&repo, &base), slot, agent_name);
    workspace::create_worktree(&repo, &space)?;
    rollback.disarm();

    let setup = match workspace::run_hook(
        &repo,
        Hook::Setup,
        &space,
        &launch::hook_env(&id, &name, &repo, &space),
    ) {
        Ok(true) => Setup::Ran,
        Ok(false) => Setup::None,
        Err(error) => {
            let reason = format!("setup failed: {error}");
            store.append(&id, &Event::new(now, EventKind::Blocked { reason }))?;
            Setup::Failed(error.to_string())
        }
    };

    // Only now, so flow server opens the worker once setup is done.
    append_workspace(store, &id, &space, now)?;

    let events = store.events(&id)?;
    let flow = fold::fold(&id, &events, None, now).expect("a started flow folds");
    let opening = match Client::connect(&client::socket_path(store), Role::Control) {
        Ok(None) => Opening::WhenUiRuns,
        Ok(Some(_)) | Err(_) => Opening::Running,
    };
    Ok(output::started(&flow, &space, &setup, opening))
}

/// The branch type for a start: the one asked for or the repo's first, if the repo has types.
fn pick_kind(
    rules: Option<&branching::Branching>,
    asked: Option<&str>,
) -> Result<Option<String>, Failure> {
    match rules {
        Some(rules) => rules.pick_type(asked),
        None if asked.is_some() => Err(anyhow::anyhow!(
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
    })
}

/// The lowest slot no unfinished flow in any project holds, claimed for this start.
fn claim_slot(
    registry: &Registry,
    claim: &Claim,
    flows: &[FlowView],
    now: u64,
) -> Result<u8, Failure> {
    let mut held = held_slots(registry, now);
    held.extend(launch::taken_slots(flows));
    Slots::at(registry)
        .claim(claim, &held, |claim| claim_is_live(claim, now))?
        .ok_or_else(|| {
            Failure::error(
                format!("all {} slots are in use", slots::MAX_SLOTS),
                vec![
                    "Run `flow status --all` to see which flows hold them".to_string(),
                    "Run `flow clear` in projects with finished flows".to_string(),
                ],
            )
        })
}

/// `base`, or base-2, base-3, ... when the repo has that branch already.
fn free_branch(repo: &Path, base: &str) -> String {
    std::iter::once(base.to_string())
        .chain((2..).map(|number| format!("{base}-{number}")))
        .find(|branch| !workspace::branch_exists(repo, branch))
        .expect("an unused suffix always exists")
}

/// Undoes a start that failed before its worktree existed: forgets the flow and gives its slot back.
struct Rollback<'a> {
    store: &'a Store,
    slots: Slots,
    id: String,
    slot: Option<(u8, Claim)>,
    armed: bool,
}

impl<'a> Rollback<'a> {
    fn new(store: &'a Store, registry: &Registry, id: &str) -> Rollback<'a> {
        Rollback {
            store,
            slots: Slots::at(registry),
            id: id.to_string(),
            slot: None,
            armed: true,
        }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for Rollback<'_> {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        if let Some((slot, claim)) = &self.slot {
            let _ = self.slots.release(*slot, claim);
        }
        let _ = self.store.remove(&self.id);
    }
}

fn append_workspace(
    store: &Store,
    id: &str,
    space: &view::Workspace,
    now: u64,
) -> Result<(), Failure> {
    store
        .append(id, &Event::new(now, space.clone().into()))
        .map_err(Failure::from)
}

fn remove(
    store: &Store,
    name: &str,
    uncommitted: Uncommitted,
    unmerged: Unmerged,
    now: u64,
) -> Result<Document, Failure> {
    let (flows, _) = views(store, now);
    let flow = fold::resolve(&flows, name).ok_or_else(|| no_flow(name))?;
    if uncommitted == Uncommitted::Keep {
        refuse_uncommitted_work(flow)?;
    }
    if unmerged == Unmerged::Keep {
        refuse_unmerged_commits(flow)?;
    }
    let pane = close_pane(store, flow)?;
    let retired = retire_worktree(flow, uncommitted);
    let branch = match retired {
        Retired::Kept(_) => BranchFate::Kept,
        Retired::NoWorktree | Retired::Removed => drop_branch(flow, unmerged),
    };
    store
        .remove(&flow.id)
        .map_err(|error| Failure::from(error.context(format!("could not remove {}", flow.name))))?;
    Ok(output::removed(&flow.name, pane, &retired, branch))
}

/// What `flow remove` does with uncommitted work in the worktree (--force discards it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Uncommitted {
    Keep,
    Discard,
}

/// What `flow remove` does with a branch whose commits are on no other branch
/// (--delete-unmerged deletes it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Unmerged {
    Keep,
    Delete,
}

/// Fails before anything is touched when the flow's branch has commits on no other branch.
fn refuse_unmerged_commits(flow: &FlowView) -> Result<(), Failure> {
    let Some((repo, branch)) = branch_of(flow) else {
        return Ok(());
    };
    match workspace::commits_only_on(&repo, branch) {
        Ok(Some(count)) if count > 0 => Err(Failure::error(
            format!(
                "{branch} has {count} commit{} {}",
                text::plural(count),
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

/// Deletes the flow's branch once its worktree is gone: with Unmerged::Delete always, else only
/// when all its commits are on another branch.
fn drop_branch(flow: &FlowView, unmerged: Unmerged) -> BranchFate {
    let Some((repo, branch)) = branch_of(flow) else {
        return BranchFate::NoBranch;
    };
    match workspace::commits_only_on(&repo, branch) {
        Ok(None) => BranchFate::NoBranch,
        Ok(Some(count)) if count == 0 || unmerged == Unmerged::Delete => {
            match workspace::delete_branch(&repo, branch) {
                Ok(()) => BranchFate::Deleted,
                Err(_) => BranchFate::Kept,
            }
        }
        Ok(Some(_)) | Err(_) => BranchFate::Kept,
    }
}

/// The repository and branch of a started flow.
fn branch_of(flow: &FlowView) -> Option<(PathBuf, &str)> {
    let space = flow.workspace.as_ref()?;
    Some((launch::repo_of(space)?, &space.branch))
}

/// Ends the flow's session in flow ui before its worktree goes.
fn close_pane(store: &Store, flow: &FlowView) -> Result<Closing, Failure> {
    client::close_flow(&client::socket_path(store), &flow.id, CLOSE_WAIT).map_err(Failure::from)
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
        match retire_worktree(flow, Uncommitted::Keep) {
            Retired::Kept(path) => kept.push((flow.name.clone(), path)),
            Retired::NoWorktree | Retired::Removed => {
                drop_branch(flow, Unmerged::Keep);
            }
        }
        store.remove(&flow.id).map_err(|error| {
            Failure::from(error.context(format!("could not clear {}", flow.name)))
        })?;
        names.push(flow.name.clone());
    }
    Ok(output::cleared(&names, &kept))
}

/// Runs the teardown hook and removes the flow's worktree; keeps it with uncommitted work unless forced.
fn retire_worktree(flow: &FlowView, uncommitted: Uncommitted) -> Retired {
    let Some(space) = &flow.workspace else {
        return Retired::NoWorktree;
    };
    let Some(repo) = launch::repo_of(space) else {
        return Retired::NoWorktree;
    };
    if !Path::new(&space.path).exists() {
        return Retired::NoWorktree;
    }
    let env = launch::hook_env(&flow.id, &flow.name, &repo, space);
    let _ = workspace::run_hook(&repo, Hook::Teardown, space, &env);
    let force = uncommitted == Uncommitted::Discard;
    if workspace::remove_worktree(&repo, space, force).unwrap_or(false) {
        Retired::Removed
    } else {
        Retired::Kept(space.path.clone())
    }
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
