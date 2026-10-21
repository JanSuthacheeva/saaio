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
    translate(&fixture(name), &phases(), Approver::User)
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
fn reading_one_skill_file_starts_its_phase() {
    assert_eq!(
        events("pre_bash_skill"),
        after_session(vec![EventKind::Phase {
            phase: Phase::of("plan")
        }])
    );
}

#[test]
fn separate_skill_reads_start_each_phase() {
    let sequence = fixture("read_ahead_separate_commands");
    let reads = sequence.as_array().unwrap();
    assert_eq!(reads.len(), 4);
    for (payload, phase) in reads
        .iter()
        .zip(["plan", "merge-request", "implement", "review"])
    {
        assert!(
            translate(payload, &Workflow::builtin(), Approver::User).contains(&EventKind::Phase {
                phase: Phase::of(phase)
            }),
            "each read now signals the phase: {payload}"
        );
    }
}

#[test]
fn reading_skills_from_multiple_phases_in_one_command_changes_none() {
    assert_eq!(events("pre_bash_skills_together"), after_session(vec![]));
    let mut payload = fixture("pre_bash_skills_together");
    payload["tool_input"]["command"] = "cat /repo/.agents/skills/demo-skill/SKILL.md && cat /repo/.agents/skills/implement-plan/SKILL.md".into();
    assert_eq!(
        translate(&payload, &phases(), Approver::User),
        after_session(vec![])
    );
}

#[test]
fn mentioning_or_writing_a_skill_file_is_not_a_read() {
    let mut payload = fixture("pre_bash_skill");
    for command in [
        "echo .agents/skills/demo-skill/SKILL.md",
        "cat > .agents/skills/demo-skill/SKILL.md",
    ] {
        payload["tool_input"]["command"] = command.into();
        assert_eq!(
            translate(&payload, &phases(), Approver::User),
            after_session(vec![])
        );
    }
}

#[test]
fn a_skill_path_in_a_shell_comment_does_not_change_the_phase() {
    let mut payload = fixture("pre_bash_skill");
    payload["tool_input"]["command"] =
        "cat notes.txt # /repo/.agents/skills/demo-skill/SKILL.md".into();
    assert_eq!(
        translate(&payload, &phases(), Approver::User),
        after_session(vec![])
    );

    payload["tool_input"]["command"] = "cat /repo/.agents/skills/demo-skill/SKILL.md # /repo/.agents/skills/implement-plan/SKILL.md".into();
    assert_eq!(
        translate(&payload, &phases(), Approver::User),
        after_session(vec![EventKind::Phase {
            phase: Phase::of("plan")
        }])
    );
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
fn a_permission_is_pending_until_codex_decides_and_a_question_waits() {
    assert_eq!(
        events("permission_bash"),
        after_session(vec![EventKind::PermissionPending {
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
                question: "Do you prefer tabs or spaces?".into(),
                questions: Vec::new()
            }
        ])
    );
}

#[test]
fn a_permission_the_reviewer_answers_waits_for_no_one() {
    let payload = fixture("permission_bash");

    assert_eq!(
        translate(&payload, &phases(), Approver::Reviewer),
        after_session(vec![])
    );
}

fn rollout() -> String {
    let path = format!(
        "{}/src/adapter/fixtures/codex/rollout_auto_review.jsonl",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read_to_string(path).unwrap()
}

/// A `last_line` over `rollout`'s text.
fn last_line_of(rollout: String) -> impl FnOnce(&Path, &LineMatch) -> Option<String> {
    move |_, matches| {
        rollout
            .lines()
            .rev()
            .find(|line| matches(line))
            .map(String::from)
    }
}

#[test]
fn the_reviewer_answers_when_the_turn_routes_approvals_to_it() {
    let payload = fixture("permission_bash_reviewed");
    let mut read_from = None;

    let approver = approver(&payload, |file, matches| {
        read_from = Some(file.to_path_buf());
        last_line_of(rollout())(file, matches)
    });

    assert_eq!(approver, Approver::Reviewer);
    assert_eq!(
        read_from.unwrap(),
        Path::new(payload["transcript_path"].as_str().unwrap())
    );
}

#[test]
fn the_newest_turn_context_is_the_one_that_counts() {
    let mention = r#"{"type":"response_item","payload":{"type":"message","content":"a \"turn_context\" line"}}"#;
    let rollout = format!("{}{mention}\n", rollout());

    assert_eq!(
        approver(&fixture("permission_bash_reviewed"), last_line_of(rollout)),
        Approver::Reviewer
    );
}

#[test]
fn the_user_answers_a_turn_that_routes_approvals_to_the_user() {
    let earlier = rollout().lines().take(3).collect::<Vec<_>>().join("\n");

    assert_eq!(
        approver(&fixture("permission_bash"), last_line_of(earlier)),
        Approver::User
    );
}

#[test]
fn the_user_answers_when_the_rollout_does_not_say() {
    let payload = fixture("permission_bash_reviewed");
    let earlier = rollout().lines().take(3).collect::<Vec<_>>().join("\n");

    assert_eq!(approver(&payload, last_line_of(earlier)), Approver::User);
    assert_eq!(approver(&payload, |_, _| None), Approver::User);
    assert_eq!(
        approver(&payload, last_line_of("not json\n".to_string())),
        Approver::User
    );
}

#[test]
fn only_the_known_reviewer_values_mean_the_reviewer() {
    let payload = fixture("permission_bash_reviewed");

    for (value, expected) in [
        ("auto_review", Approver::Reviewer),
        ("guardian_subagent", Approver::Reviewer),
        ("user", Approver::User),
        ("someone_new", Approver::User),
    ] {
        let rollout = rollout().replace("\"auto_review\"", &format!("\"{value}\""));
        assert_eq!(
            approver(&payload, last_line_of(rollout)),
            expected,
            "{value}"
        );
    }
}

#[test]
fn only_a_permission_request_reads_the_rollout() {
    let read = |_: &Path, _: &LineMatch| -> Option<String> { panic!("read the rollout") };

    assert_eq!(approver(&fixture("stop"), read), Approver::User);
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
        translate(
            &serde_json::json!({"hook_event_name": "Stop"}),
            &phases(),
            Approver::User
        ),
        vec![EventKind::Idle {
            note: "turn ended".into()
        }],
        "no session id, no session"
    );
}

#[test]
fn a_session_id_that_is_no_uuid_is_no_session() {
    for forged in ["-cnotify=[\"sh\",\"-c\",\"id\"]", "../../x", "7f3c"] {
        let payload = serde_json::json!({"hook_event_name": "Stop", "session_id": forged});
        assert_eq!(
            translate(&payload, &phases(), Approver::User),
            vec![EventKind::Idle {
                note: "turn ended".into()
            }],
            "{forged}"
        );
    }
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
    let tool = HOOKS
        .iter()
        .find(|hook| hook.event == "PreToolUse")
        .unwrap();
    assert_eq!(tool.timeout, TOOL_HOOK_TIMEOUT_SECS);
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
    let recorded = Hook {
        event: "PreToolUse",
        key: "pre_tool_use",
        timeout: HOOK_TIMEOUT_SECS,
    };
    assert_eq!(
        trust_hash(&recorded, command),
        "sha256:8968411b220f168aa89dfdd45fd451ce0922ab254c2fd229348cf39cc5f8656a",
        "recorded from Codex at the 10 s timeout"
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
