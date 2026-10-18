use std::path::Path;

use super::{no_flow, views};
use crate::cli::Env;
use crate::cli::output::{self, Document, Failure};
use crate::fold;
use crate::registry::{self, Registry};
use crate::store::Store;
use crate::view;

#[derive(Debug, clap::Args)]
pub struct Args {
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
}

/// Every flow, or one flow with its phases, timeline and decisions; --all for every project.
pub fn run(
    args: &Args,
    store: &Store,
    registry: &Registry,
    env: &Env,
    now: u64,
) -> Result<Document, Failure> {
    if args.all {
        return all_stores(registry, args.json, args.full, now);
    }
    one_store(args, store, env, now)
}

fn one_store(args: &Args, store: &Store, env: &Env, now: u64) -> Result<Document, Failure> {
    let (flows, warnings) = views(store, now);
    let Some(name) = &args.flow else {
        let board = fold::board(flows, now);
        return Ok(if args.json {
            Document::Json(board.to_json())
        } else {
            output::status::list(&board, &warnings, args.full)
        });
    };
    let one = fold::resolve(&flows, name)
        .cloned()
        .ok_or_else(|| no_flow(name))?;
    Ok(if args.json {
        Document::Json(fold::board(vec![one], now).to_json())
    } else {
        let worktree_exists = one
            .workspace
            .as_ref()
            .map(|space| Path::new(&space.path).exists());
        // saaio ui reports a config it refuses; status only names the key in a hint.
        let prefix = env
            .config()
            .map(|config| config.ui.prefix)
            .unwrap_or_default();
        output::status::detail(&one, now, args.full, worktree_exists, prefix)
    })
}

/// Every registered project with flows, each as its board; unreadable logs become warnings.
fn all_stores(registry: &Registry, json: bool, full: bool, now: u64) -> Result<Document, Failure> {
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
        output::status::all(&projects, &warnings, full)
    })
}
