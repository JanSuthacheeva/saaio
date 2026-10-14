use std::path::Path;

use serde_json::{Value, json};

use crate::adapter::{cut, phase_of};
use crate::config::PhaseMap;
use crate::event::EventKind;

/// The hook events flow listens to.
const EVENTS: [&str; 6] = [
    "UserPromptSubmit",
    "PreToolUse",
    "PostToolUse",
    "PermissionRequest",
    "Stop",
    "SessionEnd",
];
/// Seconds Claude Code waits for `flow hook` before it goes on without it.
const HOOK_TIMEOUT_SECS: u64 = 10;
/// plan-progress's report tool, whose answers carry the plan's task counts.
const PLAN_PROGRESS_TOOL: &str = "mcp__plan-progress__report";

/// The --settings JSON: each event in EVENTS runs `<flow_exe> hook claude-code`, synchronously so
/// the events reach the log in order.
pub fn settings(flow_exe: &Path) -> String {
    let handler = json!({
        "type": "command",
        "command": flow_exe.display().to_string(),
        "args": ["hook", "claude-code"],
        "timeout": HOOK_TIMEOUT_SECS,
    });
    let hooks: serde_json::Map<String, Value> = EVENTS
        .iter()
        .map(|event| {
            let entry = if event.ends_with("ToolUse") || *event == "PermissionRequest" {
                json!({ "matcher": "*", "hooks": [handler] })
            } else {
                json!({ "hooks": [handler] })
            };
            (event.to_string(), json!([entry]))
        })
        .collect();
    json!({ "hooks": hooks }).to_string()
}

/// The flow events one Claude Code hook payload means. A subagent's events (agent_id set) mean
/// nothing; the flow follows the main agent only.
pub fn translate(payload: &Value, phases: &PhaseMap) -> Vec<EventKind> {
    if payload.get("agent_id").is_some() {
        return Vec::new();
    }
    let tool = payload["tool_name"].as_str().unwrap_or("");
    let input = &payload["tool_input"];
    match payload["hook_event_name"].as_str().unwrap_or("") {
        "UserPromptSubmit" => {
            let mut kinds = vec![EventKind::Working];
            let skill = payload["prompt"].as_str().and_then(typed_skill);
            if let Some(phase) = skill.and_then(|skill| phase_of(skill, phases)) {
                kinds.push(EventKind::Phase { phase });
            }
            kinds
        }
        "PreToolUse" => match tool {
            "Skill" => input["skill"]
                .as_str()
                .and_then(|skill| phase_of(skill, phases))
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
            vec![EventKind::Waiting {
                question: cut(&format!("permission: {}", tool_call(tool, input))),
            }]
        }
        "Stop" if payload["stop_hook_active"] != Value::Bool(true) => {
            let answer = payload["last_assistant_message"].as_str().unwrap_or("");
            vec![EventKind::Waiting {
                question: waiting_question(answer).unwrap_or_else(|| "turn ended".to_string()),
            }]
        }
        "SessionEnd" if payload["reason"] == "prompt_input_exit" => vec![EventKind::Ended],
        _ => Vec::new(),
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

/// The last sentence ending in "?" of the answer's last non-empty line, else its last sentence.
fn waiting_question(answer: &str) -> Option<String> {
    let last = answer
        .lines()
        .map(str::trim)
        .rfind(|line| !line.is_empty())?;
    let plain = plain(last);
    let sentences = sentences(&plain);
    let question = sentences
        .iter()
        .rev()
        .find(|sentence| sentence.ends_with('?'))
        .or_else(|| sentences.last())
        .map(|sentence| sentence.to_string())
        .unwrap_or_else(|| last.to_string());
    Some(cut(&question))
}

/// A Markdown line as plain text: no list marker, no emphasis or code ticks.
fn plain(line: &str) -> String {
    let mut text = line.trim();
    if let Some(rest) = text
        .strip_prefix("- ")
        .or_else(|| text.strip_prefix("* "))
        .or_else(|| text.strip_prefix("+ "))
    {
        text = rest;
    } else if let Some((number, rest)) = text.split_once(". ")
        && !number.is_empty()
        && number.chars().all(|c| c.is_ascii_digit())
    {
        text = rest;
    }
    text.chars()
        .filter(|c| !matches!(c, '*' | '_' | '`'))
        .collect::<String>()
        .trim()
        .to_string()
}

/// Sentences split after ".", "!" or "?" followed by whitespace.
fn sentences(text: &str) -> Vec<String> {
    let mut sentences = Vec::new();
    let mut current = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        current.push(c);
        if matches!(c, '.' | '!' | '?') && chars.peek().is_some_and(|next| next.is_whitespace()) {
            sentences.push(current.trim().to_string());
            current.clear();
        }
    }
    if !current.trim().is_empty() {
        sentences.push(current.trim().to_string());
    }
    sentences
}

/// "ok, 4/9 done" -> (4, 9); "ok, all 9 done" -> (9, 9).
fn task_counts(result: &str) -> Option<(u32, u32)> {
    if let Some(rest) = result.split("ok, all ").nth(1) {
        let total: u32 = rest.split(' ').next()?.parse().ok()?;
        if rest[total.to_string().len()..].starts_with(" done") {
            return Some((total, total));
        }
    }
    let rest = result.split("ok, ").nth(1)?;
    let (counts, after) = rest.split_once(' ')?;
    if !after.starts_with("done") {
        return None;
    }
    let (done, total) = counts.split_once('/')?;
    Some((done.parse().ok()?, total.parse().ok()?))
}

/// The URL `gh pr create` or `glab mr create` printed: the first http(s) URL in its stdout.
fn created_merge_request(command: &str, stdout: &str) -> Option<String> {
    let words: Vec<&str> = command.split_whitespace().collect();
    let creates = words.windows(3).any(|window| {
        let tool = (window[0], window[1]);
        let opens = tool == ("gh", "pr") || tool == ("glab", "mr");
        opens && (window[2] == "create" || window[2] == "new")
    });
    if !creates {
        return None;
    }
    stdout
        .split_whitespace()
        .find(|word| word.starts_with("https://") || word.starts_with("http://"))
        .map(String::from)
}

/// The skill a prompt typed as "/name args" names.
fn typed_skill(prompt: &str) -> Option<&str> {
    let first = prompt.split_whitespace().next()?;
    first.strip_prefix('/').filter(|name| !name.is_empty())
}

/// Name(target) for a permission question: Bash(command), Write(file_path), ...
fn tool_call(name: &str, input: &Value) -> String {
    let target = [
        "command",
        "file_path",
        "path",
        "url",
        "pattern",
        "description",
    ]
    .iter()
    .find_map(|key| input[*key].as_str())
    .and_then(|value| value.lines().next())
    .unwrap_or("");
    if target.is_empty() {
        name.to_string()
    } else {
        format!("{name}({target})")
    }
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

    fn phases() -> PhaseMap {
        PhaseMap::from([
            ("design-code".to_string(), Phase::Design),
            ("implement-plan".to_string(), Phase::Implement),
        ])
    }

    fn events(name: &str) -> Vec<EventKind> {
        translate(&fixture(name), &phases())
    }

    fn waiting(question: &str) -> EventKind {
        EventKind::Waiting {
            question: question.into(),
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
                    phase: Phase::Implement
                }
            ]
        );
    }

    #[test]
    fn a_skill_sets_the_phase_and_a_subagent_is_delegated() {
        assert_eq!(
            events("pre_skill"),
            vec![EventKind::Phase {
                phase: Phase::Design
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
    fn a_subagents_own_events_mean_nothing() {
        assert_eq!(events("subagent_bash"), vec![]);
    }

    #[test]
    fn a_question_or_a_permission_waits_for_you() {
        assert_eq!(events("pre_ask"), vec![waiting("Red or blue?")]);
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
        let [EventKind::Waiting { question }] = &events("stop")[..] else {
            panic!("expected waiting");
        };
        assert!(
            question.starts_with("Let me know if you still want it"),
            "{question}"
        );
        assert_eq!(events("session_end_exit"), vec![EventKind::Ended]);
        assert_eq!(events("session_end_hangup"), vec![], "a hangup resumes");
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
    fn takes_the_question_from_the_last_line_of_an_answer() {
        assert_eq!(
            waiting_question("Done.\n\n- **Which** approach: `A` or B? I lean A."),
            Some("Which approach: A or B?".into())
        );
        assert_eq!(
            waiting_question("All set. Tests pass."),
            Some("Tests pass.".into())
        );
        assert_eq!(
            waiting_question("1. Shall I go on?"),
            Some("Shall I go on?".into())
        );
        assert_eq!(waiting_question("  \n"), None);
    }

    #[test]
    fn reads_task_counts_merge_requests_and_typed_skills() {
        assert_eq!(task_counts("ok, 4/9 done - next: 2.4"), Some((4, 9)));
        assert_eq!(task_counts("ok, all 9 done - plan complete"), Some((9, 9)));
        assert_eq!(task_counts("refused: 2.4 matches two items"), None);
        assert_eq!(
            created_merge_request("gh pr create --fill", "https://github.com/a/b/pull/12\n"),
            Some("https://github.com/a/b/pull/12".into())
        );
        assert_eq!(created_merge_request("git push", "https://x.y/z"), None);
        assert_eq!(
            created_merge_request("glab mr create", "error: no permission"),
            None
        );
        assert_eq!(typed_skill(" /design-code add csv"), Some("design-code"));
        assert_eq!(typed_skill("please design"), None);
        assert_eq!(typed_skill("/"), None);
    }

    #[test]
    fn wires_every_event_to_flow_hook_claude_code() {
        let settings: Value = serde_json::from_str(&settings(Path::new("/bin/flow"))).unwrap();

        for event in EVENTS {
            let handler = &settings["hooks"][event][0]["hooks"][0];
            assert_eq!(handler["command"], "/bin/flow", "{event}");
            assert_eq!(handler["args"], json!(["hook", "claude-code"]), "{event}");
        }
        assert_eq!(settings["hooks"]["PreToolUse"][0]["matcher"], "*");
        assert_eq!(settings["hooks"]["Stop"][0].get("matcher"), None);
    }
}
