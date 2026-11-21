//! `saaio setup`: a flow's .saaio/teardown, then its .saaio/setup again

use std::path::Path;

use crate::cli::output::Failure;
use crate::cli::output::setup::Setup;
use crate::event::{Event, EventKind};
use crate::id::FlowId;
use crate::launch::worktree;
use crate::store::Store;
use crate::trust::Trust;
use crate::view::Workspace;
use crate::workspace::hooks::{self, Hook};

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
