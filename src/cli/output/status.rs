//! The status documents: the home view, the list, every project (`--all`), one flow's detail

use serde::Serialize;

use super::{DESCRIPTION, Document, toon};
use crate::config::prefix::Prefix;
use crate::event::ServiceStatus;
use crate::text::short_duration;
use crate::text::{LIST_TEXT_LIMIT, truncated};
use crate::view::{Board, FlowView, Projects, State};
use crate::workflow::Phase;

/// bin, description, then the list view.
pub(crate) fn home(board: &Board, bin: &str, warnings: &[String]) -> Document {
    board_list(board, Some(bin), warnings, false)
}

/// count, needs_you, flows[n]{name,kind,state,phase,tasks,age}, blockers[n]{name,blocker}, warnings, help;
/// "flows: none tracked" when empty.
pub(crate) fn list(board: &Board, warnings: &[String], full: bool) -> Document {
    board_list(board, None, warnings, full)
}

const NEW_HELP: &str = "Run `saaio new --name <name> --request <text>` to track a new flow";

#[derive(Debug, Serialize)]
struct ListDocument<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    bin: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<&'a str>,
    count: usize,
    needs_you: usize,
    flows: FlowRows<'a>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    blockers: Vec<BlockerRow<'a>>,
    #[serde(skip_serializing_if = "<[String]>::is_empty")]
    warnings: &'a [String],
    help: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
enum FlowRows<'a> {
    None(&'static str),
    Rows(Vec<FlowRow<'a>>),
}

#[derive(Debug, Serialize)]
struct FlowRow<'a> {
    name: &'a str,
    kind: &'a str,
    state: &'static str,
    phase: &'a str,
    tasks: String,
    age: String,
    services: &'static str,
}

#[derive(Debug, Serialize)]
struct BlockerRow<'a> {
    name: &'a str,
    blocker: String,
}

fn board_list(board: &Board, bin: Option<&str>, warnings: &[String], full: bool) -> Document {
    let flows = if board.flows.is_empty() {
        FlowRows::None("none tracked")
    } else {
        FlowRows::Rows(
            board
                .flows
                .iter()
                .map(|flow| FlowRow {
                    name: &flow.name,
                    kind: flow.kind.as_deref().unwrap_or_default(),
                    state: flow.state.name(),
                    phase: flow.phase.as_ref().map_or("", Phase::name),
                    tasks: flow
                        .tasks
                        .map(|tasks| tasks.to_string())
                        .unwrap_or_default(),
                    age: short_duration(board.now.saturating_sub(flow.changed_at)),
                    services: services(flow),
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
                name: &flow.name,
                blocker: truncated(blocker, LIST_TEXT_LIMIT, full),
            })
        })
        .collect();
    let mut help = Vec::new();
    if !board.flows.is_empty() {
        help.push(
            "Run `saaio status <name>` for one flow's phases, timeline and decisions".to_string(),
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

/// Where a flow's services stand: empty while they run or were never paused, else paused,
/// pausing, resuming, pause-failed or resume-failed.
fn services(flow: &FlowView) -> &'static str {
    match &flow.services {
        Some(services) if services.status != ServiceStatus::Running => services.status.name(),
        _ => "",
    }
}

/// Every project's flows in one list with a project column, blockers likewise.
pub(crate) fn all(projects: &Projects, warnings: &[String], full: bool) -> Document {
    #[derive(Serialize)]
    struct AllDocument<'a> {
        count: usize,
        needs_you: usize,
        flows: AllRows<'a>,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        blockers: Vec<AllBlockerRow<'a>>,
        #[serde(skip_serializing_if = "<[String]>::is_empty")]
        warnings: &'a [String],
        help: Vec<String>,
    }
    #[derive(Serialize)]
    #[serde(untagged)]
    enum AllRows<'a> {
        None(&'static str),
        Rows(Vec<AllRow<'a>>),
    }
    #[derive(Serialize)]
    struct AllRow<'a> {
        project: &'a str,
        name: &'a str,
        kind: &'a str,
        state: &'static str,
        phase: &'a str,
        tasks: String,
        age: String,
        services: &'static str,
    }
    #[derive(Serialize)]
    struct AllBlockerRow<'a> {
        project: &'a str,
        name: &'a str,
        blocker: String,
    }

    let flows: Vec<(&str, &FlowView)> = projects
        .projects
        .iter()
        .flat_map(|project| {
            project
                .board
                .flows
                .iter()
                .map(|flow| (project.name.as_str(), flow))
        })
        .collect();
    let rows = flows
        .iter()
        .map(|&(project, flow)| AllRow {
            project,
            name: &flow.name,
            kind: flow.kind.as_deref().unwrap_or_default(),
            state: flow.state.name(),
            phase: flow.phase.as_ref().map_or("", Phase::name),
            tasks: flow
                .tasks
                .map(|tasks| tasks.to_string())
                .unwrap_or_default(),
            age: short_duration(projects.now.saturating_sub(flow.changed_at)),
            services: services(flow),
        })
        .collect::<Vec<_>>();
    let blockers = flows
        .iter()
        .filter(|(_, flow)| flow.needs_you())
        .filter_map(|&(project, flow)| {
            flow.blocker.as_ref().map(|blocker| AllBlockerRow {
                project,
                name: &flow.name,
                blocker: truncated(blocker, LIST_TEXT_LIMIT, full),
            })
        })
        .collect();
    let mut help = Vec::new();
    if !rows.is_empty() {
        help.push(
            "Run `saaio status <name>` in a flow's project for its phases, timeline and decisions"
                .to_string(),
        );
    }
    help.push("Run `saaio status --all --json` for every project's board".to_string());
    Document::Toon(toon(&AllDocument {
        count: rows.len(),
        needs_you: flows.iter().filter(|(_, flow)| flow.needs_you()).count(),
        flows: if rows.is_empty() {
            AllRows::None("none tracked")
        } else {
            AllRows::Rows(rows)
        },
        blockers,
        warnings,
        help,
    }))
}

/// One flow with its steps, decisions and timeline, and for a started flow where its code is:
/// branch, base, worktree and whether the worktree is still there (`worktree_exists`), or a plain
/// session's folder.
#[expect(
    clippy::too_many_lines,
    reason = "one document, its fields in the order an agent reads them"
)]
pub(crate) fn detail(
    flow: &FlowView,
    now: u64,
    full: bool,
    worktree_exists: Option<bool>,
    prefix: Prefix,
) -> Document {
    #[derive(Serialize)]
    struct DetailDocument {
        name: String,
        id: String,
        #[serde(skip_serializing_if = "String::is_empty")]
        request: String,
        mode: &'static str,
        #[serde(skip_serializing_if = "Option::is_none")]
        kind: Option<String>,
        state: &'static str,
        #[serde(skip_serializing_if = "Option::is_none")]
        phase: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        blocker: Option<String>,
        #[serde(skip_serializing_if = "str::is_empty")]
        services: &'static str,
        #[serde(skip_serializing_if = "Option::is_none")]
        services_error: Option<String>,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        questions: Vec<QuestionRow>,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        options: Vec<OptionRow>,
        #[serde(skip_serializing_if = "Option::is_none")]
        delegate: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        tasks: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        review: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        merge_request: Option<String>,
        #[serde(skip_serializing_if = "std::ops::Not::not")]
        merged: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        folder: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        branch: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        base: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        worktree: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        worktree_exists: Option<bool>,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        steps: Vec<StepRow>,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        decisions: Vec<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        answer: Option<String>,
        timeline: Vec<TimelineRow>,
        help: Vec<String>,
    }

    #[derive(Serialize)]
    struct StepRow {
        phase: String,
        status: &'static str,
        duration: String,
    }

    #[derive(Serialize)]
    struct TimelineRow {
        ago: String,
        text: String,
    }

    #[derive(Serialize)]
    struct QuestionRow {
        n: usize,
        header: String,
        question: String,
        pick: &'static str,
    }

    #[derive(Serialize)]
    struct OptionRow {
        question: usize,
        n: usize,
        label: String,
        description: String,
    }

    let cut = |text: &str| truncated(text, LIST_TEXT_LIMIT, full);
    let (plain, worktree) = match &flow.workspace {
        Some(space) if flow.is_plain() => (Some(space), None),
        space => (None, space.as_ref()),
    };
    let document = DetailDocument {
        name: flow.name.clone(),
        id: flow.id.clone(),
        request: cut(&flow.request),
        mode: flow.mode.name(),
        kind: flow.kind.clone(),
        state: flow.state.name(),
        phase: flow.phase.as_ref().map(|phase| phase.name().to_string()),
        blocker: flow.blocker.as_deref().map(cut),
        services: services(flow),
        services_error: flow
            .services
            .as_ref()
            .and_then(|services| services.reason.as_deref())
            .map(cut),
        questions: (1..)
            .zip(&flow.questions)
            .map(|(n, asked)| QuestionRow {
                n,
                header: cut(&asked.header),
                question: cut(&asked.question),
                pick: if asked.multi_select { "several" } else { "one" },
            })
            .collect(),
        options: (1..)
            .zip(&flow.questions)
            .flat_map(|(question, asked)| {
                (1..).zip(&asked.options).map(move |(n, choice)| OptionRow {
                    question,
                    n,
                    label: cut(&choice.label),
                    description: cut(&choice.description),
                })
            })
            .collect(),
        delegate: flow.delegate.as_deref().map(cut),
        tasks: flow.tasks.map(|tasks| tasks.to_string()),
        review: flow.review.map(|review| review.to_string()),
        merge_request: flow.merge_request.clone(),
        merged: flow.merged,
        folder: plain.map(|space| space.path.clone()),
        branch: worktree.and_then(|space| space.branch.clone()),
        base: worktree.and_then(|space| space.base.clone()),
        worktree: worktree.map(|space| space.path.clone()),
        worktree_exists: worktree.and(worktree_exists),
        steps: flow
            .steps
            .iter()
            .map(|step| StepRow {
                phase: step.phase.name().to_string(),
                status: step.status.name(),
                duration: step.duration_ms.map(short_duration).unwrap_or_default(),
            })
            .collect(),
        decisions: flow
            .decisions
            .iter()
            .map(|decision| cut(decision))
            .collect(),
        answer: flow.answer.as_deref().map(cut),
        timeline: flow
            .timeline
            .iter()
            .map(|entry| TimelineRow {
                ago: short_duration(now.saturating_sub(entry.at)),
                text: cut(&entry.text),
            })
            .collect(),
        help: detail_help(flow, worktree_exists, prefix),
    };
    Document::Toon(toon(&document))
}

fn detail_help(flow: &FlowView, worktree_exists: Option<bool>, prefix: Prefix) -> Vec<String> {
    let mut help = Vec::new();
    if let Some(space) = &flow.workspace
        && flow.state == State::Ended
    {
        help.push(format!("{} in saaio resumes it", prefix.then("r")));
        if let Some(branch) = &space.branch
            && worktree_exists == Some(false)
        {
            help.push(format!(
                "Its worktree is gone; its commits stay on {branch}"
            ));
        }
    }
    match flow.state {
        State::Blocked => help.push(format!(
            "Run `saaio report --flow {} working` once the block is resolved",
            flow.name
        )),
        State::Waiting if !flow.questions.is_empty() => help.push(format!(
            "Answer every question in one `saaio send {} \"<answer>\"`; saaio closes the dialog and the worker reads it",
            flow.name
        )),
        State::Waiting => help.push(format!(
            "Run `saaio report --flow {} working` once answered",
            flow.name
        )),
        State::Done => help.push(super::flow::after_done(flow)),
        _ => {}
    }
    help.push("Run `saaio` for all flows".to_string());
    help
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::output::fixtures::{MIN, flow, sample_board};
    use crate::event::{EventKind, Question};
    use crate::fold;
    use crate::view::VIEW_VERSION;
    use crate::workflow::Phase;

    #[test]
    fn home_shows_the_binary_the_description_and_the_flows() {
        let Document::Toon(text) = home(&sample_board(10 * MIN), "~/.cargo/bin/saaio", &[]) else {
            panic!("home is TOON");
        };

        assert_eq!(
            text,
            [
                "bin: ~/.cargo/bin/saaio",
                "description: \"Tracks parallel feature flows: what each is doing, where it is blocked, how far it got\"",
                "count: 3",
                "needs_you: 1",
                "flows[3]{name,kind,state,phase,tasks,age,services}:",
                "  shop-221,fix,blocked,implement,4/9,7m,\"\"",
                "  shop-230,feature,working,plan,\"\",9m,\"\"",
                "  export-csv,\"\",done,merge-request,\"\",9m,paused",
                "blockers[1]{name,blocker}:",
                "  shop-221,\"Task 5: migration needs a default value\"",
                "help[2]: \"Run `saaio status <name>` for one flow's phases, timeline and decisions\",Run `saaio new --name <name> --request <text>` to track a new flow",
            ]
            .join("\n")
        );
    }

    #[test]
    fn an_empty_list_says_so_and_points_to_flow_new() {
        let board = fold::board(Vec::new(), 0);

        assert_eq!(
            list(&board, &[], false).text(),
            [
                "count: 0",
                "needs_you: 0",
                "flows: none tracked",
                "help[1]: Run `saaio new --name <name> --request <text>` to track a new flow",
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

        let short = list(&board, &warnings, false);
        let full = list(&board, &warnings, true);

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
    fn the_overview_lists_every_projects_flows_with_their_project() {
        let now = 30 * MIN;
        let projects = Projects {
            version: VIEW_VERSION,
            now,
            projects: vec![
                crate::view::Project {
                    name: "api".into(),
                    root: "/state/saaio/projects/api".into(),
                    board: fold::board(vec![flow("api-7", vec![], now)], now),
                },
                crate::view::Project {
                    name: "shop".into(),
                    root: "/state/saaio/projects/shop".into(),
                    board: sample_board(now),
                },
            ],
        };

        let document = all(&projects, &["web: cannot read x".into()], false);
        let text = document.text();

        assert!(text.starts_with("count: 4\nneeds_you: 1\n"), "{text}");
        assert!(
            text.contains("flows[4]{project,name,kind,state,phase,tasks,age,services}:"),
            "{text}"
        );
        assert!(text.contains("  api,api-7,\"\",working,"), "{text}");
        assert!(
            text.contains("  shop,shop-221,fix,blocked,implement,4/9,"),
            "{text}"
        );
        assert!(
            text.contains("blockers[1]{project,name,blocker}:\n  shop,shop-221,"),
            "{text}"
        );
        assert!(text.contains("web: cannot read x"), "{text}");
        assert!(
            text.contains("  shop,export-csv,\"\",done,merge-request,\"\",29m,paused"),
            "{text}"
        );
    }

    #[test]
    fn an_empty_overview_says_so() {
        let projects = Projects {
            version: VIEW_VERSION,
            now: 0,
            projects: vec![],
        };

        let document = all(&projects, &[], false);
        let text = document.text();

        assert!(text.contains("flows: none tracked"), "{text}");
        assert!(!text.contains("saaio status <name>"), "{text}");
    }

    #[test]
    fn the_detail_says_where_paused_services_stand_and_why_a_resume_failed() {
        let services = |status, reason: Option<&str>| EventKind::Services {
            status,
            reason: reason.map(str::to_string),
        };
        let text = |events| {
            detail(
                &flow("shop-221", events, 9 * MIN),
                9 * MIN,
                false,
                None,
                Prefix::default(),
            )
            .text()
            .to_string()
        };

        let paused = text(vec![services(ServiceStatus::Paused, None)]);
        assert!(paused.contains("\nservices: paused\n"), "{paused}");
        assert!(!paused.contains("services_error"), "{paused}");
        let failed = text(vec![services(
            ServiceStatus::ResumeFailed,
            Some("port 8003 is taken"),
        )]);
        assert!(failed.contains("\nservices: resume-failed\n"), "{failed}");
        assert!(
            failed.contains("\nservices_error: port 8003 is taken\n"),
            "{failed}"
        );
        let running = text(vec![services(ServiceStatus::Running, None)]);
        assert!(!running.contains("services"), "{running}");
    }

    #[test]
    fn the_detail_names_the_kind_after_the_mode() {
        let mut fix = flow("shop-230", vec![], MIN);
        fix.kind = Some("fix".into());

        let document = detail(&fix, MIN, false, None, Prefix::default());
        let text = document.text();

        assert!(
            text.contains("\nmode: auto\nkind: fix\nstate: working\n"),
            "{text}"
        );
    }

    #[test]
    fn the_detail_shows_steps_decisions_timeline_and_the_next_step() {
        let flow = flow(
            "shop-221",
            vec![
                EventKind::Phase {
                    phase: Phase::of("plan"),
                },
                EventKind::Decision {
                    text: "approach B".into(),
                },
                EventKind::Phase {
                    phase: Phase::of("implement"),
                },
                EventKind::Tasks { done: 4, total: 9 },
                EventKind::Blocked {
                    reason: "no default".into(),
                },
            ],
            10 * MIN,
        );

        assert_eq!(
            detail(&flow, 10 * MIN, false, None, Prefix::default()).text(),
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
                "  plan,done,2m",
                "  implement,current,7m",
                "  review,open,\"\"",
                "  merge-request,open,\"\"",
                "decisions[1]: approach B",
                "timeline[5]{ago,text}:",
                "  10m,started",
                "  9m,\"phase: plan\"",
                "  8m,\"decided: approach B\"",
                "  7m,\"phase: implement\"",
                "  5m,\"blocked: no default\"",
                "help[2]: Run `saaio report --flow shop-221 working` once the block is resolved,Run `saaio` for all flows",
            ]
            .join("\n")
        );
    }

    #[test]
    fn the_detail_lists_a_dialogs_questions_and_options() {
        let mut toppings = Question::of("Which toppings?", &["Cheese", "Olives"], true);
        toppings.options[1].description = format!("Black {}", "olives ".repeat(20));
        let flow = flow(
            "ask",
            vec![EventKind::Waiting {
                question: "Which color?".into(),
                questions: vec![
                    Question::of("Which color?", &["Red", "Blue"], false),
                    toppings,
                ],
                permission: false,
            }],
            2 * MIN,
        );

        let text = detail(&flow, 2 * MIN, false, None, Prefix::default())
            .text()
            .to_string();

        assert!(
            text.contains(
                &[
                    "blocker: Which color?",
                    "questions[2]{n,header,question,pick}:",
                    "  1,Which,Which color?,one",
                    "  2,Which,Which toppings?,several",
                    "options[4]{question,n,label,description}:",
                    "  1,1,Red,Red it is",
                    "  1,2,Blue,Blue it is",
                    "  2,1,Cheese,Cheese it is",
                    "  2,2,Olives,\"Black olives olives",
                ]
                .join("\n")
            ),
            "{text}"
        );
        assert!(text.contains("use --full"), "{text}");
        assert!(
            text.ends_with(
                "help[2]: \"Answer every question in one `saaio send ask \\\"<answer>\\\"`; saaio closes the dialog and the worker reads it\",Run `saaio` for all flows"
            ),
            "{text}"
        );
        let full = detail(&flow, 2 * MIN, true, None, Prefix::default())
            .text()
            .to_string();
        assert!(!full.contains("use --full"), "{full}");
    }

    #[test]
    fn the_detail_says_a_pr_open_worker_must_end_before_clear() {
        let url = || "https://github.com/acme/shop/pull/42".to_string();
        let open = flow(
            "shop-221",
            vec![EventKind::MergeRequest { url: url() }],
            MIN,
        );
        let merged = flow(
            "shop-221",
            vec![EventKind::Merged {
                url: url(),
                into: None,
            }],
            MIN,
        );

        let open = detail(&open, MIN, false, None, Prefix::default())
            .text()
            .to_string();
        let merged = detail(&merged, MIN, false, None, Prefix::default())
            .text()
            .to_string();

        assert!(!open.contains("merged: true"), "{open}");
        assert!(
            open.contains("Run `saaio clear` after the worker session ends"),
            "{open}"
        );
        assert!(
            merged.contains(
                "merge_request: \"https://github.com/acme/shop/pull/42\"\nmerged: true\n"
            ),
            "{merged}"
        );
        assert!(
            merged.contains("Run `saaio clear` after the worker session ends"),
            "{merged}"
        );
    }

    #[test]
    fn the_detail_shows_the_last_answer() {
        let mut answered = flow("shop-217", vec![], 10 * MIN);
        answered.answer = Some(format!("Done. {}", "x".repeat(LIST_TEXT_LIMIT)));

        let text = detail(&answered, 10 * MIN, false, None, Prefix::default())
            .text()
            .to_string();
        assert!(text.contains("answer: \"Done. x"), "{text}");
        assert!(text.contains("use --full"), "{text}");
        let full = detail(&answered, 10 * MIN, true, None, Prefix::default())
            .text()
            .to_string();
        assert!(!full.contains("use --full"), "{full}");
    }

    #[test]
    fn the_detail_cuts_a_long_request_that_full_shows_whole() {
        let mut long = flow("shop-300", vec![], MIN);
        long.request = format!("SHOP-300 {} the tail", "word ".repeat(120));

        let short = detail(&long, MIN, false, None, Prefix::default())
            .text()
            .to_string();
        assert!(
            short.contains("(truncated, 618 chars - use --full)"),
            "{short}"
        );
        assert!(!short.contains("the tail"), "{short}");
        let full = detail(&long, MIN, true, None, Prefix::default())
            .text()
            .to_string();
        assert!(full.contains(&format!("{}\n", long.request)), "{full}");
    }
}
