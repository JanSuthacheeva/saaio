use std::path::Path;

use serde_json::{Value, json};

use crate::adapter::{Adapter, cut, phase_of};
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
/// Seconds Claude Code lets an asyncRewake waiter sleep before it ends it: a week.
const WAIT_TIMEOUT_SECS: u64 = 604_800;
/// The events that arm a session's waiter: its start (the master starts without a prompt) and
/// every turn end.
const WAIT_EVENTS: [&str; 2] = ["SessionStart", "Stop"];
/// What a worker is told about the master, appended to its system prompt: the master's messages
/// arrive as hook feedback, which it would rightly not take as the user's words otherwise.
pub const WORKER_PROMPT: &str = "You run as a worker of flow, in your own git worktree. The flow \
master is the Claude session that coordinates the workers for the user. Its messages reach you as \
hook feedback that starts with \"message from the flow master:\"; they answer what you asked or \
pass on what the user said, so act on them as you would on the user's own answer. To ask the \
master or the user something, end your turn with the question.";
/// plan-progress's report tool, whose answers carry the plan's task counts.
const PLAN_PROGRESS_TOOL: &str = "mcp__plan-progress__report";
/// How a Stop's background tasks list the watch Claude Code keeps on a published artifact.
const ARTIFACT_WATCH_TYPE: &str = "monitor";
const ARTIFACT_WATCH_PREFIX: &str = "live updates for artifact ";

/// The --settings JSON: each event in EVENTS runs `<flow_exe> hook claude-code`, synchronously so
/// the events reach the log in order; each in WAIT_EVENTS also arms `<flow_exe> wait`.
pub fn settings(flow_exe: &Path) -> String {
    let handler = json!({
        "type": "command",
        "command": flow_exe.display().to_string(),
        "args": ["hook", Adapter::ClaudeCode.name()],
        "timeout": HOOK_TIMEOUT_SECS,
    });
    let mut hooks: serde_json::Map<String, Value> = EVENTS
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
    let waiter = waiter(flow_exe, &["wait"]);
    for event in WAIT_EVENTS {
        let entry = hooks
            .entry(event)
            .or_insert_with(|| json!([{ "hooks": [] }]));
        entry[0]["hooks"]
            .as_array_mut()
            .expect("an entry holds a hooks list")
            .push(waiter.clone());
    }
    json!({ "hooks": hooks }).to_string()
}

/// The master's --settings JSON: only its waiter, `<flow_exe> wait --master`; the master is no
/// flow, so it reports nothing.
pub fn master_settings(flow_exe: &Path) -> String {
    let waiter = waiter(flow_exe, &["wait", "--master"]);
    let hooks: serde_json::Map<String, Value> = WAIT_EVENTS
        .iter()
        .map(|event| (event.to_string(), json!([{ "hooks": [waiter] }])))
        .collect();
    json!({ "hooks": hooks }).to_string()
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
pub fn translate(payload: &Value, phases: &PhaseMap) -> Vec<EventKind> {
    let tool = payload["tool_name"].as_str().unwrap_or("");
    let input = &payload["tool_input"];
    if payload.get("agent_id").is_some() {
        let starts_skill = payload["hook_event_name"] == "PreToolUse" && tool == "Skill";
        return match input["skill"].as_str() {
            Some(skill) if starts_skill => phase_of(skill, phases)
                .map(|phase| vec![EventKind::Phase { phase }])
                .unwrap_or_default(),
            _ => Vec::new(),
        };
    }
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
            let cwd = payload["cwd"].as_str().unwrap_or("");
            vec![EventKind::Waiting {
                question: cut(&format!("permission: {}", tool_call(tool, input, cwd))),
            }]
        }
        "Stop" => {
            if let Some(task) = running_background_task(&payload["background_tasks"]) {
                return vec![EventKind::Delegated {
                    description: cut(task),
                }];
            }
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

/// What a turn ending with background work in flight waits for: the first running or pending
/// task's description, artifact watches aside.
fn running_background_task(tasks: &Value) -> Option<&str> {
    tasks.as_array()?.iter().find_map(|task| {
        let busy = matches!(task["status"].as_str(), Some("running" | "pending"));
        (busy && !artifact_watch(task))
            .then(|| task["description"].as_str().unwrap_or("background work"))
    })
}

/// A live subscription Claude Code keeps for a published artifact: it never finishes.
fn artifact_watch(task: &Value) -> bool {
    task["type"] == ARTIFACT_WATCH_TYPE
        && task["description"]
            .as_str()
            .is_some_and(|description| description.starts_with(ARTIFACT_WATCH_PREFIX))
}

/// Waiting with the last sentence of the answer's last non-empty line that asks (ends in "?", or
/// starts "Let me know" or "Tell me"); else idle, with its last sentence: only a question needs
/// the person.
fn turn_end(answer: &str) -> EventKind {
    let Some(last) = answer.lines().map(str::trim).rfind(|line| !line.is_empty()) else {
        return EventKind::Idle {
            note: "turn ended".to_string(),
        };
    };
    let plain = plain(last);
    let sentences = sentences(&plain);
    let asks = |sentence: &&String| {
        let lower = sentence.to_lowercase();
        sentence.ends_with('?') || lower.starts_with("let me know") || lower.starts_with("tell me")
    };
    match sentences.iter().rev().find(asks) {
        Some(question) => EventKind::Waiting {
            question: cut(question),
        },
        None => EventKind::Idle {
            note: cut(sentences.last().map_or(last, |sentence| sentence.as_str())),
        },
    }
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

/// Name(target) for a permission question: Bash(command), Write(file_path), ...; a path inside the
/// session's folder is shown relative to it.
fn tool_call(name: &str, input: &Value, cwd: &str) -> String {
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
    let inside = format!("{}/", cwd.trim_end_matches('/'));
    let target = match target.strip_prefix(&inside) {
        Some(relative) if !cwd.is_empty() => relative,
        _ => target,
    };
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
    fn of_a_subagents_events_only_a_phase_skill_counts() {
        assert_eq!(events("subagent_bash"), vec![]);
        let review = json!({
            "agent_id": "a939a6772fbea50e7",
            "hook_event_name": "PreToolUse",
            "tool_name": "Skill",
            "tool_input": {"skill": "review-changes"}
        });
        let phases = PhaseMap::from([("review-changes".to_string(), Phase::Review)]);
        assert_eq!(
            translate(&review, &phases),
            vec![EventKind::Phase {
                phase: Phase::Review
            }]
        );
    }

    #[test]
    fn a_turn_that_ends_with_background_work_running_is_delegated() {
        assert_eq!(
            events("stop_background"),
            vec![EventKind::Delegated {
                description: "background sleeper".into()
            }]
        );
    }

    #[test]
    fn an_artifact_watch_is_not_work_the_turn_waits_on() {
        let link = "https://claude.ai/artifact/CUmbqod2Bv8mGRtioXWRH7";
        assert_eq!(
            events("stop_artifact_watch"),
            vec![
                EventKind::Answer { text: link.into() },
                EventKind::Idle { note: link.into() }
            ]
        );
    }

    #[test]
    fn a_pending_artifact_watch_is_not_work_either() {
        let mut stop = fixture("stop_artifact_watch");
        stop["background_tasks"][0]["status"] = json!("pending");
        assert!(matches!(
            translate(&stop, &phases()).last(),
            Some(EventKind::Idle { .. })
        ));
    }

    #[test]
    fn a_subagent_running_beside_an_artifact_watch_is_delegated() {
        let mut stop = fixture("stop_artifact_watch");
        stop["background_tasks"]
            .as_array_mut()
            .unwrap()
            .push(json!({
                "id": "adfb26396b14a6b82",
                "type": "subagent",
                "status": "running",
                "description": "background sleeper",
                "agent_type": "general-purpose"
            }));
        assert_eq!(
            translate(&stop, &phases()),
            vec![EventKind::Delegated {
                description: "background sleeper".into()
            }]
        );
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
        let [EventKind::Answer { text }, EventKind::Waiting { question }] = &events("stop")[..]
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
            "a turn flow woke ends like any other"
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
    fn waits_for_the_person_only_when_the_last_line_asks() {
        let waiting = |question: &str| EventKind::Waiting {
            question: question.into(),
        };
        let idle = |note: &str| EventKind::Idle { note: note.into() };

        assert_eq!(
            turn_end("Done.\n\n- **Which** approach: `A` or B? I lean A."),
            waiting("Which approach: A or B?")
        );
        assert_eq!(turn_end("1. Shall I go on?"), waiting("Shall I go on?"));
        assert_eq!(
            turn_end("Tests pass. Tell me which one to keep."),
            waiting("Tell me which one to keep.")
        );
        assert_eq!(turn_end("All set. Tests pass."), idle("Tests pass."));
        assert_eq!(
            turn_end("Started the review.\n\nThe review is still running."),
            idle("The review is still running.")
        );
        assert_eq!(turn_end("  \n"), idle("turn ended"));
    }

    #[test]
    fn reads_task_counts_merge_requests_and_typed_skills() {
        assert_eq!(task_counts("ok, 4/9 done - next: 2.4"), Some((4, 9)));
        assert_eq!(task_counts("ok, all 9 done - plan complete"), Some((9, 9)));
        assert_eq!(task_counts("ok, all 09 done"), Some((9, 9)));
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

    #[test]
    fn tells_a_worker_who_the_flow_master_is() {
        assert!(WORKER_PROMPT.contains("message from the flow master:"));
        let wiring = Adapter::ClaudeCode.wiring(Path::new("/bin/flow"));
        assert_eq!(wiring[0], "--settings");
        assert_eq!(wiring[2..], ["--append-system-prompt", WORKER_PROMPT]);
    }

    #[test]
    fn every_session_waits_for_news_at_its_start_and_each_turn_end() {
        let waiter = |args: Value| {
            json!({
                "type": "command",
                "command": "/bin/flow",
                "args": args,
                "asyncRewake": true,
                "timeout": WAIT_TIMEOUT_SECS,
            })
        };
        let worker: Value = serde_json::from_str(&settings(Path::new("/bin/flow"))).unwrap();
        let master: Value = serde_json::from_str(&master_settings(Path::new("/bin/flow"))).unwrap();

        for event in ["SessionStart", "Stop"] {
            let hooks = worker["hooks"][event][0]["hooks"].as_array().unwrap();
            assert_eq!(hooks.last().unwrap(), &waiter(json!(["wait"])), "{event}");
            assert_eq!(
                master["hooks"][event],
                json!([{ "hooks": [waiter(json!(["wait", "--master"]))] }]),
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
        assert_eq!(
            master["hooks"].as_object().unwrap().len(),
            2,
            "the master reports nothing"
        );
    }
}
