use std::collections::BTreeMap;
use std::fmt::Write;
use std::path::Path;

use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::adapter::answer::turn_end;
use crate::adapter::calls::{created_merge_request, tool_call};
use crate::adapter::{Adapter, Turn, cut};
use crate::event::{EventKind, Phase};
use crate::workflow::Workflow;

/// One hook saaio listens to: Codex's event name, the key its trust state uses, its timeout.
struct Hook {
    event: &'static str,
    key: &'static str,
    timeout: u64,
}

/// The hook events saaio listens to; Interrupt at 3 s, the most Codex allows.
const HOOKS: [Hook; 8] = [
    Hook {
        event: "UserPromptSubmit",
        key: "user_prompt_submit",
        timeout: HOOK_TIMEOUT_SECS,
    },
    Hook {
        event: "PreToolUse",
        key: "pre_tool_use",
        timeout: HOOK_TIMEOUT_SECS,
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
            Value::Array(vec![serde_json::to_value(command_hook).unwrap()]),
        ),
    ]);
    let hash = Sha256::digest(serde_json::to_string(&json).unwrap());
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
/// Of a subagent's events (agent_id set) only a phase skill it reads counts.
pub fn translate(payload: &Value, workflow: &Workflow) -> Vec<EventKind> {
    let tool = payload["tool_name"].as_str().unwrap_or("");
    let input = &payload["tool_input"];
    let event = payload["hook_event_name"].as_str().unwrap_or("");
    if payload.get("agent_id").is_some() {
        let reads = event == "PreToolUse" && tool == "Bash";
        return input["command"]
            .as_str()
            .filter(|_| reads)
            .and_then(|command| read_phase(command, workflow))
            .map(|phase| vec![EventKind::Phase { phase }])
            .unwrap_or_default();
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
        "PreToolUse" if tool.ends_with(SPAWN_AGENT) => kinds.push(EventKind::Delegated {
            description: cut(&subagent(input)),
        }),
        "PreToolUse" if tool == "Bash" => {
            let command = input["command"].as_str().unwrap_or("");
            if let Some(phase) = read_phase(command, workflow) {
                kinds.push(EventKind::Phase { phase });
            }
        }
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
        "PermissionRequest" if !tool.is_empty() => {
            let cwd = payload["cwd"].as_str().unwrap_or("");
            kinds.push(EventKind::Waiting {
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

/// How the subagent tool's name ends: Codex puts its namespace in front (collaborationspawn_agent).
const SPAWN_AGENT: &str = "spawn_agent";

/// The session a payload names: Codex's session id and its rollout file.
fn session(payload: &Value) -> Option<EventKind> {
    let id = payload["session_id"].as_str().filter(|id| !id.is_empty())?;
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

/// The phase of the first skill a shell command reads whose name is in the map: Codex reads
/// ".../skills/<name>/SKILL.md", often several in one command.
fn read_phase(command: &str, workflow: &Workflow) -> Option<Phase> {
    read_skills(command).find_map(|skill| workflow.phase_of(skill))
}

/// The skills whose SKILL.md a shell command reads: ".../skills/<name>/SKILL.md" -> name.
fn read_skills(command: &str) -> impl Iterator<Item = &str> {
    command
        .split(|c: char| c.is_whitespace() || c == '\'' || c == '"')
        .filter_map(|word| {
            let dir = word.strip_suffix("/SKILL.md")?;
            let (parent, name) = dir.rsplit_once('/')?;
            let in_skills = parent == "skills" || parent.ends_with("/skills");
            (in_skills && !name.is_empty()).then_some(name)
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
mod tests {
    use super::*;

    fn fixture(name: &str) -> Value {
        let path = format!(
            "{}/src/adapter/fixtures/codex/{name}.json",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    fn phases() -> Workflow {
        Workflow::of(&[
            ("plan", &["demo-skill"]),
            ("implement", &["implement-plan"]),
        ])
    }

    fn events(name: &str) -> Vec<EventKind> {
        translate(&fixture(name), &phases())
    }

    /// The session every recorded payload ran in.
    fn session() -> EventKind {
        EventKind::Session {
            id: "01a14000-0000-7000-8000-000000000000".into(),
            transcript: Some(
                "/home/dev/.codex/sessions/2026/10/16/\
                 rollout-2026-10-16T14-51-43-01a14000-0000-7000-8000-000000000000.jsonl"
                    .into(),
            ),
        }
    }

    fn after_session(kinds: Vec<EventKind>) -> Vec<EventKind> {
        std::iter::once(session()).chain(kinds).collect()
    }

    #[test]
    fn a_prompt_means_working_and_a_mentioned_skill_its_phase() {
        assert_eq!(
            events("user_prompt"),
            after_session(vec![EventKind::Working])
        );
        assert_eq!(
            events("user_prompt_skill"),
            after_session(vec![
                EventKind::Working,
                EventKind::Phase {
                    phase: Phase::of("plan")
                }
            ])
        );
    }

    #[test]
    fn reading_a_skill_file_starts_its_phase() {
        assert_eq!(
            events("pre_bash_skill"),
            after_session(vec![EventKind::Phase {
                phase: Phase::of("plan")
            }])
        );
    }

    #[test]
    fn finds_the_phase_skill_among_several_read_at_once() {
        let command = "cat /home/dev/.agents/skills/clickup-axi/SKILL.md && \
                       sed -n 1,200p '/home/dev/.agents/skills/demo-skill/SKILL.md'";

        assert_eq!(read_phase(command, &phases()), Some(Phase::of("plan")));
        assert_eq!(read_phase("cat README.md", &phases()), None);
    }

    #[test]
    fn a_subagent_is_delegated_until_the_main_agent_goes_on() {
        assert_eq!(
            events("pre_spawn_agent"),
            after_session(vec![EventKind::Delegated {
                description: "count_files".into()
            }])
        );
        assert_eq!(events("post_spawn_agent"), after_session(vec![]));
        assert_eq!(events("pre_wait_agent"), after_session(vec![]));
        assert_eq!(
            events("post_wait_agent"),
            after_session(vec![EventKind::Working])
        );
        assert_eq!(events("subagent_bash"), vec![], "a subagent's own tools");
    }

    #[test]
    fn after_a_tool_it_works_on_and_reports_merge_requests() {
        assert_eq!(events("post_bash"), after_session(vec![EventKind::Working]));
        assert_eq!(
            events("post_bash_merge_request"),
            after_session(vec![
                EventKind::Working,
                EventKind::MergeRequest {
                    url: "https://github.com/probe/repo/pull/7".into()
                }
            ])
        );
    }

    #[test]
    fn a_permission_or_a_question_waits_for_you() {
        assert_eq!(
            events("permission_bash"),
            after_session(vec![EventKind::Waiting {
                question: "permission: Bash(touch notes.txt)".into()
            }])
        );
        assert_eq!(
            events("stop_question"),
            after_session(vec![
                EventKind::Answer {
                    text: "Do you prefer tabs or spaces?".into()
                },
                EventKind::Waiting {
                    question: "Do you prefer tabs or spaces?".into()
                }
            ])
        );
    }

    #[test]
    fn a_compaction_is_a_turn_that_changes_nothing() {
        assert_eq!(events("pre_compact"), after_session(vec![]));
        assert_eq!(events("post_compact"), after_session(vec![]));
        assert_eq!(turn(&fixture("pre_compact")), Some(Turn::Began));
        assert_eq!(turn(&fixture("post_compact")), Some(Turn::Idle));
    }

    #[test]
    fn a_turn_ends_idle_and_an_interrupt_too() {
        assert_eq!(
            events("stop"),
            after_session(vec![
                EventKind::Answer {
                    text: "demo skill ran.".into()
                },
                EventKind::Idle {
                    note: "demo skill ran.".into()
                }
            ])
        );
        assert_eq!(
            events("interrupt"),
            after_session(vec![EventKind::Idle {
                note: "interrupted".into()
            }])
        );
    }

    #[test]
    fn a_session_start_or_end_only_names_the_session() {
        assert_eq!(events("session_start_resume"), vec![session()]);
        assert_eq!(events("session_end"), vec![session()]);
        assert_eq!(
            translate(&serde_json::json!({"hook_event_name": "Stop"}), &phases()),
            vec![EventKind::Idle {
                note: "turn ended".into()
            }],
            "no session id, no session"
        );
    }

    #[test]
    fn hooks_every_event_to_flow_hook_codex() {
        let args = hooks(Path::new("/opt/my saaio/saaio"));

        assert_eq!(args[..2], ["-c", "check_for_update_on_startup=false"]);
        let values: Vec<&String> = args.iter().skip(3).step_by(2).collect();
        assert!(args.iter().skip(2).step_by(2).all(|flag| flag == "-c"));
        assert_eq!(values.len(), HOOKS.len() + 1);
        for (hook, value) in HOOKS.iter().zip(&values) {
            let (key, toml_value) = value.split_once('=').unwrap();
            assert_eq!(key, format!("hooks.{}", hook.event));
            let parsed: toml::Table = toml::from_str(&format!("v = {toml_value}")).unwrap();
            let handler = &parsed["v"][0]["hooks"][0];
            assert_eq!(handler["type"].as_str(), Some("command"));
            assert_eq!(
                handler["command"].as_str(),
                Some("'/opt/my saaio/saaio' hook codex"),
                "{}",
                hook.event
            );
            assert_eq!(handler["timeout"].as_integer(), Some(hook.timeout as i64));
        }
        let (_, toml_value) = values.last().unwrap().split_once('=').unwrap();
        let parsed: toml::Table = toml::from_str(&format!("v = {toml_value}")).unwrap();
        let state = parsed["v"].as_table().unwrap();
        let command = "'/opt/my saaio/saaio' hook codex";

        assert_eq!(state.len(), HOOKS.len());
        for hook in HOOKS {
            let key = format!("{SESSION_FLAGS}:{}:0:0", hook.key);
            let hash = trust_hash(&hook, command);
            assert_eq!(state[&key]["trusted_hash"].as_str(), Some(hash.as_str()));
        }
        assert!(hooks(Path::new("/b'in/saaio"))[3].contains(r"'/b'\\''in/saaio'"));
    }

    #[test]
    fn hashes_hooks_as_codex_trusts_them() {
        let command = "'/Users/jansuthacheeva/.cargo/bin/saaio' hook codex";
        let hashes: Vec<_> = HOOKS
            .iter()
            .map(|hook| (hook.key, trust_hash(hook, command)))
            .collect();

        assert_eq!(
            hashes.iter().find(|(key, _)| *key == "stop").unwrap().1,
            "sha256:7fdf0398b9d5ac50aee6888193db70211a1b5026483ffe30b99bfcf90fd0bf23"
        );
        assert_eq!(
            hashes
                .iter()
                .find(|(key, _)| *key == "interrupt")
                .unwrap()
                .1,
            "sha256:50f2e8b23093a3fc4e12bf9edb639b04b46adb4cbac23d59fcd894d300b9528c"
        );
        assert_eq!(
            hashes
                .iter()
                .find(|(key, _)| *key == "pre_tool_use")
                .unwrap()
                .1,
            "sha256:8968411b220f168aa89dfdd45fd451ce0922ab254c2fd229348cf39cc5f8656a"
        );
    }

    #[test]
    fn writes_any_text_as_a_toml_string() {
        let text = "line \"one\"\n\tC:\\path\u{7}";
        let parsed: toml::Table = toml::from_str(&format!("v = {}", toml_string(text))).unwrap();

        assert_eq!(parsed["v"].as_str(), Some(text));
    }

    #[test]
    fn trusts_a_folder_without_writing_config() {
        let folder = Path::new("/worktrees/with.a-quote\"/");
        let args = trusted_folder(folder);

        assert_eq!(args[0], "-c");
        let parsed: toml::Table = toml::from_str(&args[1]).unwrap();
        assert_eq!(
            parsed["projects"][folder.display().to_string()]["trust_level"].as_str(),
            Some("trusted")
        );
    }
}
