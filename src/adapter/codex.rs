//! Codex's `-c hooks.*` arguments and folder trust, its hook events -> flow events (pure)

use std::collections::BTreeMap;
use std::fmt::Write;
use std::path::Path;

use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::adapter::answer::turn_end;
use crate::adapter::calls::{created_merge_request, simple_commands, tool_call};
use crate::adapter::{Adapter, Approver, LineMatch, Turn, cut};
use crate::event::{EventKind, is_uuid};
use crate::workflow::Phase;
use crate::workflow::Workflow;

/// One hook saaio listens to: Codex's event name, the key its trust state uses, its timeout.
#[derive(Debug)]
struct Hook {
    event: &'static str,
    key: &'static str,
    timeout: u64,
}

/// The hook events saaio listens to; PreToolUse at 150 s, so a tool call can wait for paused
/// services, Interrupt at 3 s, the most Codex allows.
const HOOKS: [Hook; 8] = [
    Hook {
        event: "UserPromptSubmit",
        key: "user_prompt_submit",
        timeout: HOOK_TIMEOUT_SECS,
    },
    Hook {
        event: "PreToolUse",
        key: "pre_tool_use",
        timeout: TOOL_HOOK_TIMEOUT_SECS,
    },
    Hook {
        event: "PostToolUse",
        key: "post_tool_use",
        timeout: HOOK_TIMEOUT_SECS,
    },
    Hook {
        event: "PermissionRequest",
        key: "permission_request",
        timeout: HOOK_TIMEOUT_SECS,
    },
    Hook {
        event: "Stop",
        key: "stop",
        timeout: HOOK_TIMEOUT_SECS,
    },
    Hook {
        event: "Interrupt",
        key: "interrupt",
        timeout: INTERRUPT_TIMEOUT_SECS,
    },
    Hook {
        event: "PreCompact",
        key: "pre_compact",
        timeout: HOOK_TIMEOUT_SECS,
    },
    Hook {
        event: "PostCompact",
        key: "post_compact",
        timeout: HOOK_TIMEOUT_SECS,
    },
];
/// Seconds Codex waits for `saaio hook` before it goes on without it.
const HOOK_TIMEOUT_SECS: u64 = 10;
/// Seconds Codex waits for `saaio hook` before a tool call: long enough for paused services to
/// come back (hook::RESUME_WAIT).
const TOOL_HOOK_TIMEOUT_SECS: u64 = 150;
/// The most seconds Codex permits for an Interrupt hook.
const INTERRUPT_TIMEOUT_SECS: u64 = 3;
/// Where Codex says hooks passed with `-c` come from, in their trust keys.
const SESSION_FLAGS: &str = "/<session-flags>/config.toml";

/// The hash Codex keeps in hooks.state for one command hook: sha256 of its identity as compact JSON
/// with sorted keys.
fn trust_hash(hook: &Hook, command: &str) -> String {
    // `serde_toon_format` enables serde_json's `preserve_order`, so use BTreeMap explicitly.
    let command_hook = BTreeMap::from([
        ("async", Value::Bool(false)),
        ("command", Value::String(command.to_string())),
        ("timeout", Value::from(hook.timeout)),
        ("type", Value::String("command".to_string())),
    ]);
    let json = BTreeMap::from([
        ("event_name", Value::String(hook.key.to_string())),
        (
            "hooks",
            Value::Array(vec![
                serde_json::to_value(command_hook).expect("a map with string keys serialises"),
            ]),
        ),
    ]);
    let hash =
        Sha256::digest(serde_json::to_string(&json).expect("a map with string keys serialises"));
    format!("sha256:{hash:x}")
}

/// The `-c` arguments that run `'<flow_exe>' hook codex` on each event in HOOKS, synchronously so
/// the events reach the log in order and a Stop can hand news over; and keep Codex from asking
/// about updates in a pane saaio opened. The same for workers, plain sessions and the master (its
/// pane's SAAIO_MASTER tells them apart): Codex keys hook trust by position, so different hooks
/// would each undo the other's trust.
pub fn hooks(flow_exe: &Path) -> Vec<String> {
    let command = format!(
        "{} hook {}",
        shell_quoted(&flow_exe.display().to_string()),
        Adapter::Codex.name()
    );
    let mut args = vec![
        "-c".to_string(),
        "check_for_update_on_startup=false".to_string(),
    ];
    for hook in HOOKS {
        let handler = format!(
            "[{{hooks=[{{type=\"command\",command={},timeout={}}}]}}]",
            toml_string(&command),
            hook.timeout
        );
        args.push("-c".to_string());
        args.push(format!("hooks.{}={handler}", hook.event));
    }
    let state = HOOKS
        .iter()
        .map(|hook| {
            let key = format!("{SESSION_FLAGS}:{}:0:0", hook.key);
            let hash = trust_hash(hook, &command);
            format!(
                "{}={{trusted_hash={}}}",
                toml_string(&key),
                toml_string(&hash)
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    args.push("-c".to_string());
    args.push(format!("hooks.state={{{state}}}"));
    args
}

/// The `-c` arguments that make Codex trust one folder without changing its config file.
pub fn trusted_folder(folder: &Path) -> Vec<String> {
    let folder = folder.display().to_string();
    vec![
        "-c".to_string(),
        format!(
            "projects={{{}={{trust_level=\"trusted\"}}}}",
            toml_string(&folder)
        ),
    ]
}

/// The flow events one Codex hook payload means: first the session it runs in, then what it did.
/// Subagent hooks do not describe the main worker's state or phase; a permission request the
/// reviewer answers waits for no one.
pub fn translate(payload: &Value, workflow: &Workflow, approver: Approver) -> Vec<EventKind> {
    let tool = payload["tool_name"].as_str().unwrap_or("");
    let input = &payload["tool_input"];
    let event = payload["hook_event_name"].as_str().unwrap_or("");
    if payload.get("agent_id").is_some() {
        return Vec::new();
    }
    let mut kinds: Vec<EventKind> = session(payload).into_iter().collect();
    match event {
        "UserPromptSubmit" => {
            kinds.push(EventKind::Working);
            let skill = payload["prompt"].as_str().and_then(mentioned_skill);
            if let Some(phase) = skill.and_then(|skill| workflow.phase_of(skill)) {
                kinds.push(EventKind::Phase { phase });
            }
        }
        "PreToolUse" if tool == "Bash" => {
            let command = input["command"].as_str().unwrap_or("");
            if let Some(phase) = read_skill_phase(command, workflow) {
                kinds.push(EventKind::Phase { phase });
            }
        }
        "PreToolUse" if tool.ends_with(SPAWN_AGENT) => kinds.push(EventKind::Delegated {
            description: cut(&subagent(input)),
        }),
        "PostToolUse" if tool.ends_with(SPAWN_AGENT) => {}
        "PostToolUse" => {
            kinds.push(EventKind::Working);
            let command = input["command"].as_str().unwrap_or("");
            let output = payload["tool_response"].as_str().unwrap_or("");
            if tool == "Bash"
                && let Some(url) = created_merge_request(command, output)
            {
                kinds.push(EventKind::MergeRequest { url });
            }
        }
        "PermissionRequest" if !tool.is_empty() && approver == Approver::User => {
            let cwd = payload["cwd"].as_str().unwrap_or("");
            kinds.push(EventKind::PermissionPending {
                question: cut(&format!("permission: {}", tool_call(tool, input, cwd))),
            });
        }
        "Stop" => {
            let answer = payload["last_assistant_message"].as_str().unwrap_or("");
            if !answer.trim().is_empty() {
                kinds.push(EventKind::Answer {
                    text: answer.trim().to_string(),
                });
            }
            kinds.push(turn_end(answer));
        }
        "Interrupt" => kinds.push(EventKind::Idle {
            note: "interrupted".to_string(),
        }),
        _ => {}
    }
    kinds
}

/// Who answers a PermissionRequest payload: the reviewer when the rollout's newest turn_context is
/// the payload's turn and names Codex's automatic reviewer; the user otherwise, also when the
/// rollout cannot be read or has no such turn. Turns do not interleave in a rollout, so the newest
/// turn_context is the running turn's; `last_line` finds it reading back from the end.
pub fn approver(
    payload: &Value,
    last_line: impl FnOnce(&Path, &LineMatch) -> Option<String>,
) -> Approver {
    let (Some("PermissionRequest"), Some(file), Some(turn)) = (
        payload["hook_event_name"].as_str(),
        payload["transcript_path"].as_str(),
        payload["turn_id"].as_str(),
    ) else {
        return Approver::User;
    };
    let context = last_line(Path::new(file), &|line| turn_context(line).is_some());
    let reviewer = context.and_then(|line| approvals_reviewer(&line, turn));
    match reviewer {
        Some(reviewer) if REVIEWERS.contains(&reviewer.as_str()) => Approver::Reviewer,
        _ => Approver::User,
    }
}

/// The approvals_reviewer values that mean Codex's automatic reviewer.
const REVIEWERS: [&str; 2] = ["auto_review", "guardian_subagent"];

/// The payload of a rollout line that is a turn_context.
fn turn_context(line: &str) -> Option<Value> {
    if !line.contains("\"turn_context\"") {
        return None;
    }
    let mut line: Value = serde_json::from_str(line).ok()?;
    (line["type"] == "turn_context").then(|| line["payload"].take())
}

/// The approvals_reviewer of a turn_context line, when it is `turn`'s.
fn approvals_reviewer(line: &str, turn: &str) -> Option<String> {
    let context = turn_context(line)?;
    (context["turn_id"] == turn)
        .then(|| context["approvals_reviewer"].as_str().map(String::from))
        .flatten()
}

/// A phase skill read by this shell call, unless it reads skills from several phases.
fn read_skill_phase(command: &str, workflow: &Workflow) -> Option<Phase> {
    let mut found = None;
    for words in simple_commands(command) {
        let Some(program) = words
            .first()
            .map(|word| word.rsplit('/').next().unwrap_or(word))
        else {
            continue;
        };
        if !matches!(program, "cat" | "sed" | "head" | "tail" | "less" | "bat") {
            continue;
        }
        for (index, word) in words.iter().enumerate().skip(1) {
            if matches!(words[index - 1].as_str(), ">" | ">>" | "1>" | "1>>") {
                continue;
            }
            let path = Path::new(word);
            if path.file_name().is_none_or(|name| name != "SKILL.md") {
                continue;
            }
            let skill = path
                .parent()
                .and_then(Path::file_name)
                .and_then(|name| name.to_str());
            let phase = skill.and_then(|name| workflow.phase_of(name));
            if let Some(phase) = phase {
                if found.as_ref().is_some_and(|earlier| earlier != &phase) {
                    return None;
                }
                found = Some(phase);
            }
        }
    }
    found
}

/// How the subagent tool's name ends: Codex puts its namespace in front (collaborationspawn_agent).
const SPAWN_AGENT: &str = "spawn_agent";

/// The session a payload names: Codex's session id and its rollout file; none when the id is no
/// UUID, since a resume passes it to codex as an argument.
fn session(payload: &Value) -> Option<EventKind> {
    let id = payload["session_id"].as_str().filter(|id| is_uuid(id))?;
    Some(EventKind::Session {
        id: id.to_string(),
        transcript: payload["transcript_path"].as_str().map(String::from),
    })
}

/// The skill a prompt mentions first as "$name".
fn mentioned_skill(prompt: &str) -> Option<&str> {
    prompt.split_whitespace().find_map(|word| {
        let name = word.strip_prefix('$')?;
        let name = name.trim_end_matches(|c: char| !(c.is_alphanumeric() || c == '-' || c == '_'));
        (!name.is_empty()).then_some(name)
    })
}

/// What a subagent works on: its task name, else its type; its message is encrypted.
fn subagent(input: &Value) -> String {
    ["task_name", "agent_type"]
        .iter()
        .find_map(|key| input[*key].as_str().filter(|value| !value.is_empty()))
        .unwrap_or("a subagent")
        .to_string()
}

/// `text` in single quotes for `$SHELL -lc`, a quote inside it closed and escaped.
fn shell_quoted(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

/// A prompt or a compaction begins a turn, a Stop ends it, an Esc or the compaction's end leaves
/// it at its prompt with no Stop; a subagent's (agent_id set) are not the session's.
pub fn turn(payload: &Value) -> Option<Turn> {
    if payload.get("agent_id").is_some() {
        return None;
    }
    match payload["hook_event_name"].as_str()? {
        "UserPromptSubmit" | "PreCompact" => Some(Turn::Began),
        "Stop" => Some(Turn::Ended),
        "Interrupt" | "PostCompact" => Some(Turn::Idle),
        _ => None,
    }
}

/// `text` as a TOML basic string, for a `-c key=value` Codex parses as TOML.
pub fn toml_string(text: &str) -> String {
    let mut quoted = String::from('"');
    for c in text.chars() {
        match c {
            '"' => quoted.push_str("\\\""),
            '\\' => quoted.push_str("\\\\"),
            '\n' => quoted.push_str("\\n"),
            '\t' => quoted.push_str("\\t"),
            '\r' => quoted.push_str("\\r"),
            c if c.is_control() => {
                let _ = write!(quoted, "\\u{:04X}", u32::from(c));
            }
            c => quoted.push(c),
        }
    }
    quoted.push('"');
    quoted
}

#[cfg(test)]
mod tests;
