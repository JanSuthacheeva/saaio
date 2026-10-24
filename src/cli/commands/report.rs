//! `saaio report`: one event appended to a flow, or a heartbeat

use clap::Subcommand;

use super::{no_flow, of_own_repo, repo_dir, views};
use crate::adapter::calls;
use crate::cli::output::{Document, Failure, flow};
use crate::event::{Event, EventKind, Phase};
use crate::fold;
use crate::store::Store;
use crate::view::FlowView;

#[derive(Debug, clap::Args)]
pub struct Args {
    /// Flow name or id [default: $SAAIO_FLOW_ID]
    #[arg(long, env = "SAAIO_FLOW_ID")]
    flow: Option<String>,
    #[command(subcommand)]
    event: ReportEvent,
}

#[derive(Debug, Subcommand)]
enum ReportEvent {
    /// The flow entered a phase
    Phase {
        /// One of the flow's phases, as `saaio status <name>` lists them: plan, implement, ...
        phase: Phase,
    },
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
            ReportEvent::Waiting { question } => EventKind::waiting(question),
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

/// The failure for a phase the flow's workflow does not have; the help lists the ones it has.
fn no_phase(flow: &FlowView, phase: &Phase) -> Failure {
    let phases: Vec<&str> = flow.steps.iter().map(|step| step.phase.name()).collect();
    let help = if flow.is_plain() {
        format!("{} is a plain session, which has no phases", flow.name)
    } else {
        format!("Phases: {}", phases.join(", "))
    };
    Failure::error(
        format!("{} has no phase {}", flow.name, phase.name()),
        vec![help],
    )
}

/// The failure for a text that is no merge request URL; the help names the form one has.
fn no_merge_request(url: &str) -> Failure {
    Failure::error(
        format!("{url} is no merge request URL"),
        vec![
            concat!(
                "Report it as https://<host>/<owner>/<repo>/pull/<n> ",
                "or https://<host>/<group>/<repo>/-/merge_requests/<n>"
            )
            .to_string(),
        ],
    )
}

/// The failure for a URL that is no merge request of the flow's repository.
fn foreign_merge_request(flow: &FlowView, url: &str) -> Failure {
    Failure::error(
        format!("{url} is not a merge request of {}'s repository", flow.name),
        vec!["Report the URL `gh pr create` or `glab mr create` printed".to_string()],
    )
}

/// Appends one event to a flow; skipped without a flow, so an agent outside saaio can report. A
/// merge request is kept in its canonical form and must be one of the flow's repository, its
/// remotes read in its main checkout, else in the current folder for a flow without one.
pub fn run(mut args: Args, store: &Store, now: u64) -> Result<Document, Failure> {
    let Some(name) = args.flow.filter(|name| !name.is_empty()) else {
        return Ok(flow::skipped());
    };
    let (flows, _) = views(store, now);
    let flow = fold::resolve(&flows, &name).ok_or_else(|| no_flow(&name))?;
    if let ReportEvent::Phase { phase } = &args.event
        && !flow.steps.iter().any(|step| step.phase == *phase)
    {
        return Err(no_phase(flow, phase));
    }
    if let ReportEvent::MergeRequest { url } = &mut args.event {
        let Some(canonical) = calls::canonical_merge_request(url) else {
            return Err(no_merge_request(url));
        };
        let dir = match repo_dir(flow) {
            Some(dir) => dir,
            None => std::env::current_dir()?,
        };
        if !of_own_repo(&dir, &canonical) {
            return Err(foreign_merge_request(flow, url));
        }
        *url = canonical;
    }
    let id = flow.id.clone();
    match args.event.into_kind() {
        Some(kind) => store.append(&id, &Event::new(now, kind)),
        None => store.beat(&id),
    }?;
    let events = store.events(&id)?;
    let beat = store.last_beat(&id)?;
    let reported = fold::fold(&id, &events, beat, now).expect("a resolved flow folds");
    Ok(flow::reported(&reported))
}
