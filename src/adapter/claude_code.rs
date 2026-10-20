use std::path::Path;

use serde_json::{Value, json};

use crate::adapter::answer::turn_end;
use crate::adapter::calls::{created_merge_request, tool_call};
use crate::adapter::{Adapter, Turn, cut};
use crate::event::{Choice, EventKind, Question};
use crate::workflow::Workflow;

/// The hook events saaio listens to.
const EVENTS: [&str; 8] = [
    "UserPromptSubmit",
    "PreToolUse",
    "PostToolUse",
    "PermissionRequest",
    "Stop",
    "SessionEnd",
    "PreCompact",
    "PostCompact",
];
/// Seconds Claude Code waits for `saaio hook` before it goes on without it.
const HOOK_TIMEOUT_SECS: u64 = 10;
/// Seconds Claude Code waits for `saaio hook` before a tool call: long enough for paused services
/// to come back (hook::RESUME_WAIT).
const TOOL_HOOK_TIMEOUT_SECS: u64 = 150;
/// Seconds Claude Code lets an asyncRewake waiter sleep before it ends it: a week.
const WAIT_TIMEOUT_SECS: u64 = 604_800;
/// The events that arm a session's waiter: its start (the master starts without a prompt) and
/// every turn end.
const WAIT_EVENTS: [&str; 2] = ["SessionStart", "Stop"];
/// plan-progress's report tool, whose answers carry the plan's task counts.
const PLAN_PROGRESS_TOOL: &str = "mcp__plan-progress__report";

/// The --settings JSON: report_hooks, and each event in WAIT_EVENTS also arms `<flow_exe> wait`.
pub fn settings(flow_exe: &Path) -> String {
    let mut hooks = report_hooks(flow_exe);
    arm(&mut hooks, &waiter(flow_exe, &["wait"]));
    json!({ "hooks": hooks }).to_string()
}

/// A plain session's --settings JSON: report_hooks alone; it waits for no master.
pub fn plain_settings(flow_exe: &Path) -> String {
    json!({ "hooks": report_hooks(flow_exe) }).to_string()
}

/// Each event in EVENTS runs `<flow_exe> hook claude-code`, synchronously so the events reach the
/// log in order.
fn report_hooks(flow_exe: &Path) -> serde_json::Map<String, Value> {
    let handler = |timeout: u64| {
        json!({
            "type": "command",
            "command": flow_exe.display().to_string(),
            "args": ["hook", Adapter::ClaudeCode.name()],
            "timeout": timeout,
        })
    };
    EVENTS
        .iter()
        .map(|event| {
            let timeout = if *event == "PreToolUse" {
                TOOL_HOOK_TIMEOUT_SECS
            } else {
                HOOK_TIMEOUT_SECS
            };
            let hooks = json!([handler(timeout)]);
            let entry = if event.ends_with("ToolUse") || *event == "PermissionRequest" {
                json!({ "matcher": "*", "hooks": hooks })
            } else {
                json!({ "hooks": hooks })
            };
            (event.to_string(), json!([entry]))
        })
        .collect()
}

/// The master's --settings JSON: report_hooks and `<flow_exe> wait --master` on each event in
/// WAIT_EVENTS; the waiter itself reads [master] wake.
pub fn master_settings(flow_exe: &Path) -> String {
    let mut hooks = report_hooks(flow_exe);
    arm(&mut hooks, &waiter(flow_exe, &["wait", "--master"]));
    json!({ "hooks": hooks }).to_string()
}

/// Adds `waiter` to each event in WAIT_EVENTS; settings and master_settings share it.
fn arm(hooks: &mut serde_json::Map<String, Value>, waiter: &Value) {
    for event in WAIT_EVENTS {
        let entry = hooks
            .entry(event)
            .or_insert_with(|| json!([{ "hooks": [] }]));
        entry[0]["hooks"]
            .as_array_mut()
            .expect("an entry holds a hooks list")
            .push(waiter.clone());
    }
}

/// A hook Claude Code runs in the background; its exit 2 wakes the session with its stderr.
fn waiter(flow_exe: &Path, args: &[&str]) -> Value {
    json!({
        "type": "command",
        "command": flow_exe.display().to_string(),
        "args": args,
        "asyncRewake": true,
        "timeout": WAIT_TIMEOUT_SECS,
    })
}

/// The flow events one Claude Code hook payload means. Of a subagent's events (agent_id set) only
/// a phase skill counts, as when a review runs in a fresh agent; the rest follows the main agent.
pub fn translate(payload: &Value, workflow: &Workflow) -> Vec<EventKind> {
    let tool = payload["tool_name"].as_str().unwrap_or("");
    let input = &payload["tool_input"];
    if payload.get("agent_id").is_some() {
        let starts_skill = payload["hook_event_name"] == "PreToolUse" && tool == "Skill";
        return match input["skill"].as_str() {
            Some(skill) if starts_skill => workflow
                .phase_of(skill)
                .map(|phase| vec![EventKind::Phase { phase }])
                .unwrap_or_default(),
            _ => Vec::new(),
        };
    }
    match payload["hook_event_name"].as_str().unwrap_or("") {
        "UserPromptSubmit" => {
            let mut kinds = vec![EventKind::Working];
            let skill = payload["prompt"].as_str().and_then(typed_skill);
            if let Some(phase) = skill.and_then(|skill| workflow.phase_of(skill)) {
                kinds.push(EventKind::Phase { phase });
            }
            kinds
        }
        "PreToolUse" => match tool {
            "Skill" => input["skill"]
                .as_str()
                .and_then(|skill| workflow.phase_of(skill))
                .map(|phase| vec![EventKind::Phase { phase }])
                .unwrap_or_default(),
            "Agent" if input["run_in_background"] != Value::Bool(true) => {
                let description = input["description"].as_str().unwrap_or("a subagent");
                vec![EventKind::Delegated {
                    description: cut(description),
                }]
            }
            "AskUserQuestion" => input["questions"][0]["question"]
                .as_str()
                .map(|question| {
                    vec![EventKind::Waiting {
                        question: cut(question),
                        questions: questions(input),
                    }]
                })
                .unwrap_or_default(),
            _ => Vec::new(),
        },
        "PostToolUse" => {
            let mut kinds = vec![EventKind::Working];
            match tool {
                PLAN_PROGRESS_TOOL => kinds.extend(plan_report(input, &payload["tool_response"])),
                "Bash" => {
                    let command = input["command"].as_str().unwrap_or("");
                    let stdout = payload["tool_response"]["stdout"].as_str().unwrap_or("");
                    if let Some(url) = created_merge_request(command, stdout) {
                        kinds.push(EventKind::MergeRequest { url });
                    }
                }
                _ => {}
            }
            kinds
        }
        "PermissionRequest" if tool != "AskUserQuestion" && !tool.is_empty() => {
            let cwd = payload["cwd"].as_str().unwrap_or("");
            vec![EventKind::Waiting {
                question: cut(&format!("permission: {}", tool_call(tool, input, cwd))),
                questions: Vec::new(),
            }]
        }
        "Stop" => {
            let answer = payload["last_assistant_message"].as_str().unwrap_or("");
            let mut kinds = Vec::new();
            if !answer.trim().is_empty() {
                kinds.push(EventKind::Answer {
                    text: answer.trim().to_string(),
                });
            }
            kinds.push(turn_end(answer));
            kinds
        }
        "SessionEnd" if payload["reason"] == "prompt_input_exit" => vec![EventKind::Ended],
        _ => Vec::new(),
    }
}

/// The questions of an AskUserQuestion input; a malformed one is left out.
fn questions(input: &Value) -> Vec<Question> {
    let text = |value: &Value| value.as_str().unwrap_or_default().to_string();
    input["questions"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|asked| asked["question"].is_string())
        .map(|asked| Question {
            header: text(&asked["header"]),
            question: text(&asked["question"]),
            options: asked["options"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|option| Choice {
                    label: text(&option["label"]),
                    description: text(&option["description"]),
                })
                .collect(),
            multi_select: asked["multiSelect"] == Value::Bool(true),
        })
        .collect()
}

/// A prompt or a compaction begins a turn, a Stop ends it whatever runs in the background (a
/// long-lived task never finishes, a question waits for the person), the compaction's end leaves
/// it at its prompt with no Stop; a subagent's (agent_id set) are not the session's. Claude Code
/// runs no hook on an Esc.
pub fn turn(payload: &Value) -> Option<Turn> {
    if payload.get("agent_id").is_some() {
        return None;
    }
    match payload["hook_event_name"].as_str()? {
        "UserPromptSubmit" | "PreCompact" => Some(Turn::Began),
        "Stop" => Some(Turn::Ended),
        "PostCompact" => Some(Turn::Idle),
        _ => None,
    }
}

/// tasks from "ok, N/M done", or blocked "item: note" when the item is reported blocked.
fn plan_report(input: &Value, response: &Value) -> Vec<EventKind> {
    if input["status"] == "blocked" {
        let item = match &input["item"] {
            Value::String(item) => item.clone(),
            Value::Null => String::new(),
            other => other.to_string(),
        };
        let reason = match input["note"].as_str() {
            Some(note) => format!("{item}: {note}"),
            None => item,
        };
        return vec![EventKind::Blocked {
            reason: cut(&reason),
        }];
    }
    task_counts(&text_of(response))
        .map(|(done, total)| vec![EventKind::Tasks { done, total }])
        .unwrap_or_default()
}

/// A tool response as text: a string, text blocks joined, or anything else as JSON.
fn text_of(response: &Value) -> String {
    match response {
        Value::String(text) => text.clone(),
        Value::Array(blocks) => blocks
            .iter()
            .filter_map(|block| block["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        other => other.to_string(),
    }
}

/// "ok, 4/9 done" -> (4, 9); "ok, all 9 done" -> (9, 9).
fn task_counts(result: &str) -> Option<(u32, u32)> {
    if let Some(rest) = result.split("ok, all ").nth(1)
        && let Some((count, after)) = rest.split_once(' ')
        && after.starts_with("done")
    {
        let total = count.parse().ok()?;
        return Some((total, total));
    }
    let rest = result.split("ok, ").nth(1)?;
    let (counts, after) = rest.split_once(' ')?;
    if !after.starts_with("done") {
        return None;
    }
    let (done, total) = counts.split_once('/')?;
    Some((done.parse().ok()?, total.parse().ok()?))
}

/// The skill a prompt typed as "/name args" names.
fn typed_skill(prompt: &str) -> Option<&str> {
    let first = prompt.split_whitespace().next()?;
    first.strip_prefix('/').filter(|name| !name.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::Phase;

    fn fixture(name: &str) -> Value {
        let path = format!(
            "{}/src/adapter/fixtures/claude-code/{name}.json",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    fn phases() -> Workflow {
        Workflow::of(&[
            ("plan", &["design-code"]),
            ("implement", &["implement-plan"]),
        ])
    }

    fn events(name: &str) -> Vec<EventKind> {
        translate(&fixture(name), &phases())
    }

    fn waiting(question: &str) -> EventKind {
        EventKind::Waiting {
            question: question.into(),
            questions: Vec::new(),
        }
    }

    #[test]
    fn a_prompt_means_working_and_a_typed_skill_its_phase() {
        assert_eq!(events("user_prompt"), vec![EventKind::Working]);
        assert_eq!(
            events("user_prompt_skill"),
            vec![
                EventKind::Working,
                EventKind::Phase {
                    phase: Phase::of("implement")
                }
            ]
        );
    }

    #[test]
    fn a_skill_sets_the_phase_and_a_subagent_is_delegated() {
        assert_eq!(
            events("pre_skill"),
            vec![EventKind::Phase {
                phase: Phase::of("plan")
            }]
        );
        assert_eq!(
            events("pre_agent"),
            vec![EventKind::Delegated {
                description: "Review the changes".into()
            }]
        );
        assert_eq!(events("pre_agent_background"), vec![], "it runs on its own");
        assert_eq!(events("pre_write"), vec![]);
    }

    #[test]
    fn of_a_subagents_events_only_a_phase_skill_counts() {
        assert_eq!(events("subagent_bash"), vec![]);
        let review = json!({
            "agent_id": "a939a6772fbea50e7",
            "hook_event_name": "PreToolUse",
            "tool_name": "Skill",
            "tool_input": {"skill": "review-changes"}
        });
        let phases = Workflow::of(&[("review", &["review-changes"])]);
        assert_eq!(
            translate(&review, &phases),
            vec![EventKind::Phase {
                phase: Phase::of("review")
            }]
        );
    }

    #[test]
    fn a_turn_ends_whatever_runs_in_the_background() {
        let sleeping = "The sleeper is still running.";
        assert_eq!(
            events("stop_background"),
            vec![
                EventKind::Answer {
                    text: sleeping.into()
                },
                EventKind::Idle {
                    note: sleeping.into()
                }
            ]
        );
        let mut asking = fixture("stop_background");
        let question = "Which color do you prefer, red or blue?";
        asking["last_assistant_message"] = json!(question);
        assert_eq!(
            translate(&asking, &phases()),
            vec![
                EventKind::Answer {
                    text: question.into()
                },
                waiting(question)
            ]
        );
        let link = "https://claude.ai/artifact/CUmbqod2Bv8mGRtioXWRH7";
        assert_eq!(
            events("stop_artifact_watch"),
            vec![
                EventKind::Answer { text: link.into() },
                EventKind::Idle { note: link.into() }
            ]
        );
    }

    fn question(header: &str, text: &str, options: &[(&str, &str)], multi: bool) -> Question {
        Question {
            header: header.into(),
            question: text.into(),
            options: options
                .iter()
                .map(|(label, description)| Choice {
                    label: (*label).into(),
                    description: (*description).into(),
                })
                .collect(),
            multi_select: multi,
        }
    }

    #[test]
    fn a_question_or_a_permission_waits_for_you() {
        assert_eq!(
            events("pre_ask"),
            vec![EventKind::Waiting {
                question: "Red or blue?".into(),
                questions: vec![question(
                    "Color",
                    "Red or blue?",
                    &[("Red", "Choose red"), ("Blue", "Choose blue")],
                    false
                )],
            }]
        );
        assert_eq!(
            events("permission_ask"),
            vec![],
            "the question already says it"
        );
        assert_eq!(
            events("permission_bash"),
            vec![waiting("permission: Bash(rm -rf build)")]
        );
    }

    #[test]
    fn a_dialog_keeps_every_question_with_its_options() {
        let [
            EventKind::Waiting {
                question: first,
                questions,
            },
        ] = &events("pre_ask_two")[..]
        else {
            panic!("one waiting event");
        };

        assert_eq!(first, "Which color?");
        assert_eq!(
            questions,
            &vec![
                question(
                    "Color",
                    "Which color?",
                    &[
                        ("Red", "A warm, bold color"),
                        ("Green", "A calm, natural color"),
                        ("Blue", "A cool, steady color"),
                    ],
                    false
                ),
                question(
                    "Toppings",
                    "Which toppings?",
                    &[
                        ("Cheese", "Melted cheese"),
                        ("Ham", "Sliced ham"),
                        ("Olives", "Black olives"),
                    ],
                    true
                ),
            ]
        );
    }

    #[test]
    fn leaves_out_a_question_without_text_and_reads_missing_fields_as_empty() {
        let mut payload = fixture("pre_ask_two");
        payload["tool_input"]["questions"][1] = json!({"header": "Broken"});
        payload["tool_input"]["questions"][0]["options"][0] = json!({"label": "Red"});
        payload["tool_input"]["questions"][0]
            .as_object_mut()
            .unwrap()
            .remove("multiSelect");

        let [EventKind::Waiting { questions, .. }] = &translate(&payload, &phases())[..] else {
            panic!("one waiting event");
        };

        assert_eq!(questions.len(), 1);
        assert_eq!(questions[0].options[0].description, "");
        assert!(!questions[0].multi_select);
    }

    #[test]
    fn names_a_file_inside_the_session_folder_by_its_relative_path() {
        let mut payload = fixture("permission_bash");
        payload["tool_name"] = json!("Write");
        payload["tool_input"] = json!({"file_path": "/repo/src/export.rs", "content": "x"});

        assert_eq!(
            translate(&payload, &phases()),
            vec![waiting("permission: Write(src/export.rs)")]
        );
        payload["tool_input"] = json!({"file_path": "/etc/hosts"});
        assert_eq!(
            translate(&payload, &phases()),
            vec![waiting("permission: Write(/etc/hosts)")]
        );
    }

    #[test]
    fn after_a_tool_it_works_on_and_reports_tasks_blocks_and_merge_requests() {
        assert_eq!(events("post_write"), vec![EventKind::Working]);
        assert_eq!(events("post_bash"), vec![EventKind::Working]);
        assert_eq!(
            events("post_plan_progress"),
            vec![EventKind::Working, EventKind::Tasks { done: 4, total: 9 }]
        );
        assert_eq!(
            events("post_plan_progress_blocked"),
            vec![
                EventKind::Working,
                EventKind::Blocked {
                    reason: "Task 5: the migration needs a default".into()
                }
            ]
        );
        assert_eq!(
            events("post_bash_merge_request"),
            vec![
                EventKind::Working,
                EventKind::MergeRequest {
                    url: "https://gitlab.com/shop/app/-/merge_requests/418".into()
                }
            ]
        );
    }

    #[test]
    fn the_end_of_a_turn_waits_with_its_question_and_only_exit_ends_the_flow() {
        let [
            EventKind::Answer { text },
            EventKind::Waiting { question, .. },
        ] = &events("stop")[..]
        else {
            panic!("expected the answer, then waiting");
        };
        assert!(text.starts_with("I ran `rm -f`"), "{text}");
        assert!(
            text.ends_with("Let me know if you still want it."),
            "{text}"
        );
        assert_eq!(question, "Let me know if you still want it.");
        assert_eq!(
            events("stop_woken").last(),
            Some(&waiting("Shall I also quote every field?")),
            "a turn saaio woke ends like any other"
        );
        assert_eq!(events("session_end_exit"), vec![EventKind::Ended]);
        assert_eq!(events("session_end_hangup"), vec![], "a hangup resumes");
    }

    #[test]
    fn a_prompt_or_compaction_begins_a_turn_and_a_stop_or_its_end_leaves_it_idle() {
        assert_eq!(turn(&fixture("user_prompt")), Some(Turn::Began));
        assert_eq!(turn(&fixture("pre_compact")), Some(Turn::Began));
        assert_eq!(turn(&fixture("post_compact")), Some(Turn::Idle));
        assert_eq!(turn(&fixture("stop")), Some(Turn::Ended));
        assert_eq!(turn(&fixture("stop_artifact_watch")), Some(Turn::Ended));
        assert_eq!(turn(&fixture("stop_background")), Some(Turn::Ended));
        assert_eq!(turn(&fixture("subagent_bash")), None);
        assert_eq!(turn(&fixture("pre_write")), None);
        assert_eq!(events("pre_compact"), vec![]);
        assert_eq!(events("post_compact"), vec![]);
    }

    #[test]
    fn ignores_events_and_shapes_it_does_not_know() {
        let phases = phases();
        assert_eq!(translate(&json!({}), &phases), vec![]);
        assert_eq!(
            translate(&json!({"hook_event_name": "Notification"}), &phases),
            vec![]
        );
        assert_eq!(
            translate(
                &json!({"hook_event_name": "PreToolUse", "tool_name": "AskUserQuestion"}),
                &phases
            ),
            vec![]
        );
    }

    #[test]
    fn reads_task_counts_and_typed_skills() {
        assert_eq!(task_counts("ok, 4/9 done - next: 2.4"), Some((4, 9)));
        assert_eq!(task_counts("ok, all 9 done - plan complete"), Some((9, 9)));
        assert_eq!(task_counts("ok, all 09 done"), Some((9, 9)));
        assert_eq!(task_counts("refused: 2.4 matches two items"), None);
        assert_eq!(typed_skill(" /design-code add csv"), Some("design-code"));
        assert_eq!(typed_skill("please design"), None);
        assert_eq!(typed_skill("/"), None);
    }

    #[test]
    fn wires_every_event_to_flow_hook_claude_code() {
        let settings: Value = serde_json::from_str(&settings(Path::new("/bin/saaio"))).unwrap();

        for event in EVENTS {
            let handler = &settings["hooks"][event][0]["hooks"][0];
            assert_eq!(handler["command"], "/bin/saaio", "{event}");
            assert_eq!(handler["args"], json!(["hook", "claude-code"]), "{event}");
        }
        assert_eq!(settings["hooks"]["PreToolUse"][0]["matcher"], "*");
        assert_eq!(settings["hooks"]["Stop"][0].get("matcher"), None);
        let timeout = |event: &str| settings["hooks"][event][0]["hooks"][0]["timeout"].clone();
        assert_eq!(timeout("PreToolUse"), TOOL_HOOK_TIMEOUT_SECS);
        assert_eq!(timeout("PostToolUse"), HOOK_TIMEOUT_SECS);
    }

    #[test]
    fn a_plain_session_reports_but_neither_waits_nor_hears_of_the_master() {
        let plain: Value = serde_json::from_str(&plain_settings(Path::new("/bin/saaio"))).unwrap();

        let hooks = plain["hooks"].as_object().unwrap();
        assert_eq!(hooks.len(), EVENTS.len(), "no SessionStart waiter");
        for event in EVENTS {
            let handlers = hooks[event][0]["hooks"].as_array().unwrap();
            assert_eq!(handlers.len(), 1, "{event}");
            assert_eq!(
                handlers[0]["args"],
                json!(["hook", "claude-code"]),
                "{event}"
            );
        }
        assert_eq!(
            Adapter::ClaudeCode.plain_wiring(Path::new("/bin/saaio"), Path::new("/worktree")),
            [
                "--settings".to_string(),
                plain_settings(Path::new("/bin/saaio"))
            ]
        );
    }

    #[test]
    fn every_session_waits_for_news_at_its_start_and_each_turn_end() {
        let waiter = |args: Value| {
            json!({
                "type": "command",
                "command": "/bin/saaio",
                "args": args,
                "asyncRewake": true,
                "timeout": WAIT_TIMEOUT_SECS,
            })
        };
        let worker: Value = serde_json::from_str(&settings(Path::new("/bin/saaio"))).unwrap();
        let master: Value =
            serde_json::from_str(&master_settings(Path::new("/bin/saaio"))).unwrap();

        for event in ["SessionStart", "Stop"] {
            let hooks = worker["hooks"][event][0]["hooks"].as_array().unwrap();
            assert_eq!(hooks.last().unwrap(), &waiter(json!(["wait"])), "{event}");
            let hooks = master["hooks"][event][0]["hooks"].as_array().unwrap();
            assert_eq!(
                hooks.last().unwrap(),
                &waiter(json!(["wait", "--master"])),
                "{event}"
            );
        }
        assert_eq!(
            worker["hooks"]["SessionStart"][0]["hooks"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn the_master_reports_every_event_and_always_waits() {
        let master: Value =
            serde_json::from_str(&master_settings(Path::new("/bin/saaio"))).unwrap();

        for event in EVENTS {
            let handler = &master["hooks"][event][0]["hooks"][0];
            assert_eq!(handler["args"], json!(["hook", "claude-code"]), "{event}");
        }
        let waiters = master["hooks"]
            .as_object()
            .unwrap()
            .values()
            .flat_map(|entries| entries[0]["hooks"].as_array().unwrap())
            .filter(|handler| handler["args"] == json!(["wait", "--master"]))
            .count();
        assert_eq!(waiters, WAIT_EVENTS.len());
    }
}
