use super::*;

#[test]
fn detaches_and_stops_only_after_a_yes() {
    let mut app = App::new(SIZE);

    assert!(matches!(
        app.on_input(Routed::Command(Command::Detach))[..],
        [Effect::Quit(_)]
    ));
    assert_eq!(app.on_input(Routed::Command(Command::StopServer)), vec![]);
    assert!(app.is_modal());
    assert_eq!(app.on_input(Routed::View(vec![Key::Char('n')])), vec![]);
    assert!(!app.is_modal());
    app.on_input(Routed::Command(Command::StopServer));
    let effects = app.on_input(Routed::View(vec![Key::Char('y')]));
    assert_eq!(effects[0], Effect::Send(ClientMsg::StopServer));
    assert!(matches!(effects[1], Effect::Quit(_)));
}

#[test]
fn offers_to_clean_up_a_worker_whose_session_ended() {
    let mut app = board_app();
    app.on_input(Routed::Command(Command::Focus(2)));
    let id = PaneKey::Flow("shop-230".into());
    app.on_server(ServerMsg::Panes(vec![PaneInfo {
        key: id.clone(),
        status: PaneStatus::Exited { code: Some(0) },
    }]));

    let terminal = render_at(&app, FLEET);
    let screen: String = (0..FLEET.rows - 2)
        .map(|y| row(&terminal, y) + "\n")
        .collect();
    assert!(
        screen.contains("x clean up: worktree, branch, flow   r resume   esc back to master"),
        "{screen}"
    );
    assert_eq!(
        app.on_input(Routed::Pane(b"a".to_vec())),
        vec![],
        "the pane is gone"
    );
    assert_eq!(
        app.on_input(Routed::Pane(b"r".to_vec())),
        vec![Effect::Send(ClientMsg::Reopen(id))]
    );
    assert_eq!(
        app.on_input(Routed::Pane(b"x".to_vec())),
        vec![Effect::Run(Job::Remove(Removal::of("shop-230")))]
    );
    assert_eq!(app.notice.as_deref(), Some("cleaning up shop-230…"));
    assert_eq!(
        app.on_input(Routed::Pane(vec![0x1b])),
        vec![Effect::Send(ClientMsg::Focus(PaneKey::Master))]
    );
}

/// What saaio ui reads from `saaio remove` when it fails with `failure`, as the binary prints it.
fn printed(failure: &Failure) -> Result<String, JobError> {
    outcome(false, &format!("{}\n", failure.to_toon()))
}

#[test]
fn knows_each_refusal_from_what_flow_remove_prints() {
    let uncommitted = printed(&remove::uncommitted("shop-230", "/shop")).unwrap_err();
    let unmerged = printed(&remove::unmerged("shop-230", "flow/shop-230", 1)).unwrap_err();
    let other = printed(&Failure::error("no flow named shop-230", Vec::new())).unwrap_err();

    assert_eq!(uncommitted.refusal, Some(Refusal::Uncommitted));
    assert_eq!(
        uncommitted.message,
        "shop-230 has uncommitted work in /shop"
    );
    assert_eq!(unmerged.refusal, Some(Refusal::Unmerged));
    assert_eq!(
        unmerged.message,
        "flow/shop-230 has 1 commit on no other branch"
    );
    assert_eq!(other.refusal, None);
}

#[test]
fn asks_before_a_clean_up_loses_work() {
    let mut app = board_app();
    let first = Removal::of("shop-230");

    app.on_ran(
        Job::Remove(first.clone()),
        printed(&remove::uncommitted("shop-230", "/shop")),
    );
    assert!(app.is_modal());
    let screen: String = {
        let terminal = render_at(&app, FLEET);
        (0..FLEET.rows - 2)
            .map(|y| row(&terminal, y) + "\n")
            .collect()
    };
    assert!(
        screen.contains("shop-230 has uncommitted changes. Discard them?"),
        "{screen}"
    );
    let forced = Removal {
        force: true,
        ..first.clone()
    };
    assert_eq!(
        app.on_input(Routed::View(vec![Key::Char('y')])),
        vec![Effect::Run(Job::Remove(forced.clone()))]
    );

    app.on_ran(
        Job::Remove(forced.clone()),
        printed(&remove::unmerged("shop-230", "flow/shop-230", 2)),
    );
    let View::Confirm { question, then } = &app.view else {
        panic!("expected a question: {:?}", app.view);
    };
    assert_eq!(
        question,
        "flow/shop-230 has 2 commits on no other branch. Delete it anyway?"
    );
    assert!(then.force && then.delete_unmerged);
    assert_eq!(
        Job::Remove(then.clone()).args(),
        ["remove", "shop-230", "--force", "--delete-unmerged"]
    );

    assert_eq!(app.on_input(Routed::View(vec![Key::Char('n')])), vec![]);
    assert!(!app.is_modal(), "any other key keeps the flow");
}

#[test]
fn keeps_or_ends_unfinished_flows_when_flow_stops() {
    let mut app = board_app();
    app.on_input(Routed::Command(Command::StopServer));
    let terminal = render_at(&app, FLEET);
    let dialog: String = (0..42).map(|y| row(&terminal, y) + "\n").collect();
    assert!(dialog.contains("3 flows are not finished:"), "{dialog}");
    assert!(
        dialog.contains("● shop-221    migration needs a default value"),
        "{dialog}"
    );
    assert!(
        dialog.contains("Ended, branches and worktrees stay until saaio clear."),
        "{dialog}"
    );
    assert!(
        dialog.contains("k keep them   e end them   esc keep saaio running"),
        "{dialog}"
    );

    assert_eq!(
        app.on_input(Routed::View(vec![Key::Char('y')])),
        vec![],
        "y is not an answer here"
    );
    let kept = app.on_input(Routed::View(vec![Key::Char('k')]));
    assert_eq!(kept[0], Effect::Send(ClientMsg::StopServer));
    assert_eq!(
        kept[1],
        Effect::Quit(
            "stopped flow; 3 unfinished flows resume the next time you run `saaio ui`".into()
        )
    );

    let mut app = board_app();
    app.on_input(Routed::Command(Command::StopServer));
    let ended = app.on_input(Routed::View(vec![Key::Char('e')]));
    assert_eq!(ended[0], Effect::Send(ClientMsg::EndFlows));
    assert_eq!(ended[1], Effect::Send(ClientMsg::StopServer));
    let Effect::Quit(message) = &ended[2] else {
        panic!("expected to quit: {ended:?}");
    };
    assert!(
        message
            .starts_with("stopped flow and ended 3 flows; their code stays until `saaio clear`:\n"),
        "{message}"
    );
    assert!(
        message.contains("\n  shop-221  flow/shop-221  /shop/.saaio/worktrees/shop-221"),
        "{message}"
    );

    let mut app = board_app();
    let mut board = fleet();
    board
        .flows
        .push(plain_session("scratch", "/Users/jan/scratch", 5, vec![]));
    app.on_server(ServerMsg::Board(board));
    app.on_input(Routed::Command(Command::StopServer));
    let ended = app.on_input(Routed::View(vec![Key::Char('e')]));
    let Effect::Quit(message) = &ended[2] else {
        panic!("expected to quit: {ended:?}");
    };
    assert!(
        message.contains("\n  scratch  ~/scratch"),
        "a plain session's folder: {message}"
    );

    let mut app = board_app();
    app.on_input(Routed::Command(Command::StopServer));
    assert_eq!(app.on_input(Routed::View(vec![Key::Esc])), vec![]);
    assert!(!app.is_modal());
}

#[test]
fn lists_every_key_until_any_key() {
    let mut app = with_fleet();
    app.on_input(Routed::Command(Command::Help));

    let terminal = render_at(&app, FLEET);
    let help: String = (0..42).map(|y| row(&terminal, y) + "\n").collect();
    assert!(help.contains("C-space then"), "{help}");
    assert!(
        help.contains("  b        fold the sidebar to a rail and back"),
        "{help}"
    );
    assert_eq!(side(&terminal, 41).trim_end(), " C-space ? keys");

    app.on_input(Routed::View(vec![Key::Char('x')]));
    assert!(!app.is_modal());
}

#[test]
fn starts_a_flow_from_the_dialog_in_the_repo_in_view() {
    let mut app = board_app();
    app.on_input(Routed::Command(Command::NewFlow));
    app.on_input(Routed::View(
        "SHOP-240 rate-limit the export endpoint"
            .chars()
            .map(Key::Char)
            .collect(),
    ));

    let terminal = render_at(&app, FLEET);
    let dialog: String = (10..24).map(|y| row(&terminal, y) + "\n").collect();
    assert!(
        dialog.contains("╭─ new flow ─") && dialog.contains(" in ~/projects/shop ─╮"),
        "{dialog}"
    );
    assert!(
        dialog.contains("│ SHOP-240 rate-limit the export endpoint"),
        "{dialog}"
    );
    assert!(
        dialog.contains("│  creates flow/shop-240  ·  .saaio/worktrees/shop-240  ·  slot 2"),
        "{dialog}"
    );
    assert!(
        terminal.backend().buffer()[(0, 30)]
            .modifier
            .contains(Modifier::DIM)
    );

    let effects = app.on_input(Routed::View(vec![Key::Enter]));
    let [Effect::Run(job)] = &effects[..] else {
        panic!("expected a job: {effects:?}");
    };
    assert_eq!(
        job.args(),
        [
            "start",
            "SHOP-240 rate-limit the export endpoint",
            "--mode",
            "guided"
        ]
    );
    assert_eq!(job.dir(), Some(Path::new("/Users/jan/projects/shop")));
    assert!(!app.is_modal());

    app.on_ran(job.clone(), Err(JobError::other("all 20 slots are in use")));
    let View::NewFlow(form) = &app.view else {
        panic!("the dialog comes back");
    };
    assert_eq!(form.request, "SHOP-240 rate-limit the export endpoint");
    assert_eq!(form.error.as_deref(), Some("all 20 slots are in use"));
}

#[test]
fn opens_a_plain_session_from_the_dialog() {
    let mut app = board_app();
    app.on_input(Routed::Command(Command::NewFlow));
    app.on_input(Routed::View(vec![
        Key::Tab,
        Key::Right,
        Key::Tab,
        Key::Ctrl('w'),
    ]));
    app.on_input(Routed::View("scratch".chars().map(Key::Char).collect()));

    let terminal = render_at(&app, FLEET);
    let dialog: String = (10..24).map(|y| row(&terminal, y) + "\n").collect();
    assert!(dialog.contains("╭─ new session ─"), "{dialog}");
    assert!(dialog.contains("│▌ folder "), "{dialog}");
    assert!(dialog.contains("│ ~/projects/scratch "), "{dialog}");
    assert!(
        dialog.contains("│  opens in ~/projects/scratch  ·  no worktree, branch or slot"),
        "{dialog}"
    );

    let effects = app.on_input(Routed::View(vec![Key::Enter]));
    let [Effect::Run(job)] = &effects[..] else {
        panic!("expected a job: {effects:?}");
    };
    assert_eq!(job.args(), ["start", "--plain", "~/projects/scratch"]);
    assert_eq!(job.dir(), Some(Path::new("/Users/jan/projects/shop")));
}

#[test]
fn the_dialog_from_a_plain_session_starts_in_its_folder() {
    let mut app = board_app();
    let mut board = fleet();
    board
        .flows
        .push(plain_session("scratch", "/Users/jan/scratch", 5, vec![]));
    app.on_server(ServerMsg::Board(board));
    app.focus = PaneKey::Flow("scratch".into());

    let effects = app.on_input(Routed::Command(Command::NewFlow));

    assert_eq!(
        effects,
        vec![Effect::LoadBranching(PathBuf::from("/Users/jan/scratch"))]
    );
    let View::NewFlow(form) = &app.view else {
        panic!("expected the dialog");
    };
    assert_eq!(form.folder, "~/scratch");
}

#[test]
fn names_the_branch_after_the_repos_convention_in_the_dialog() {
    let mut app = board_app();
    let repo = PathBuf::from("/Users/jan/projects/shop");

    assert_eq!(
        app.on_input(Routed::Command(Command::NewFlow)),
        vec![Effect::LoadBranching(repo.clone())]
    );
    app.on_branching(
        &repo,
        Ok(Some(Branching {
            name: "{type}/{ticket}-{slug}".into(),
            types: vec!["feature".into(), "bugfix".into()],
        })),
    );
    app.on_input(Routed::View(
        "SHOP-240 fix totals".chars().map(Key::Char).collect(),
    ));
    app.on_input(Routed::View(vec![Key::Tab, Key::Right]));

    let terminal = render_at(&app, FLEET);
    let dialog: String = (10..26).map(|y| row(&terminal, y) + "\n").collect();
    assert!(
        dialog.contains("│▌ type    ( ) feature   (•) bugfix"),
        "{dialog}"
    );
    assert!(
        dialog.contains("│  creates bugfix/SHOP-240-fix-totals"),
        "{dialog}"
    );
    let effects = app.on_input(Routed::View(vec![Key::Enter]));
    let [Effect::Run(job)] = &effects[..] else {
        panic!("expected a job: {effects:?}");
    };
    assert_eq!(
        job.args(),
        [
            "start",
            "SHOP-240 fix totals",
            "--mode",
            "guided",
            "--type",
            "bugfix"
        ]
    );

    app.on_input(Routed::Command(Command::NewFlow));
    app.on_branching(&repo, Err("/shop/.saaio/config.toml is not valid".into()));
    let View::NewFlow(form) = &app.view else {
        panic!("expected the dialog");
    };
    assert_eq!(
        form.error.as_deref(),
        Some("/shop/.saaio/config.toml is not valid")
    );
}

#[test]
fn the_new_flow_dialog_keeps_its_title_beside_a_long_place() {
    let mut app = with_fleet();
    app.on_server(ServerMsg::Master {
        cwd: "/private/tmp/claude-501/-Users-jan-projects-flow/9e99090c-c805-4120-a341-a400210fb25e/scratchpad/ui-look/repo".into(),
        branch: None,
        turn: None,
    });
    app.on_input(Routed::Command(Command::NewFlow));

    let terminal = render_at(&app, FLEET);
    let header = (5..20)
        .map(|y| row(&terminal, y))
        .find(|line| line.contains(" new flow"))
        .expect("the dialog names itself");

    assert!(header.contains("in /private/…/"), "{header}");
    assert!(header.contains("/ui-look/repo ─╮"), "{header}");
}

#[test]
fn keeps_what_a_new_flow_creates_inside_the_dialog() {
    let mut app = board_app();
    app.on_input(Routed::Command(Command::NewFlow));
    app.on_input(Routed::View(
        "List the files here with ls, then say done"
            .chars()
            .map(Key::Char)
            .collect(),
    ));

    let terminal = render_at(&app, FLEET);
    let creates = (10..24)
        .map(|y| row(&terminal, y))
        .find(|line| line.contains("creates"))
        .unwrap();

    assert!(
        creates.contains("│  creates flow/list-the-files-here  ·  slot 2"),
        "{creates}"
    );
    assert!(creates.contains("  │"), "{creates}");
}

#[test]
fn picks_the_agent_in_the_dialog_when_there_are_two() {
    let mut app = board_app().with_agents(vec!["claude".into(), "codex".into()]);
    app.on_input(Routed::Command(Command::NewFlow));
    app.on_input(Routed::View(
        "SHOP-240 fix totals".chars().map(Key::Char).collect(),
    ));
    app.on_input(Routed::View(vec![Key::Tab, Key::Right]));

    let terminal = render_at(&app, FLEET);
    let dialog: String = (10..26).map(|y| row(&terminal, y) + "\n").collect();
    assert!(
        dialog.contains("│▌ agent   ( ) claude   (•) codex"),
        "{dialog}"
    );
    let effects = app.on_input(Routed::View(vec![Key::Enter]));
    let [Effect::Run(job)] = &effects[..] else {
        panic!("expected a job: {effects:?}");
    };
    assert_eq!(
        job.args(),
        [
            "start",
            "SHOP-240 fix totals",
            "--mode",
            "guided",
            "--agent",
            "codex"
        ]
    );

    app.on_input(Routed::Command(Command::NewFlow));
    app.on_input(Routed::View(vec![
        Key::Tab,
        Key::Right,
        Key::Tab,
        Key::Right,
    ]));
    let effects = app.on_input(Routed::View(vec![Key::Enter]));
    let [Effect::Run(job)] = &effects[..] else {
        panic!("expected a job: {effects:?}");
    };
    assert_eq!(
        job.args(),
        ["start", "--plain", "~/projects/shop", "--agent", "codex"],
        "a plain session too"
    );
}

#[test]
fn shows_no_agent_row_with_one_agent() {
    let mut app = board_app().with_agents(vec!["claude".into()]);
    app.on_input(Routed::Command(Command::NewFlow));

    let terminal = render_at(&app, FLEET);
    let dialog: String = (10..26).map(|y| row(&terminal, y) + "\n").collect();
    assert!(!dialog.contains("agent "), "{dialog}");
}
