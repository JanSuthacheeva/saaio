use clap::Subcommand;

use super::{no_flow, views};
use crate::cli::output::{self, Document, Failure};
use crate::event::{Event, EventKind, Phase};
use crate::fold;
use crate::store::Store;

#[derive(Debug, clap::Args)]
pub struct Args {
    /// Flow name or id [default: $FLOW_ID]
    #[arg(long, env = "FLOW_ID")]
    flow: Option<String>,
    #[command(subcommand)]
    event: ReportEvent,
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

/// Appends one event to a flow; skipped without a flow, so an agent outside flow can report.
pub fn run(args: Args, store: &Store, now: u64) -> Result<Document, Failure> {
    let Some(name) = args.flow.filter(|name| !name.is_empty()) else {
        return Ok(output::skipped());
    };
    let (flows, _) = views(store, now);
    let id = fold::resolve(&flows, &name)
        .ok_or_else(|| no_flow(&name))?
        .id
        .clone();
    match args.event.into_kind() {
        Some(kind) => store.append(&id, &Event::new(now, kind)),
        None => store.beat(&id),
    }?;
    let events = store.events(&id)?;
    let beat = store.last_beat(&id)?;
    let reported = fold::fold(&id, &events, beat, now).expect("a resolved flow folds");
    Ok(output::reported(&reported))
}
