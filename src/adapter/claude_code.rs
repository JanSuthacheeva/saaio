//! Claude Code's `--settings` hooks and its hook events -> flow events and turns (pure)

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
mod tests;
