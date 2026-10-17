mod plain;

use std::path::{Path, PathBuf};

use super::{check_request, views};
use crate::branching::{self, Branching};
use crate::cli::output::start::{self, Opening, Setup};
use crate::cli::output::{Document, Failure};
use crate::client::{self, Client};
use crate::config::Config;
use crate::event::{Event, EventKind, Mode};
use crate::fold;
use crate::launch::{name, worktree};
use crate::proto::Role;
use crate::registry::Registry;
use crate::slots::{self, Claim, Slots};
use crate::store::Store;
use crate::view::{FlowView, Workspace};
use crate::workspace::{self, Hook};

#[derive(Debug, clap::Args)]
pub struct Args {
    /// What to build, e.g. "SHOP-217 add CSV export"
    #[arg(required_unless_present = "plain")]
    request: Option<String>,
    /// Open the agent in this folder instead: no worktree, branch, slot or prompt
    #[arg(long, value_name = "FOLDER", conflicts_with_all = ["request", "mode", "kind"])]
    plain: Option<PathBuf>,
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
}

/// Starts a worker: a flow, a worktree, the setup hook, and the agent flow ui opens; with --plain,
/// plain::open.
pub fn run(
    args: &Args,
    store: &Store,
    registry: &Registry,
    config: &Config,
    cwd: &Path,
    home: Option<&Path>,
    now: u64,
) -> Result<Document, Failure> {
    if let Some(folder) = &args.plain {
        return plain::open(folder, args, store, config, cwd, home, now);
    }
    let request = args
        .request
        .as_deref()
        .expect("clap requires a request without --plain");
    check_request(request)?;
    let agent_name = pick_agent(config, args.agent.as_deref())?;
    let repo = workspace::repo_root(cwd).map_err(|_| {
        Failure::error(
            "flow start needs a git repository",
            vec!["Run it inside the repository the flow works on".to_string()],
        )
    })?;
    let branching = Branching::load(&repo)?;
    let kind = pick_kind(branching.as_ref(), args.kind.as_deref())?;
    let (flows, _) = views(store, now);
    let name = name::flow_name(args.name.as_deref(), request, &live_names(&flows));
    let first = Event::new(
        now,
        EventKind::Started {
            name: name.clone(),
            request: request.to_string(),
            mode: args.mode,
            phases: Some(config.workflow.phases()),
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
    let base = branching::branch_name(branching.as_ref(), kind.as_deref(), request, &name);
    let space = worktree::workspace_for(&repo, &name, free_branch(&repo, &base), slot, agent_name);
    workspace::create_worktree(&repo, &space)?;
    rollback.disarm();

    let setup = match workspace::run_hook(
        &repo,
        Hook::Setup,
        &space,
        &worktree::hook_env(&id, &name, &repo, &space),
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
    Ok(start::started(&flow, &space, &setup, opening(store)))
}

/// The names unfinished flows hold; a new flow takes another.
fn live_names(flows: &[FlowView]) -> Vec<String> {
    flows
        .iter()
        .filter(|flow| !flow.is_finished())
        .map(|flow| flow.name.clone())
        .collect()
}

/// The config's agent `asked` names (or its default); the failure lists the agents.
fn pick_agent<'a>(config: &'a Config, asked: Option<&str>) -> Result<&'a str, Failure> {
    let Some((name, _)) = config.agent(asked) else {
        let known: Vec<&str> = config.agents.keys().map(String::as_str).collect();
        return Err(Failure::error(
            format!("no agent named {}", asked.unwrap_or(&config.worker.agent)),
            vec![format!("Agents: {}", known.join(", "))],
        ));
    };
    Ok(name)
}

/// Whether flow ui opens the new agent now or once it runs.
fn opening(store: &Store) -> Opening {
    match Client::connect(&client::socket_path(store), Role::Control) {
        Ok(None) => Opening::WhenUiRuns,
        Ok(Some(_)) | Err(_) => Opening::Running,
    }
}

/// The branch type for a start: the one asked for or the repo's first, if the repo has types.
fn pick_kind(rules: Option<&Branching>, asked: Option<&str>) -> Result<Option<String>, Failure> {
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
    held.extend(slots::taken_slots(flows));
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

/// The slots unfinished flows hold in every registered store, claimed or not (flows started before
/// claims existed).
fn held_slots(registry: &Registry, now: u64) -> Vec<u8> {
    registry
        .stores()
        .iter()
        .flat_map(|store| slots::taken_slots(&views(store, now).0))
        .collect()
}

/// Whether a claim's flow is still unfinished in its store.
fn claim_is_live(claim: &Claim, now: u64) -> bool {
    Store::at(claim.store.clone())
        .events(&claim.flow)
        .ok()
        .and_then(|events| fold::fold(&claim.flow, &events, None, now))
        .is_some_and(|flow| !flow.is_finished())
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

fn append_workspace(store: &Store, id: &str, space: &Workspace, now: u64) -> Result<(), Failure> {
    store
        .append(id, &Event::new(now, space.clone().into()))
        .map_err(Failure::from)
}
