use serde::Serialize;

use crate::view::{Board, FlowView, State};

pub const DESCRIPTION: &str =
    "Tracks parallel feature flows: what each is doing, where it is blocked, how far it got";

/// List views cut long texts to this many chars, with a hint; --full lifts it.
pub const LIST_TEXT_LIMIT: usize = 80;

/// What a successful command prints: TOON for agents, JSON for programs, a line for people (flow ui).
#[derive(Debug, PartialEq)]
pub enum Document {
    Toon(String),
    Json(String),
    Text(String),
}

/// An error an agent can act on: a message and next steps, printed as TOON on stdout.
#[derive(Debug, PartialEq)]
pub struct Failure {
    pub message: String,
    pub help: Vec<String>,
    pub code: u8,
}

impl Document {
    pub fn text(&self) -> &str {
        match self {
            Document::Toon(text) | Document::Json(text) | Document::Text(text) => text,
        }
    }
}

impl Failure {
    /// Exit code 1.
    pub fn error(message: impl Into<String>, help: Vec<String>) -> Failure {
        Failure {
            message: message.into(),
            help,
            code: 1,
        }
    }

    /// Exit code 2: clap's message and where to read the usage.
    pub fn usage(clap_error: &clap::Error) -> Failure {
        let rendered = clap_error.to_string();
        let message = rendered
            .lines()
            .next()
            .unwrap_or_default()
            .trim_start_matches("error: ")
            .to_string();
        let command = rendered
            .lines()
            .find_map(|line| line.strip_prefix("Usage: "))
            .map(|usage| {
                usage
                    .split_whitespace()
                    .take_while(|word| !word.starts_with(['[', '<']))
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .unwrap_or_else(|| "flow".to_string());
        Failure {
            message,
            help: vec![format!("Run `{command} --help` for the usage")],
            code: 2,
        }
    }

    /// Exit code 1: the error chain as the message.
    pub fn from_io(error: anyhow::Error) -> Failure {
        Failure::error(format!("{error:#}"), Vec::new())
    }

    /// error: ..., help[n]: ...
    pub fn to_toon(&self) -> String {
        #[derive(Serialize)]
        struct ErrorDocument<'a> {
            error: &'a str,
            #[serde(skip_serializing_if = "<[String]>::is_empty")]
            help: &'a [String],
        }
        toon(&ErrorDocument {
            error: &self.message,
            help: &self.help,
        })
    }
}

/// bin, description, then the list view.
pub fn home(board: &Board, bin: &str, warnings: &[String]) -> Document {
    list(board, Some(bin), warnings, false)
}

/// count, needs_you, flows[n]{name,state,phase,tasks,age}, blockers[n]{name,blocker}, warnings, help;
/// "flows: none tracked" when empty.
pub fn status_list(board: &Board, warnings: &[String], full: bool) -> Document {
    list(board, None, warnings, full)
}

const NEW_HELP: &str = "Run `flow new --name <name> --request <text>` to track a new flow";

#[derive(Serialize)]
struct ListDocument<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    bin: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<&'a str>,
    count: usize,
    needs_you: usize,
    flows: FlowRows,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    blockers: Vec<BlockerRow>,
    #[serde(skip_serializing_if = "<[String]>::is_empty")]
    warnings: &'a [String],
    help: Vec<String>,
}

#[derive(Serialize)]
#[serde(untagged)]
enum FlowRows {
    None(&'static str),
    Rows(Vec<FlowRow>),
}

#[derive(Serialize)]
struct FlowRow {
    name: String,
    state: String,
    phase: String,
    tasks: String,
    age: String,
}

#[derive(Serialize)]
struct BlockerRow {
    name: String,
    blocker: String,
}

fn list(board: &Board, bin: Option<&str>, warnings: &[String], full: bool) -> Document {
    let flows = if board.flows.is_empty() {
        FlowRows::None("none tracked")
    } else {
        FlowRows::Rows(
            board
                .flows
                .iter()
                .map(|flow| FlowRow {
                    name: flow.name.clone(),
                    state: name_of(&flow.state),
                    phase: flow
                        .phase
                        .map(|phase| phase.name().to_string())
                        .unwrap_or_default(),
                    tasks: tasks_text(flow),
                    age: short_duration(board.now.saturating_sub(flow.changed_at)),
                })
                .collect(),
        )
    };
    let blockers = board
        .flows
        .iter()
        .filter(|flow| flow.needs_you())
        .filter_map(|flow| {
            flow.blocker.as_ref().map(|blocker| BlockerRow {
                name: flow.name.clone(),
                blocker: truncated(blocker, LIST_TEXT_LIMIT, full),
            })
        })
        .collect();
    let mut help = Vec::new();
    if !board.flows.is_empty() {
        help.push(
            "Run `flow status <name>` for one flow's phases, timeline and decisions".to_string(),
        );
    }
    help.push(NEW_HELP.to_string());
    Document::Toon(toon(&ListDocument {
        bin,
        description: bin.map(|_| DESCRIPTION),
        count: board.count,
        needs_you: board.needs_you,
        flows,
        blockers,
        warnings,
        help,
    }))
}

/// One flow with its steps, decisions and timeline.
pub fn status_detail(flow: &FlowView, now: u64, full: bool) -> Document {
    #[derive(Serialize)]
    struct DetailDocument {
        name: String,
        id: String,
        request: String,
        mode: String,
        state: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        phase: Option<&'static str>,
        #[serde(skip_serializing_if = "Option::is_none")]
        blocker: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        delegate: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        tasks: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        review: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        merge_request: Option<String>,
        steps: Vec<StepRow>,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        decisions: Vec<String>,
        timeline: Vec<TimelineRow>,
        help: Vec<String>,
    }

    #[derive(Serialize)]
    struct StepRow {
        phase: &'static str,
        status: String,
        duration: String,
    }

    #[derive(Serialize)]
    struct TimelineRow {
        ago: String,
        text: String,
    }

    let cut = |text: &str| truncated(text, LIST_TEXT_LIMIT, full);
    let document = DetailDocument {
        name: flow.name.clone(),
        id: flow.id.clone(),
        request: cut(&flow.request),
        mode: name_of(&flow.mode),
        state: name_of(&flow.state),
        phase: flow.phase.map(|phase| phase.name()),
        blocker: flow.blocker.as_deref().map(cut),
        delegate: flow.delegate.as_deref().map(cut),
        tasks: flow.tasks.map(|_| tasks_text(flow)),
        review: flow
            .review
            .map(|review| format!("{} fixed, {} open", review.fixed, review.open)),
        merge_request: flow.merge_request.clone(),
        steps: flow
            .steps
            .iter()
            .map(|step| StepRow {
                phase: step.phase.name(),
                status: name_of(&step.status),
                duration: step.duration_ms.map(short_duration).unwrap_or_default(),
            })
            .collect(),
        decisions: flow
            .decisions
            .iter()
            .map(|decision| cut(decision))
            .collect(),
        timeline: flow
            .timeline
            .iter()
            .map(|entry| TimelineRow {
                ago: short_duration(now.saturating_sub(entry.at)),
                text: cut(&entry.text),
            })
            .collect(),
        help: detail_help(flow),
    };
    Document::Toon(toon(&document))
}

fn detail_help(flow: &FlowView) -> Vec<String> {
    let mut help = Vec::new();
    match flow.state {
        State::Blocked => help.push(format!(
            "Run `flow report --flow {} working` once the block is resolved",
            flow.name
        )),
        State::Waiting => help.push(format!(
            "Run `flow report --flow {} working` once answered",
            flow.name
        )),
        State::Done => help.push("Run `flow clear` after the MR is merged".to_string()),
        _ => {}
    }
    help.push("Run `flow` for all flows".to_string());
    help
}

fn tasks_text(flow: &FlowView) -> String {
    flow.tasks
        .map(|tasks| format!("{}/{}", tasks.done, tasks.total))
        .unwrap_or_default()
}

/// The kebab-case name serde gives a unit enum value.
fn name_of<T: Serialize>(value: &T) -> String {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(name)) => name,
        _ => String::new(),
    }
}

/// Whether flow ui opens a started worker now or once it runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Opening {
    Running,
    WhenUiRuns,
}

/// flow, id, worktree, branch, slot, setup (ran / none / failed: ...), agent, pane, help.
pub fn started(flow: &FlowView, setup: &str, opening: Opening) -> Document {
    #[derive(Serialize)]
    struct Started<'a> {
        flow: &'a str,
        id: &'a str,
        worktree: &'a str,
        branch: &'a str,
        slot: u8,
        setup: &'a str,
        agent: &'a str,
        pane: &'a str,
        help: Vec<String>,
    }
    let workspace = flow
        .workspace
        .as_ref()
        .expect("a started flow has a workspace");
    let (pane, watch) = match opening {
        Opening::Running => (
            "opening",
            format!("flow ui opens {} in its sidebar", flow.name),
        ),
        Opening::WhenUiRuns => (
            "when flow ui runs",
            "Run `flow ui` to open the worker".to_string(),
        ),
    };
    Document::Toon(toon(&Started {
        flow: &flow.name,
        id: &flow.id,
        worktree: &workspace.path,
        branch: &workspace.branch,
        slot: workspace.slot,
        setup,
        agent: &workspace.agent,
        pane,
        help: vec![watch, "Run `flow` for all flows".to_string()],
    }))
}

/// flow, id, existing (true when `new` found the unfinished flow of that name), how to use it.
pub fn created(flow: &FlowView, existing: bool) -> Document {
    #[derive(Serialize)]
    struct Created<'a> {
        flow: &'a str,
        id: &'a str,
        existing: bool,
        help: Vec<String>,
    }
    Document::Toon(toon(&Created {
        flow: &flow.name,
        id: &flow.id,
        existing,
        help: vec![
            format!(
                "Run `flow report --flow {} phase design` when design starts",
                flow.name
            ),
            format!(
                "Or set FLOW_ID={} in the worker's environment and leave out --flow",
                flow.id
            ),
        ],
    }))
}

/// flow, state, phase, and the next step that fits the new state.
pub fn reported(flow: &FlowView) -> Document {
    #[derive(Serialize)]
    struct Reported<'a> {
        flow: &'a str,
        state: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        phase: Option<&'static str>,
        help: Vec<String>,
    }
    let next = match flow.state {
        State::Blocked => "Run `flow report working` once the block is resolved".to_string(),
        State::Waiting => "Run `flow report working` once answered".to_string(),
        State::Done => "Run `flow clear` after the MR is merged".to_string(),
        _ => format!(
            "Run `flow status {}` for its phases and timeline",
            flow.name
        ),
    };
    Document::Toon(toon(&Reported {
        flow: &flow.name,
        state: name_of(&flow.state),
        phase: flow.phase.map(|phase| phase.name()),
        help: vec![next],
    }))
}

/// skipped: no flow id, and how to give one.
pub fn skipped() -> Document {
    #[derive(Serialize)]
    struct Skipped {
        skipped: &'static str,
        help: [&'static str; 1],
    }
    Document::Toon(toon(&Skipped {
        skipped: "no flow id",
        help: ["Set FLOW_ID or pass --flow <name>"],
    }))
}

/// removed: name, window (closed / gone / none), worktree (removed / kept: path / none), help.
pub fn removed(name: &str, window: &str, worktree: &str) -> Document {
    #[derive(Serialize)]
    struct Removed<'a> {
        removed: &'a str,
        window: &'a str,
        worktree: &'a str,
        help: Vec<String>,
    }
    let mut help = Vec::new();
    if let Some(path) = worktree.strip_prefix("kept: ") {
        help.push(format!(
            "Run `git worktree remove --force {path}` to discard what is left"
        ));
    }
    help.push("Run `flow` to see what is left".to_string());
    Document::Toon(toon(&Removed {
        removed: name,
        window,
        worktree,
        help,
    }))
}

/// cleared: n, names[n], kept[n]{name,path} for worktrees with uncommitted work, help.
pub fn cleared(names: &[String], kept: &[(String, String)]) -> Document {
    #[derive(Serialize)]
    struct Kept<'a> {
        name: &'a str,
        path: &'a str,
    }
    #[derive(Serialize)]
    struct Cleared<'a> {
        cleared: usize,
        #[serde(skip_serializing_if = "<[String]>::is_empty")]
        names: &'a [String],
        #[serde(skip_serializing_if = "Vec::is_empty")]
        kept: Vec<Kept<'a>>,
        help: Vec<&'static str>,
    }
    let mut help = vec!["Run `flow` to see what is left"];
    if !kept.is_empty() {
        help.push("Commit or discard the work in a kept worktree, then remove it with `git worktree remove <path>`");
    }
    Document::Toon(toon(&Cleared {
        cleared: names.len(),
        names,
        kept: kept
            .iter()
            .map(|(name, path)| Kept { name, path })
            .collect(),
        help,
    }))
}

/// 45s, 12m, 3h, 2d.
pub fn short_duration(ms: u64) -> String {
    let seconds = ms / 1_000;
    match seconds {
        0..60 => format!("{seconds}s"),
        60..3_600 => format!("{}m", seconds / 60),
        3_600..86_400 => format!("{}h", seconds / 3_600),
        _ => format!("{}d", seconds / 86_400),
    }
}

/// Text cut to `limit` chars with " (truncated, N chars - use --full)" unless `full`.
pub fn truncated(text: &str, limit: usize, full: bool) -> String {
    let length = text.chars().count();
    if full || length <= limit {
        return text.to_string();
    }
    let kept: String = text.chars().take(limit).collect();
    format!("{kept} (truncated, {length} chars - use --full)")
}

fn toon<T: Serialize>(document: &T) -> String {
    serde_toon::to_string(document).expect("a document always encodes")
}

#[cfg(test)]
mod tests {
    use clap::{CommandFactory, Parser, Subcommand};

    use super::*;

    #[derive(Debug, Parser)]
    #[command(name = "flow")]
    struct Cli {
        #[command(subcommand)]
        command: Command,
    }

    #[derive(Debug, Subcommand)]
    enum Command {
        Report {
            #[arg(long)]
            flow: Option<String>,
        },
    }

    use crate::event::{Event, EventKind, Mode, Phase};
    use crate::fold;

    const MIN: u64 = 60_000;

    fn flow(name: &str, events: Vec<EventKind>, now: u64) -> FlowView {
        let mut all = vec![Event::new(
            0,
            EventKind::Started {
                name: name.into(),
                request: format!("{} add CSV export", name.to_uppercase()),
                mode: Mode::Auto,
            },
        )];
        all.extend(
            events
                .into_iter()
                .enumerate()
                .map(|(step, kind)| Event::new((step as u64 + 1) * MIN, kind)),
        );
        let mut view = fold::fold(&format!("{name}-id"), &all, None, now).unwrap();
        view.id = format!("{name}-id");
        view
    }

    fn sample_board(now: u64) -> Board {
        fold::board(
            vec![
                flow(
                    "shop-221",
                    vec![
                        EventKind::Phase {
                            phase: Phase::Implement,
                        },
                        EventKind::Tasks { done: 4, total: 9 },
                        EventKind::Blocked {
                            reason: "Task 5: migration needs a default value".into(),
                        },
                    ],
                    now,
                ),
                flow(
                    "shop-230",
                    vec![EventKind::Phase {
                        phase: Phase::Design,
                    }],
                    now,
                ),
                flow(
                    "export-csv",
                    vec![EventKind::MergeRequest {
                        url: "https://x/88".into(),
                    }],
                    now,
                ),
            ],
            now,
        )
    }

    #[test]
    fn home_shows_the_binary_the_description_and_the_flows() {
        let Document::Toon(text) = home(&sample_board(10 * MIN), "~/.cargo/bin/flow", &[]) else {
            panic!("home is TOON");
        };

        assert_eq!(
            text,
            [
                "bin: ~/.cargo/bin/flow",
                "description: \"Tracks parallel feature flows: what each is doing, where it is blocked, how far it got\"",
                "count: 3",
                "needs_you: 1",
                "flows[3]{name,state,phase,tasks,age}:",
                "  shop-221,blocked,implement,4/9,7m",
                "  shop-230,working,design,\"\",9m",
                "  export-csv,done,merge-request,\"\",9m",
                "blockers[1]{name,blocker}:",
                "  shop-221,\"Task 5: migration needs a default value\"",
                "help[2]: \"Run `flow status <name>` for one flow's phases, timeline and decisions\",Run `flow new --name <name> --request <text>` to track a new flow",
            ]
            .join("\n")
        );
    }

    #[test]
    fn an_empty_list_says_so_and_points_to_flow_new() {
        let board = fold::board(Vec::new(), 0);

        assert_eq!(
            status_list(&board, &[], false).text(),
            [
                "count: 0",
                "needs_you: 0",
                "flows: none tracked",
                "help[1]: Run `flow new --name <name> --request <text>` to track a new flow",
            ]
            .join("\n")
        );
    }

    #[test]
    fn the_list_truncates_long_blockers_and_shows_warnings() {
        let reason = "b".repeat(LIST_TEXT_LIMIT + 1);
        let board = fold::board(
            vec![flow(
                "shop-221",
                vec![EventKind::Blocked {
                    reason: reason.clone(),
                }],
                MIN,
            )],
            MIN,
        );
        let warnings = vec!["cannot read 9a: Permission denied".to_string()];

        let short = status_list(&board, &warnings, false);
        let full = status_list(&board, &warnings, true);

        assert!(
            short.text().contains(&format!(
                "{} (truncated, 81 chars - use --full)",
                "b".repeat(LIST_TEXT_LIMIT)
            )),
            "{}",
            short.text()
        );
        assert!(
            full.text().contains(&format!("shop-221,{reason}")),
            "{}",
            full.text()
        );
        assert!(
            short
                .text()
                .contains("warnings[1]: \"cannot read 9a: Permission denied\""),
            "{}",
            short.text()
        );
    }

    #[test]
    fn the_detail_shows_steps_decisions_timeline_and_the_next_step() {
        let flow = flow(
            "shop-221",
            vec![
                EventKind::Phase {
                    phase: Phase::Design,
                },
                EventKind::Decision {
                    text: "approach B".into(),
                },
                EventKind::Phase {
                    phase: Phase::Implement,
                },
                EventKind::Tasks { done: 4, total: 9 },
                EventKind::Blocked {
                    reason: "no default".into(),
                },
            ],
            10 * MIN,
        );

        assert_eq!(
            status_detail(&flow, 10 * MIN, false).text(),
            [
                "name: shop-221",
                "id: shop-221-id",
                "request: SHOP-221 add CSV export",
                "mode: auto",
                "state: blocked",
                "phase: implement",
                "blocker: no default",
                "tasks: 4/9",
                "steps[4]{phase,status,duration}:",
                "  design,done,2m",
                "  implement,current,7m",
                "  review,open,\"\"",
                "  merge-request,open,\"\"",
                "decisions[1]: approach B",
                "timeline[5]{ago,text}:",
                "  10m,started",
                "  9m,\"phase: design\"",
                "  8m,\"decided: approach B\"",
                "  7m,\"phase: implement\"",
                "  5m,\"blocked: no default\"",
                "help[2]: Run `flow report --flow shop-221 working` once the block is resolved,Run `flow` for all flows",
            ]
            .join("\n")
        );
    }

    #[test]
    fn created_names_the_flow_and_how_to_report_into_it() {
        let flow = flow("shop-221", vec![], MIN);

        assert_eq!(
            created(&flow, false).text(),
            [
                "flow: shop-221",
                "id: shop-221-id",
                "existing: false",
                "help[2]: Run `flow report --flow shop-221 phase design` when design starts,Or set FLOW_ID=shop-221-id in the worker's environment and leave out --flow",
            ]
            .join("\n")
        );
        assert!(created(&flow, true).text().contains("existing: true"));
    }

    #[test]
    fn reported_answers_with_the_new_state_and_what_comes_next() {
        let blocked = flow(
            "shop-221",
            vec![
                EventKind::Phase {
                    phase: Phase::Implement,
                },
                EventKind::Blocked { reason: "x".into() },
            ],
            MIN,
        );
        assert_eq!(
            reported(&blocked).text(),
            [
                "flow: shop-221",
                "state: blocked",
                "phase: implement",
                "help[1]: Run `flow report working` once the block is resolved",
            ]
            .join("\n")
        );

        let waiting = flow(
            "shop-221",
            vec![EventKind::Waiting {
                question: "ok?".into(),
            }],
            MIN,
        );
        assert!(
            reported(&waiting)
                .text()
                .ends_with("help[1]: Run `flow report working` once answered")
        );

        let done = flow(
            "shop-221",
            vec![EventKind::MergeRequest {
                url: "https://x/1".into(),
            }],
            MIN,
        );
        assert!(
            reported(&done)
                .text()
                .ends_with("help[1]: Run `flow clear` after the MR is merged")
        );

        let working = flow("shop-221", vec![], MIN);
        assert_eq!(
            reported(&working).text(),
            "flow: shop-221\nstate: working\nhelp[1]: Run `flow status shop-221` for its phases and timeline"
        );
    }

    #[test]
    fn skipped_says_why_and_how_to_give_a_flow() {
        assert_eq!(
            skipped().text(),
            "skipped: no flow id\nhelp[1]: Set FLOW_ID or pass --flow <name>"
        );
    }

    #[test]
    fn cleared_counts_and_names_the_removed_flows() {
        assert_eq!(
            cleared(&["export-csv".into(), "shop-198".into()], &[]).text(),
            "cleared: 2\nnames[2]: export-csv,shop-198\nhelp[1]: Run `flow` to see what is left"
        );
        assert_eq!(
            cleared(&[], &[]).text(),
            "cleared: 0\nhelp[1]: Run `flow` to see what is left"
        );
    }

    #[test]
    fn prints_an_error_with_next_steps_as_toon() {
        let failure = Failure::error(
            "no flow named shop-999",
            vec!["Run `flow` to list the flows".into()],
        );

        assert_eq!(failure.code, 1);
        assert_eq!(
            failure.to_toon(),
            "error: no flow named shop-999\nhelp[1]: Run `flow` to list the flows"
        );
    }

    #[test]
    fn turns_a_clap_error_into_a_usage_failure_naming_the_command() {
        Cli::command().debug_assert();
        let error = Cli::try_parse_from(["flow", "report", "--nope"]).unwrap_err();

        let failure = Failure::usage(&error);

        assert_eq!(failure.code, 2);
        assert_eq!(failure.message, "unexpected argument '--nope' found");
        assert_eq!(failure.help, vec!["Run `flow report --help` for the usage"]);
    }

    #[test]
    fn names_the_whole_error_chain_for_io_failures() {
        let error = anyhow::anyhow!("Permission denied").context("could not create flow in /x");

        let failure = Failure::from_io(error);

        assert_eq!(failure.code, 1);
        assert_eq!(
            failure.message,
            "could not create flow in /x: Permission denied"
        );
        assert_eq!(
            failure.to_toon(),
            "error: \"could not create flow in /x: Permission denied\""
        );
    }

    #[test]
    fn shortens_durations_to_one_unit() {
        assert_eq!(short_duration(0), "0s");
        assert_eq!(short_duration(45_000), "45s");
        assert_eq!(short_duration(12 * 60_000 + 59_000), "12m");
        assert_eq!(short_duration(3 * 3_600_000), "3h");
        assert_eq!(short_duration(49 * 3_600_000), "2d");
    }

    #[test]
    fn truncates_long_texts_with_a_hint_unless_full() {
        let long = "x".repeat(LIST_TEXT_LIMIT + 5);

        assert_eq!(
            truncated(&long, LIST_TEXT_LIMIT, false),
            format!(
                "{} (truncated, 85 chars - use --full)",
                "x".repeat(LIST_TEXT_LIMIT)
            )
        );
        assert_eq!(truncated(&long, LIST_TEXT_LIMIT, true), long);
        assert_eq!(truncated("short", LIST_TEXT_LIMIT, false), "short");
    }
}
