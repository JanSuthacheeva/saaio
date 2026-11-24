//! `saaio setup`: a flow's .saaio/teardown, then its .saaio/setup again

use std::path::Path;

use super::retire;
use super::{no_flow, views};
use crate::cli::context::{Context, Opened};
use crate::cli::output::setup::{self, Setup};
use crate::cli::output::{Document, Failure};
use crate::event::{Event, EventKind};
use crate::fold;
use crate::id::FlowId;
use crate::launch::worktree;
use crate::store::Store;
use crate::trust::Trust;
use crate::view::Workspace;
use crate::workspace::hooks::{self, Hook};

#[derive(Debug, clap::Args)]
pub struct Args {
    /// Flow name or id [default: the flow whose worker runs it]
    name: Option<String>,
}

/// Runs the flow's teardown, then its setup, with its slot and environment.
pub fn run(args: &Args, ctx: &Context) -> Result<Document, Failure> {
    let Opened { store, registry } = ctx.open()?;
    let trust = Trust::at(&registry);
    let Some(name) = args.name.as_deref().or(ctx.env.flow_id()) else {
        return Err(Failure::error(
            "name the flow to set up",
            ["Run `saaio setup <name>`; `saaio` lists the flows"],
        ));
    };
    let (flows, _) = views(&store, ctx.now());
    let flow = fold::resolve(&flows, name).ok_or_else(|| no_flow(name))?;
    let name = flow.name.as_str();
    if flow.is_finished() {
        return Err(setup::refused(name, "has ended"));
    }
    let space = flow.workspace.as_ref().filter(|_| !flow.is_plain());
    let Some((space, repo)) = space.and_then(|space| Some((space, worktree::repo_of(space)?)))
    else {
        return Err(setup::refused(name, "has no worktree to set up"));
    };
    if !Path::new(&space.path).exists() {
        let why = format!("has no worktree: {} is gone", space.path);
        return Err(setup::refused(name, &why));
    }
    if !hooks::exists(&repo, Hook::Setup) {
        let why = format!("has no .saaio/setup in {}", repo.display());
        return Err(setup::refused(name, &why));
    }
    if !trust.covers(&repo) {
        return Err(setup::untrusted(name, &repo));
    }
    let teardown = retire::teardown(&trust, &store, flow);
    match run_setup(&store, &trust, &repo, flow.id, name, space, ctx.now())? {
        Setup::Failed { reason, log } => Err(setup::failed(flow, &teardown, &reason, &log)),
        ran => Ok(setup::reran(flow, &teardown, &ran)),
    }
}

/// Runs .saaio/setup for the flow into its setup log and appends a Setup event;
/// Setup::None and no event when the repo has none. Shared with start.
pub(super) fn run_setup(
    store: &Store,
    trust: &Trust,
    repo: &Path,
    id: FlowId,
    name: &str,
    space: &Workspace,
    now: u64,
) -> Result<Setup, Failure> {
    let path = store.hook_log(id, Hook::Setup.name());
    let env = worktree::hook_env(id, name, repo, space);
    let log = path.display().to_string();
    let (setup, failed) = match hooks::run(trust, repo, Hook::Setup, space, &env, &path) {
        Ok(false) => return Ok(Setup::None),
        Ok(true) => (Setup::Ran { log: log.clone() }, None),
        Err(error) => {
            let reason = format!("{error:#}");
            let failed = Setup::Failed {
                reason: reason.clone(),
                log: log.clone(),
            };
            (failed, Some(reason))
        }
    };
    store.append(id, &Event::new(now, EventKind::Setup { failed, log }))?;
    Ok(setup)
}
