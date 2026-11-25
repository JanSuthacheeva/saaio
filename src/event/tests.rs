use super::*;

#[test]
fn names_modes_and_phases_as_the_log_does() {
    for mode in Mode::ALL {
        assert_eq!(serde_json::to_value(mode).unwrap(), mode.name());
    }
    let phase = phase("merge-request");
    assert_eq!(serde_json::to_value(&phase).unwrap(), "merge-request");
    assert_eq!(phase.name(), "merge-request");
}

fn phase(name: &str) -> Phase {
    name.parse().unwrap()
}

#[test]
fn a_phase_is_named_in_lowercase_letters_digits_and_hyphens() {
    assert_eq!(phase("step-2").name(), "step-2");
    for bad in ["", "Plan", "my phase", "plan!", "über"] {
        let error = bad.parse::<Phase>().unwrap_err();
        assert!(error.contains("lowercase letters, digits and -"), "{error}");
    }
    assert!(
        serde_json::from_str::<Phase>(r#""Review""#).is_err(),
        "a log cannot smuggle one in"
    );
}

fn every_kind() -> Vec<EventKind> {
    vec![
        EventKind::Started {
            name: "shop-221".into(),
            request: "SHOP-221 add CSV export".into(),
            mode: Mode::Auto,
            phases: Some(vec![phase("plan"), phase("ship")]),
            kind: None,
            workflow: None,
            prompt: None,
            merges: true,
        },
        EventKind::Started {
            name: "scratch".into(),
            request: String::new(),
            mode: Mode::Plain,
            phases: None,
            kind: None,
            workflow: None,
            prompt: None,
            merges: true,
        },
        EventKind::Phase {
            phase: phase("merge-request"),
        },
        EventKind::Working,
        EventKind::Delegated {
            description: "implement plan".into(),
        },
        EventKind::waiting("Approve the plan?"),
        EventKind::Waiting {
            question: "Which color?".into(),
            questions: vec![color()],
            permission: false,
        },
        EventKind::Blocked {
            reason: "Task 5: migration needs a default".into(),
        },
        EventKind::Decision {
            text: "approach B".into(),
        },
        EventKind::Tasks { done: 4, total: 9 },
        EventKind::Review { fixed: 3, open: 1 },
        EventKind::MergeRequest {
            url: "https://gitlab.com/acme/shop/-/merge_requests/88".into(),
        },
        EventKind::Merged {
            url: "https://github.com/acme/shop/pull/42".into(),
            into: Some("main".into()),
        },
        EventKind::MergeRequestClosed {
            url: "https://github.com/acme/shop/pull/41".into(),
        },
        EventKind::Typed {
            text: "/compact".into(),
        },
        EventKind::Ended,
        EventKind::Workspace {
            path: "/Users/jan/scratch".into(),
            branch: None,
            slot: None,
            window: None,
            agent: "claude".into(),
            base: None,
        },
        EventKind::Workspace {
            path: "/repo/.saaio/worktrees/shop-222".into(),
            branch: Some("flow/shop-222".into()),
            slot: Some(3),
            window: None,
            agent: "claude".into(),
            base: Some("flow/shop-221".into()),
        },
        EventKind::BaseMerged {
            base: "flow/shop-221".into(),
            onto: "main".into(),
        },
        EventKind::Services {
            status: ServiceStatus::Paused,
            reason: None,
        },
        EventKind::Services {
            status: ServiceStatus::ResumeFailed,
            reason: Some("port 8003 is taken".into()),
        },
        EventKind::Setup {
            failed: Some("Please run composer update".into()),
            log: "/store/hooks/0b6e/setup.log".into(),
        },
        EventKind::Setup {
            failed: None,
            log: "/store/hooks/0b6e/setup.log".into(),
        },
    ]
}

fn color() -> Question {
    Question {
        header: "Color".into(),
        question: "Which color?".into(),
        options: vec![
            Choice {
                label: "Red".into(),
                description: "A warm, bold color".into(),
            },
            Choice {
                label: "Blue".into(),
                description: "A cool, steady color".into(),
            },
        ],
        multi_select: false,
    }
}

#[test]
fn every_kind_survives_a_round_trip_through_one_line() {
    for kind in every_kind() {
        let event = Event::new(1_791_945_698_308, kind);
        let line = event.to_line();
        assert!(!line.contains('\n'));
        assert_eq!(Event::from_line(&line), Some(event));
    }
}

#[test]
fn writes_a_setup_line_without_failed_when_it_passed_and_cleans_its_texts() {
    let passed = Event::new(
        7,
        EventKind::Setup {
            failed: None,
            log: "/s/setup.log".into(),
        },
    );
    assert_eq!(
        passed.to_line(),
        r#"{"v":1,"at":7,"kind":"setup","log":"/s/setup.log"}"#
    );

    let failed = Event::new(
        7,
        EventKind::Setup {
            failed: Some(format!("no\u{1b}[31m ext-zip{}", "x".repeat(TEXT_LIMIT))),
            log: format!("/s/\u{7}{}", "l".repeat(LONG_TEXT_LIMIT)),
        },
    );
    let EventKind::Setup {
        failed: Some(reason),
        log,
    } = failed.kind
    else {
        panic!("kind changed");
    };
    assert!(reason.starts_with("no[31m ext-zip"), "{reason}");
    assert_eq!(reason.chars().count(), TEXT_LIMIT);
    assert!(log.starts_with("/s/l"));
    assert_eq!(log.chars().count(), LONG_TEXT_LIMIT);
}

#[test]
fn writes_the_version_time_and_kind_as_flat_fields() {
    let event = Event::new(
        7,
        EventKind::Blocked {
            reason: "no default".into(),
        },
    );

    assert_eq!(
        event.to_line(),
        r#"{"v":1,"at":7,"kind":"blocked","reason":"no default"}"#
    );
    assert_eq!(
        Event::new(
            7,
            EventKind::Phase {
                phase: phase("merge-request")
            }
        )
        .to_line(),
        r#"{"v":1,"at":7,"kind":"phase","phase":"merge-request"}"#
    );
    assert_eq!(
        Event::new(
            7,
            EventKind::Typed {
                text: "/compact".into()
            }
        )
        .to_line(),
        r#"{"v":1,"at":7,"kind":"typed","text":"/compact"}"#
    );
}

#[test]
fn writes_a_turn_waited_for_and_taken() {
    let waiting = Event::new(
        7,
        EventKind::TurnWaiting {
            command: "artisan test".into(),
            holder: "shop-42".into(),
        },
    );
    let line = waiting.to_line();
    assert_eq!(
        line,
        r#"{"v":1,"at":7,"kind":"turn-waiting","command":"artisan test","holder":"shop-42"}"#
    );
    assert_eq!(Event::from_line(&line), Some(waiting));
    assert_eq!(
        Event::new(8, EventKind::TurnTaken).to_line(),
        r#"{"v":1,"at":8,"kind":"turn-taken"}"#
    );
    assert_eq!(
        Event::new(9, EventKind::Prompted).to_line(),
        r#"{"v":1,"at":9,"kind":"prompted"}"#
    );
}

#[test]
fn writes_an_edit_with_its_path() {
    let edited = Event::new(
        7,
        EventKind::Edited {
            path: "/repo/src/csv.rs".into(),
        },
    );
    let line = edited.to_line();
    assert_eq!(
        line,
        r#"{"v":1,"at":7,"kind":"edited","path":"/repo/src/csv.rs"}"#
    );
    assert_eq!(Event::from_line(&line), Some(edited));
}

#[test]
fn writes_services_with_a_reason_only_when_one_failed() {
    let services = |status, reason: Option<&str>| {
        Event::new(
            7,
            EventKind::Services {
                status,
                reason: reason.map(str::to_string),
            },
        )
        .to_line()
    };

    assert_eq!(
        services(ServiceStatus::Paused, None),
        r#"{"v":1,"at":7,"kind":"services","status":"paused"}"#
    );
    assert_eq!(
        services(ServiceStatus::ResumeFailed, Some("port 8003 is taken")),
        r#"{"v":1,"at":7,"kind":"services","status":"resume-failed","reason":"port 8003 is taken"}"#
    );
    for status in [
        ServiceStatus::Pausing,
        ServiceStatus::Paused,
        ServiceStatus::Resuming,
        ServiceStatus::Running,
        ServiceStatus::PauseFailed,
        ServiceStatus::ResumeFailed,
    ] {
        assert_eq!(serde_json::to_value(status).unwrap(), status.name());
    }
}

#[test]
fn cuts_a_typed_line_to_the_long_limit() {
    let long = "x".repeat(LONG_TEXT_LIMIT + 5);

    let event = Event::new(0, EventKind::Typed { text: long });

    let EventKind::Typed { text } = event.kind else {
        panic!("kind changed");
    };
    assert_eq!(text.chars().count(), LONG_TEXT_LIMIT);
}

#[test]
fn cuts_every_text_to_the_limit_in_characters_but_the_request() {
    let long = "ä".repeat(TEXT_LIMIT + 20);
    let event = Event::new(
        0,
        EventKind::Started {
            name: long.clone(),
            request: long.clone(),
            mode: Mode::Guided,
            phases: None,
            kind: None,
            workflow: None,
            prompt: None,
            merges: true,
        },
    );

    let EventKind::Started { name, request, .. } = event.kind else {
        panic!("kind changed");
    };
    assert_eq!(name.chars().count(), TEXT_LIMIT);
    assert_eq!(
        request, long,
        "the request is the worker's prompt, kept whole"
    );

    let short = Event::new(0, EventKind::waiting("ok?"));
    assert_eq!(short.kind, EventKind::waiting("ok?"));
}

#[test]
fn cuts_every_text_of_a_dialog_to_the_limit() {
    let long = "x".repeat(TEXT_LIMIT + 5);
    let mut asked = color();
    asked.header = long.clone();
    asked.question = long.clone();
    asked.options[0].label = long.clone();
    asked.options[1].description = long.clone();

    let event = Event::new(
        0,
        EventKind::Waiting {
            question: long,
            questions: vec![asked],
            permission: false,
        },
    );

    let EventKind::Waiting {
        question,
        questions,
        ..
    } = event.kind
    else {
        panic!("kind changed");
    };
    let asked = &questions[0];
    for text in [
        &question,
        &asked.header,
        &asked.question,
        &asked.options[0].label,
        &asked.options[1].description,
    ] {
        assert_eq!(text.chars().count(), TEXT_LIMIT);
    }
}

#[test]
fn keeps_no_control_character_and_at_most_8_questions_of_8_options() {
    let dirty = "a\x1b[2Jb\u{9b}c\x07\r\nd";
    let asked = Question {
        header: dirty.into(),
        question: dirty.into(),
        options: (0..50)
            .map(|_| Choice {
                label: dirty.into(),
                description: dirty.into(),
            })
            .collect(),
        multi_select: false,
    };

    let event = Event::new(
        0,
        EventKind::Waiting {
            question: dirty.into(),
            questions: vec![asked; 20],
            permission: false,
        },
    );

    let EventKind::Waiting {
        question,
        questions,
        ..
    } = event.kind
    else {
        panic!("kind changed");
    };
    assert_eq!(question, "a[2Jbc\nd");
    assert_eq!(questions.len(), MAX_QUESTIONS);
    for asked in &questions {
        assert_eq!(asked.options.len(), MAX_QUESTIONS);
        assert_eq!(
            (asked.header.as_str(), asked.question.as_str()),
            ("a[2Jbc\nd", "a[2Jbc\nd")
        );
        for choice in &asked.options {
            assert_eq!(
                (choice.label.as_str(), choice.description.as_str()),
                ("a[2Jbc\nd", "a[2Jbc\nd")
            );
        }
    }
}

#[test]
fn a_text_keeps_its_lines_and_tabs_but_no_other_control_character() {
    let dirty = "Done.\n\n\tcode\x1b[31m red\u{9b}2J\r\n- next\x07";

    for kind in [
        EventKind::Answer { text: dirty.into() },
        EventKind::Message { text: dirty.into() },
        EventKind::Typed { text: dirty.into() },
    ] {
        let (EventKind::Answer { text } | EventKind::Message { text } | EventKind::Typed { text }) =
            Event::new(0, kind).kind
        else {
            panic!("kind changed");
        };
        assert_eq!(text, "Done.\n\n\tcode[31m red2J\n- next");
    }
    let EventKind::Blocked { reason } = Event::new(
        0,
        EventKind::Blocked {
            reason: dirty.into(),
        },
    )
    .kind
    else {
        panic!("kind changed");
    };
    assert_eq!(reason, "Done.\n\n\tcode[31m red2J\n- next");
}

#[test]
fn writes_a_dialog_in_camel_case_and_reads_a_waiting_line_without_one() {
    let line = Event::new(
        7,
        EventKind::Waiting {
            question: "Which color?".into(),
            questions: vec![color()],
            permission: false,
        },
    )
    .to_line();
    assert!(line.contains(r#""multiSelect":false"#), "{line}");

    let old = r#"{"v":1,"at":7,"kind":"waiting","question":"ok?"}"#;
    assert_eq!(
        Event::from_line(old).map(|event| event.kind),
        Some(EventKind::waiting("ok?"))
    );
    assert_eq!(
        Event::new(7, Event::from_line(old).unwrap().kind).to_line(),
        old
    );
}

#[test]
fn writes_a_permission_only_when_the_harness_asks_one() {
    let asked = EventKind::Waiting {
        question: "permission: Bash(ls)".into(),
        questions: Vec::new(),
        permission: true,
    };
    let line = Event::new(7, asked.clone()).to_line();
    assert_eq!(
        line,
        r#"{"v":1,"at":7,"kind":"waiting","question":"permission: Bash(ls)","permission":true}"#
    );
    assert_eq!(Event::from_line(&line).map(|event| event.kind), Some(asked));

    let worded = Event::new(7, EventKind::waiting("ok?")).to_line();
    assert_eq!(
        worded,
        r#"{"v":1,"at":7,"kind":"waiting","question":"ok?"}"#
    );
    let old = r#"{"v":1,"at":7,"kind":"waiting","question":"permission: Bash(ls)"}"#;
    assert_eq!(
        Event::from_line(old).map(|event| event.kind),
        Some(EventKind::waiting("permission: Bash(ls)"))
    );
}

#[test]
fn reads_a_workspace_line_written_before_plain_sessions() {
    let line = r#"{"v":1,"at":7,"kind":"workspace","path":"/repo/.saaio/worktrees/shop-221","branch":"flow/shop-221","slot":2,"window":"saaio-ui","agent":"claude"}"#;

    let Some(Event {
        kind: EventKind::Workspace {
            branch, slot, base, ..
        },
        ..
    }) = Event::from_line(line)
    else {
        panic!("an old workspace line parses");
    };
    assert_eq!(branch.as_deref(), Some("flow/shop-221"));
    assert_eq!(slot, Some(2));
    assert_eq!(base, None);
    assert_eq!(Event::from_line(line).unwrap().to_line(), line);
}

#[test]
fn reads_a_started_line_written_before_phases_were_recorded() {
    let line = r#"{"v":1,"at":7,"kind":"started","name":"shop-221","request":"x","mode":"guided"}"#;

    let Some(Event {
        kind: EventKind::Started { phases, .. },
        ..
    }) = Event::from_line(line)
    else {
        panic!("an old started line parses");
    };
    assert_eq!(phases, None);
    let plain = Event::new(
        7,
        EventKind::Started {
            name: "scratch".into(),
            request: String::new(),
            mode: Mode::Plain,
            phases: None,
            kind: None,
            workflow: None,
            prompt: None,
            merges: true,
        },
    );
    assert!(!plain.to_line().contains("phases"), "left out when None");
    let Some(Event {
        kind:
            EventKind::Started {
                kind,
                workflow,
                prompt,
                merges,
                ..
            },
        ..
    }) = Event::from_line(line)
    else {
        panic!("an old started line parses");
    };
    assert_eq!((kind, workflow, prompt, merges), (None, None, None, true));
}

#[test]
fn writes_a_flows_kind_workflow_and_prompt_in_its_started_line() {
    let fix = Workflow::of(&[("diagnose", &["diagnose-bug"]), ("review", &[])]);
    let started = |merges| {
        Event::new(
            7,
            EventKind::Started {
                name: "shop-230".into(),
                request: "SHOP-230 fix totals".into(),
                mode: Mode::Guided,
                phases: None,
                kind: Some("fix".into()),
                workflow: Some(fix.clone()),
                prompt: Some("/ship-fix {request}".into()),
                merges,
            },
        )
    };

    let line = started(true).to_line();
    assert_eq!(
        line,
        r#"{"v":1,"at":7,"kind":"started","name":"shop-230","request":"SHOP-230 fix totals","mode":"guided","flow_kind":"fix","workflow":[{"name":"diagnose","skills":["diagnose-bug"]},{"name":"review","skills":[]}],"prompt":"/ship-fix {request}"}"#
    );
    assert_eq!(Event::from_line(&line), Some(started(true)));
    let research = started(false).to_line();
    assert!(research.ends_with(r#","merges":false}"#), "{research}");
    assert_eq!(Event::from_line(&research), Some(started(false)));
}

#[test]
fn plain_is_not_a_mode_flag_takes() {
    assert!(Mode::from_str("plain", false).is_err());
    assert_eq!(Mode::from_str("auto", false), Ok(Mode::Auto));
}

#[test]
fn skips_broken_lines_other_versions_and_unknown_kinds() {
    assert_eq!(Event::from_line("not json"), None);
    assert_eq!(Event::from_line(r#"{"v":1,"at":7"#), None);
    assert_eq!(Event::from_line(r#"{"v":2,"at":7,"kind":"working"}"#), None);
    assert_eq!(
        Event::from_line(r#"{"v":1,"at":7,"kind":"teleported"}"#),
        None
    );
    assert_eq!(
        Event::from_line(r#"{"v":1,"at":7,"kind":"working"}"#),
        Some(Event {
            v: 1,
            at: 7,
            kind: EventKind::Working
        })
    );
}

#[test]
fn reads_every_readable_line_of_a_log_in_order() {
    let mut log = Vec::new();
    log.extend_from_slice(b"{\"v\":1,\"at\":1,\"kind\":\"working\"}\n\n");
    log.extend_from_slice(b"{\"v\":1,\"at\":2,\"ki\n");
    log.extend_from_slice(b"{\"v\":1,\"at\":5,\"kind\":\"blocked\",\"reason\":\"\xc3\"}\n");
    log.extend_from_slice(b"{\"v\":1,\"at\":3,\"kind\":\"working\"}\r\n");
    log.extend_from_slice(b"{\"v\":1,\"at\":4,\"kind\":\"working\"}");

    assert_eq!(
        Event::from_log(&log)
            .iter()
            .map(|event| event.at)
            .collect::<Vec<_>>(),
        [1, 3, 4]
    );
    assert_eq!(Event::from_log(b""), []);
}

#[test]
fn skips_a_session_line_whose_id_is_no_id() {
    let line = |id: &str| format!(r#"{{"v":1,"at":7,"kind":"session","id":"{id}"}}"#);
    let id = "01a14000-0000-7000-8000-000000000000";

    assert_eq!(
        Event::from_line(&line(id)).map(|event| event.kind),
        Some(EventKind::Session {
            id: SessionId::parse(id).unwrap(),
            transcript: None
        })
    );
    for forged in [
        "-cnotify=x",
        "../../x",
        "01A14000-0000-7000-8000-000000000000",
    ] {
        assert_eq!(Event::from_line(&line(forged)), None, "{forged}");
    }
}

mod properties {
    use proptest::prelude::*;

    use super::*;
    use crate::fuzzing;
    use crate::strategies;

    /// An event's line cut anywhere, or with one byte changed.
    fn damaged_line() -> impl Strategy<Value = String> {
        let cut = (strategies::event(), any::<prop::sample::Index>()).prop_map(|(event, at)| {
            let line = event.to_line().into_bytes();
            String::from_utf8_lossy(&line[..at.index(line.len() + 1)]).into_owned()
        });
        let changed = (
            strategies::event(),
            any::<prop::sample::Index>(),
            any::<u8>(),
        )
            .prop_map(|(event, at, byte)| {
                let mut line = event.to_line().into_bytes();
                let at = at.index(line.len());
                line[at] = byte;
                String::from_utf8_lossy(&line).into_owned()
            });
        prop_oneof![
            strategies::text(),
            strategies::event().prop_map(|event| event.to_line()),
            cut,
            changed,
        ]
    }

    proptest! {
        #[test]
        fn any_event_survives_its_log_line(event in strategies::event()) {
            let line = event.to_line();

            prop_assert!(!line.contains('\n'));
            fuzzing::log_line(&line);
            prop_assert_eq!(Event::from_line(&line), Some(event));
        }

        #[test]
        fn cleaning_an_event_twice_changes_nothing(event in strategies::event()) {
            prop_assert_eq!(Event::new(event.at, event.kind.clone()), event);
        }

        #[test]
        fn any_line_reads_back_as_it_was(lines in prop::collection::vec(damaged_line(), 0..6)) {
            for line in &lines {
                fuzzing::log_line(line);
            }
            fuzzing::log(lines.join("\n").as_bytes());
        }
    }
}
