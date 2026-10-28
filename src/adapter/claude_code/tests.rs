//! Tests of `claude_code`: recorded payloads as flow events and turns, the `--settings` hooks

use super::*;
use crate::workflow::Phase;

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
    EventKind::waiting(question)
}

fn permission(question: &str) -> EventKind {
    EventKind::Waiting {
        question: question.into(),
        questions: Vec::new(),
        permission: true,
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
            permission: false,
        }]
    );
    assert_eq!(
        events("permission_ask"),
        vec![],
        "the question already says it"
    );
    assert_eq!(
        events("permission_bash"),
        vec![permission("permission: Bash(rm -rf build)")]
    );
}

#[test]
fn a_dialog_keeps_every_question_with_its_options() {
    let [
        EventKind::Waiting {
            question: first,
            questions,
            ..
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
        vec![permission("permission: Write(src/export.rs)")]
    );
    payload["tool_input"] = json!({"file_path": "/etc/hosts"});
    assert_eq!(
        translate(&payload, &phases()),
        vec![permission("permission: Write(/etc/hosts)")]
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
        Adapter::ClaudeCode.plain_wiring(Path::new("/bin/saaio"), Some(Path::new("/worktree"))),
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
    let master: Value = serde_json::from_str(&master_settings(Path::new("/bin/saaio"))).unwrap();

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
    let master: Value = serde_json::from_str(&master_settings(Path::new("/bin/saaio"))).unwrap();

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
