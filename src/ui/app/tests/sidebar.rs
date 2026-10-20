use super::*;

#[test]
fn shows_the_sidebar_from_the_start_and_keeps_the_pane_when_workers_come() {
    let mut app = App::new(FLEET);
    let pane = app.areas().pane;

    assert_eq!(app.areas().side.width, 40, "the master has its row");
    assert_eq!(
        app.on_server(ServerMsg::Board(fold::board(vec![], NOW))),
        vec![]
    );
    assert_eq!(
        app.on_server(ServerMsg::Board(fleet())),
        vec![],
        "the first worker resizes nothing"
    );
    assert_eq!(app.areas().pane, pane);
}

#[test]
fn says_there_are_no_workers_yet_and_how_to_start_one() {
    let mut app = App::new(FLEET);
    app.on_server(ServerMsg::Board(fold::board(vec![], NOW)));

    let terminal = render_at(&app, FLEET);

    assert_eq!(side(&terminal, 2).trim_end(), "▌0   master");
    assert_eq!(side(&terminal, 4).trim_end(), " no workers yet");
    assert_eq!(side(&terminal, 5).trim_end(), " C-space n starts a flow");
    assert_eq!(side(&terminal, 41).trim_end(), " C-space ? keys");
}

#[test]
fn draws_the_fleet_in_the_sidebar_grouped_by_what_it_needs() {
    let terminal = render_at(&with_fleet(), FLEET);

    assert_eq!(
        side(&terminal, 0),
        " ▚▄▞ saaio                              "
    );
    assert_eq!(
        side(&terminal, 2),
        "▌0   master                             "
    );
    assert_eq!(side(&terminal, 4).trim_end(), " NEEDS YOU 2");
    assert_eq!(
        side(&terminal, 5),
        " 1 ● shop-221                claude 29m "
    );
    assert_eq!(
        side(&terminal, 6),
        "     ●───●───○───○ implement        4/9 "
    );
    assert_eq!(
        side(&terminal, 7),
        "     migration needs a default value f… "
    );
    assert_eq!(
        side(&terminal, 10),
        "     ●───○───○───○ plan                 "
    );
    assert_eq!(side(&terminal, 13).trim_end(), " RUNNING 1");
    assert!(side(&terminal, 14).starts_with(" 3 ◓ export-csv"));
    assert_eq!(side(&terminal, 16).trim_end(), "     agent: review changes");
    assert_eq!(side(&terminal, 18).trim_end(), " DONE 1");
    assert_eq!(
        side(&terminal, 20).trim_end(),
        "     ●───●───●───● MR !418      took 0s",
        "a finished flow says how long it took"
    );
    assert_eq!(side(&terminal, 40).trim_end(), "", "no rule above the hint");
    assert_eq!(side(&terminal, 41).trim_end(), " C-space ? keys");
    let buffer = terminal.backend().buffer();
    assert_eq!(
        buffer[(112, 6)].fg,
        Theme::default().state.blocked.fg.unwrap()
    );
}

#[test]
fn fits_agent_and_age_by_display_width_and_moves_age_to_existing_second_row() {
    let mut app = with_fleet();
    let mut board = fleet();
    board.flows[0].name = "a-very-long-feature-name".into();
    board.flows[0].workspace.as_mut().unwrap().agent = "my-agent".into();
    app.on_server(ServerMsg::Board(board));

    let terminal = render_at(&app, FLEET);
    assert!(side(&terminal, 5).trim_end().ends_with("my-agent"));
    assert!(side(&terminal, 5).contains("a-very-long-feature-name"));
    assert!(side(&terminal, 6).trim_end().ends_with("4/9 29m"));
    assert!(side(&terminal, 9).trim_end().ends_with("claude 31m"));

    let mut board = fleet();
    board.flows[0].name = "界".repeat(12);
    app.on_server(ServerMsg::Board(board));
    let terminal = render_at(&app, FLEET);
    assert!(side(&terminal, 5).trim_end().ends_with("claude"));
    assert!(side(&terminal, 6).trim_end().ends_with("4/9 29m"));
}

#[test]
fn clips_a_long_agent_before_clipping_a_flow_name_that_fits() {
    let mut app = with_fleet();
    let mut board = fleet();
    board.flows[0].workspace.as_mut().unwrap().agent =
        "custom-agent-with-an-excessively-long-name".into();
    app.on_server(ServerMsg::Board(board));

    let terminal = render_at(&app, FLEET);
    let first = side(&terminal, 5);
    assert!(first.contains("shop-221"), "{first}");
    assert!(first.trim_end().ends_with('…'), "{first}");
    assert!(side(&terminal, 6).trim_end().ends_with("4/9 29m"));
}

#[test]
fn the_master_row_names_no_agent() {
    let terminal = render_at(&board_app(), FLEET);
    assert!(side(&terminal, 2).trim_end().ends_with("master"));
}

#[test]
fn focuses_a_worker_by_digit_or_click_and_marks_it() {
    let mut app = with_fleet();

    assert_eq!(
        app.on_input(Routed::Command(Command::Focus(2))),
        vec![Effect::Send(ClientMsg::Focus(PaneKey::Flow(
            "shop-230".into()
        )))]
    );
    let terminal = render_at(&app, FLEET);
    let title = row(&terminal, 0);
    assert!(
        title.starts_with(" ● shop-230  ●───○───○───○ plan  · flow/shop-230 · slot 1 "),
        "{title}"
    );
    assert!(
        title.ends_with("  │ ▚▄▞ saaio                              "),
        "{title}"
    );
    assert_eq!(
        side(&terminal, 2),
        " 0   master                             "
    );
    assert!(side(&terminal, 9).starts_with("▌2 ● shop-230"));
    let buffer = terminal.backend().buffer();
    let selection = Theme::default().ui.selection.bg.unwrap();
    assert!((109..149).all(|x| buffer[(x, 11)].bg == selection));
    assert_ne!(buffer[(119, 9)].bg, selection);

    assert_eq!(
        app.on_input(Routed::Pointer(Pointer {
            kind: PointerKind::Press,
            at: Position::new(120, 7)
        })),
        vec![Effect::Send(ClientMsg::Focus(PaneKey::Flow(
            "shop-221".into()
        )))]
    );
    assert_eq!(
        app.on_input(Routed::Pointer(Pointer {
            kind: PointerKind::Press,
            at: Position::new(120, 30)
        })),
        vec![]
    );
    assert_eq!(app.on_input(Routed::Command(Command::Focus(9))), vec![]);
    assert_eq!(
        app.on_input(Routed::Command(Command::Focus(0))),
        vec![Effect::Send(ClientMsg::Focus(PaneKey::Master))]
    );
}

#[test]
fn shows_workers_that_exited_failed_or_have_no_session() {
    let mut app = with_fleet();
    app.on_server(ServerMsg::Panes(vec![
        PaneInfo {
            key: PaneKey::Flow("shop-221".into()),
            status: PaneStatus::Exited { code: Some(0) },
        },
        PaneInfo {
            key: PaneKey::Flow("shop-230".into()),
            status: PaneStatus::Failed {
                reason: "no agent named codex".into(),
            },
        },
    ]));
    app.on_input(Routed::Command(Command::Focus(3)));

    let terminal = render_at(&app, FLEET);

    assert!(side(&terminal, 5).ends_with("exited "));
    assert!(side(&terminal, 9).ends_with("failed "));
    assert_eq!(side(&terminal, 11).trim_end(), "     no agent named codex");
    let pane: String = (1..43).map(|y| row(&terminal, y)).collect();
    assert!(
        pane.contains("saaio has no session for this flow."),
        "{pane}"
    );
}

#[test]
fn turns_the_spinner_only_while_a_worker_works() {
    let mut app = with_fleet();

    assert!(app.on_tick(NOW + 100), "export-csv is delegated");
    let mut idle = App::new(FLEET);
    idle.on_server(ServerMsg::Board(fold::board(vec![], NOW)));
    let redraws = (1..=20)
        .filter(|tick| idle.on_tick(NOW + tick * 100))
        .count();
    assert_eq!(redraws, 2, "only for the ages, once a second");
}

#[test]
fn names_the_state_of_a_flow_that_has_no_phase_yet() {
    use crate::event::Mode::Auto;
    let mut app = App::new(FLEET);
    let flows = vec![
        worker("fresh", Auto, 1, vec![]),
        worker(
            "asks",
            Auto,
            2,
            vec![EventKind::Waiting {
                question: "ready".into(),
                questions: Vec::new(),
            }],
        ),
    ];
    app.on_server(ServerMsg::Board(fold::board(flows, NOW)));
    app.on_tick(NOW);

    let terminal = render_at(&app, FLEET);

    assert_eq!(side(&terminal, 6).trim_end(), "     ○───○───○───○ waiting");
    assert_eq!(side(&terminal, 7).trim_end(), "     ready");
    assert_eq!(
        side(&terminal, 11).trim_end(),
        "     ○───○───○───○ starting"
    );
    assert_eq!(
        side(&terminal, 12).trim_end(),
        "",
        "no note that it started"
    );
}

#[test]
fn goes_back_to_the_master_when_the_focused_flow_is_removed() {
    let mut app = with_fleet();
    app.on_input(Routed::Command(Command::Focus(1)));
    let mut board = fleet();
    board.flows.retain(|flow| flow.name != "shop-221");

    let effects = app.on_server(ServerMsg::Board(board));

    assert_eq!(
        effects,
        vec![Effect::Send(ClientMsg::Focus(PaneKey::Master))]
    );
}

#[test]
fn peek_writes_paths_in_the_worktree_relative_and_home_with_a_tilde() {
    let mut app = with_fleet().with_home(Some("/Users/jan".into()));
    for _ in 0..3 {
        app.on_input(Routed::Command(Command::Cursor(Step::Next)));
    }
    let peek = vec![
        PeekLine::Tool {
            name: "Write".into(),
            target: "/shop/.saaio/worktrees/export-csv/PLAN.md".into(),
        },
        PeekLine::Result("/Users/jan/shop".into()),
        PeekLine::Text("The plan is in /shop/.saaio/worktrees/export-csv/PLAN.md.".into()),
        PeekLine::Now("Read(/Users/jan/notes.md)".into()),
    ];

    let terminal = render_peek(&app, &peek);

    assert_eq!(side(&terminal, 17).trim_end(), "  ⏺ Write(PLAN.md)");
    assert_eq!(side(&terminal, 18).trim_end(), "      ⎿  ~/shop");
    assert_eq!(
        side(&terminal, 19).trim_end(),
        "  ⏺ The plan is in PLAN.md."
    );
    assert_eq!(side(&terminal, 20).trim_end(), "  ◓ Read(~/notes.md)");
}

#[test]
fn peek_renders_worker_markdown_with_the_existing_glyph_and_width() {
    let mut app = with_fleet();
    for _ in 0..3 {
        app.on_input(Routed::Command(Command::Cursor(Step::Next)));
    }
    let peek = vec![PeekLine::Text(
        "**bold** and `code`\n\n- first item\n- second item".into(),
    )];

    let terminal = render_peek(&app, &peek);
    let rows: Vec<_> = (0..FLEET.rows - 2).map(|y| side(&terminal, y)).collect();
    assert!(
        rows.iter().any(|row| row.contains("⏺ bold and code")),
        "{rows:?}"
    );
    assert!(
        rows.iter().any(|row| row.contains("• first item")),
        "{rows:?}"
    );
    assert!(rows.iter().all(|row| !row.contains("**bold**")));
}

#[test]
fn peeks_into_a_worker_under_the_cursor_and_jumps_in() {
    let mut app = with_fleet();
    app.on_input(Routed::Command(Command::Cursor(Step::Next)));
    app.on_input(Routed::Command(Command::Cursor(Step::Next)));
    app.on_input(Routed::Command(Command::Cursor(Step::Next)));
    assert_eq!(
        app.peeked().map(|flow| flow.name.as_str()),
        Some("export-csv")
    );
    let peek = vec![
        PeekLine::Tool {
            name: "Update".into(),
            target: "app/Http/Middleware/ThrottleExports.php".into(),
        },
        PeekLine::Result("Added 22 lines".into()),
        PeekLine::Text(
            "The limiter keys on the user id, so one export can't starve the rest.".into(),
        ),
        PeekLine::Now("Write(ThrottleExportTest.php)".into()),
    ];

    let terminal = render_peek(&app, &peek);

    assert!(
        side(&terminal, 14).starts_with(" 3 ◓ export-csv"),
        "{}",
        side(&terminal, 14)
    );
    assert_eq!(
        side(&terminal, 15).trim_end(),
        "     ●───●───●───○ review           6/6"
    );
    assert_eq!(
        side(&terminal, 16).trim_end(),
        format!("  {}", "─".repeat(37))
    );
    assert_eq!(
        side(&terminal, 17).trim_end(),
        "  ⏺ Update(app/Http/Middleware/Thrott…)"
    );
    assert_eq!(side(&terminal, 18).trim_end(), "      ⎿  Added 22 lines");
    assert_eq!(
        side(&terminal, 19).trim_end(),
        "  ⏺ The limiter keys on the user id, so"
    );
    assert_eq!(
        side(&terminal, 20).trim_end(),
        "    one export can't starve the rest."
    );
    assert_eq!(
        side(&terminal, 21).trim_end(),
        "  ◓ Write(ThrottleExportTest.php)"
    );
    assert_eq!(side(&terminal, 22).trim_end(), "  C-space enter jump in");
    assert_eq!(side(&terminal, 23).trim_end(), "");
    assert_eq!(side(&terminal, 24).trim_end(), " DONE 1");
    assert_eq!(side(&terminal, 41).trim_end(), " C-space ? keys");

    assert_eq!(
        app.on_input(Routed::Command(Command::Jump)),
        vec![Effect::Send(ClientMsg::Focus(PaneKey::Flow(
            "export-csv".into()
        )))]
    );
    assert_eq!(app.peeked(), None);
}

#[test]
fn moves_the_cursor_past_the_ends_and_skips_the_flow_in_view() {
    let mut app = with_fleet();
    app.on_input(Routed::Command(Command::Focus(1)));

    app.on_input(Routed::Command(Command::Cursor(Step::Next)));
    assert_eq!(
        app.peeked().map(|flow| flow.name.as_str()),
        Some("shop-230")
    );
    app.on_input(Routed::Command(Command::Cursor(Step::Prev)));
    assert_eq!(app.peeked(), None);
    app.on_input(Routed::Command(Command::Cursor(Step::Prev)));
    assert_eq!(
        app.peeked().map(|flow| flow.name.as_str()),
        Some("shop-198")
    );
}

#[test]
fn peeks_at_where_an_ended_flows_code_is() {
    let mut app = with_fleet();
    for _ in 0..4 {
        app.on_input(Routed::Command(Command::Cursor(Step::Next)));
    }
    assert_eq!(
        app.peeked().map(|flow| flow.name.as_str()),
        Some("shop-198")
    );
    let peek = vec![PeekLine::Code {
        branch: Some("flow/shop-198".into()),
        worktree: "/shop/.saaio/worktrees/shop-198".into(),
        exists: false,
    }];

    let terminal = render_peek(&app, &peek);

    assert_eq!(side(&terminal, 22).trim_end(), "  branch   flow/shop-198");
    assert_eq!(side(&terminal, 23).trim_end(), "  worktree gone");
}

#[test]
fn peeks_at_an_ended_plain_sessions_folder_without_a_branch() {
    let mut app = board_app();
    app.on_server(ServerMsg::Board(fold::board(
        vec![plain_session(
            "notes",
            "/Users/jan/notes",
            200,
            vec![EventKind::Ended],
        )],
        NOW,
    )));
    app.on_input(Routed::Command(Command::Cursor(Step::Next)));
    assert_eq!(app.peeked().map(|flow| flow.name.as_str()), Some("notes"));
    let peek = vec![PeekLine::Code {
        branch: None,
        worktree: "/Users/jan/notes".into(),
        exists: true,
    }];

    let terminal = render_peek(&app, &peek);
    let rows: Vec<String> = (0..20)
        .map(|y| side(&terminal, y).trim_end().to_string())
        .collect();

    assert!(
        rows.iter().any(|row| row == "  folder   ~/notes"),
        "{rows:#?}"
    );
    assert!(!rows.iter().any(|row| row.contains("branch")), "{rows:#?}");
}

#[test]
fn a_long_place_loses_its_middle_but_keeps_the_branch_and_the_corner() {
    let mut fleet = with_fleet();
    fleet.on_server(ServerMsg::Master {
        cwd: "/private/tmp/claude-501/-Users-jan-projects-flow/9e99090c-c805-4120-a341-a400210fb25e/scratchpad/ui-look/repo".into(),
        branch: Some("main".into()),
        turn: None,
    });

    let narrow = Size { cols: 90, rows: 20 };
    let mut folded = App::new(narrow);
    folded.on_server(ServerMsg::Master {
        cwd: "/private/tmp/claude-501/-Users-jan-projects-flow/9e99090c-c805-4120-a341-a400210fb25e/scratchpad/ui-look/repo".into(),
        branch: Some("main".into()),
        turn: None,
    });
    let folded = render_at(&folded, narrow);
    let top = row(&folded, 0);
    assert!(top.contains("/private/…/"), "{top}");
    assert!(
        top.contains("/repo · main ") && top.contains("C-space b sidebar │"),
        "with the rail the title says where the master runs: {top}"
    );

    let screen = render_at(&fleet, FLEET);
    assert_eq!(
        side(&screen, 3).trim_end(),
        "▌    /private/…/ui-look/repo · main"
    );
}

#[test]
fn the_rail_marks_the_master_like_any_focused_row() {
    let mut app = board_app();
    app.on_input(Routed::Command(Command::Rail));
    let terminal = render_at(&app, FLEET);
    let rail = |y| row(&terminal, y).chars().skip(142).collect::<String>();
    let theme = Theme::default();

    assert_eq!(rail(2), "▌0    ");
    assert_eq!(rail(4), " 1 ●  ");
    let buffer = terminal.backend().buffer();
    assert_eq!(buffer[(143, 3)].fg, theme.palette.accent);
    assert_eq!(buffer[(148, 3)].bg, theme.palette.selection);
}

#[test]
fn folds_the_sidebar_to_a_rail_and_back() {
    let mut app = board_app();
    app.on_input(Routed::Command(Command::Focus(1)));

    assert_eq!(
        app.on_input(Routed::Command(Command::Rail)),
        vec![Effect::Send(ClientMsg::Resize(Size {
            cols: 141,
            rows: 41
        }))]
    );
    let terminal = render_at(&app, FLEET);
    let rail = |y| row(&terminal, y).chars().skip(142).collect::<String>();

    assert!(
        row(&terminal, 0).ends_with(" ● 2 shop-230 is waiting  │  C-space b sidebar │ ▚▄▞  "),
        "{}",
        row(&terminal, 0)
    );
    assert_eq!(rail(2), " 0    ");
    assert_eq!(rail(4), "▌1 ●  ");
    assert_eq!(rail(5), " 2 ●  ");
    assert_eq!(rail(7), " 3 ◓  ");
    assert_eq!(rail(9), " 4 ○  ");
    assert_eq!(rail(41), " C-␣  ");
    assert_eq!(
        app.on_input(Routed::Pointer(Pointer {
            kind: PointerKind::Press,
            at: Position::new(146, 6)
        })),
        vec![Effect::Send(ClientMsg::Focus(PaneKey::Flow(
            "shop-230".into()
        )))]
    );
    assert_eq!(
        app.on_input(Routed::Command(Command::Rail)),
        vec![Effect::Send(ClientMsg::Resize(Size {
            cols: 107,
            rows: 41
        }))]
    );
}

#[test]
fn a_row_stands_out_a_moment_after_its_flow_changed() {
    let selection = Theme::default().ui.selection.bg.unwrap();
    let mut app = with_fleet();
    let background_of = |app: &App, text: &str| {
        let terminal = render_at(app, FLEET);
        let y = (0..FLEET.rows - 2)
            .find(|&y| side(&terminal, y).contains(text))
            .unwrap();
        terminal.backend().buffer()[(120, y + layout::MARGIN)].bg
    };
    assert_ne!(background_of(&app, "shop-221"), selection);

    let mut board = fleet();
    let flow = board
        .flows
        .iter_mut()
        .find(|flow| flow.name == "shop-221")
        .unwrap();
    flow.state = State::Working;
    flow.blocker = None;
    app.on_server(ServerMsg::Board(board));

    assert_eq!(
        background_of(&app, "shop-221"),
        selection,
        "it just started working"
    );
    assert_ne!(
        background_of(&app, "shop-230"),
        selection,
        "nothing changed there"
    );
    assert!(
        app.on_tick(NOW + CHANGE_SHOWN_MS),
        "a redraw takes the mark away"
    );
    assert_ne!(background_of(&app, "shop-221"), selection);
}

#[test]
fn the_master_waits_for_you_after_a_turn_you_did_not_see() {
    let mut app = with_fleet();
    app.on_input(Routed::Command(Command::Focus(1)));

    app.on_server(master_at(State::Working, 1));
    assert_eq!(app.master_state(), Some(State::Working));
    app.on_server(master_at(State::Idle, 2));
    assert_eq!(app.master_state(), Some(State::Waiting), "an ended turn");
    app.on_server(master_at(State::Waiting, 3));
    assert_eq!(app.master_state(), Some(State::Waiting), "a question");
    app.on_server(master_at(State::Blocked, 4));
    assert_eq!(app.master_state(), Some(State::Blocked), "a permission");
}

#[test]
fn a_turn_that_ends_in_view_needs_nothing() {
    let mut app = with_fleet();

    app.on_server(master_at(State::Working, 1));
    assert_eq!(
        app.master_state(),
        Some(State::Working),
        "it spins in view too"
    );
    app.on_server(master_at(State::Idle, 2));
    assert_eq!(app.master_state(), None);
    app.on_input(Routed::Command(Command::Focus(1)));
    assert_eq!(
        app.master_state(),
        None,
        "looking away later changes nothing"
    );
}

#[test]
fn looking_at_the_master_clears_its_dot() {
    let mut app = with_fleet();
    app.on_input(Routed::Command(Command::Focus(1)));
    app.on_server(master_at(State::Idle, 2));

    app.on_input(Routed::Command(Command::Focus(0)));
    assert_eq!(app.master_state(), None);
    app.on_input(Routed::Command(Command::Focus(1)));
    assert_eq!(app.master_state(), None, "seen stays seen");
    app.on_server(master_at(State::Working, 3));
    app.on_server(master_at(State::Idle, 4));
    assert_eq!(
        app.master_state(),
        Some(State::Waiting),
        "until its next turn"
    );
    app.on_server(ServerMsg::Master {
        cwd: "/shop".into(),
        branch: None,
        turn: None,
    });
    assert_eq!(app.master_state(), None, "a new master runs no turn yet");
}

#[test]
fn a_woken_turn_that_ends_idle_again_shows_the_dot_again() {
    let mut app = with_fleet();
    app.on_server(master_at(State::Idle, 2));
    app.on_input(Routed::Command(Command::Focus(1)));
    assert_eq!(app.master_state(), None);

    app.on_server(master_at(State::Idle, 5));
    assert_eq!(app.master_state(), Some(State::Waiting));
}

#[test]
fn a_working_master_turns_the_spinner_and_its_row_lights_up_on_a_change() {
    let mut app = App::new(FLEET);
    app.on_server(ServerMsg::Board(fold::board(vec![], NOW)));
    app.on_tick(NOW);
    let spin = app.spin;

    app.on_server(master_at(State::Working, 1));
    assert!(app.on_tick(NOW + 100));
    assert_ne!(app.spin, spin, "the master works");
    assert!(
        !app.changed.contains_key(&PaneKey::Master),
        "its first turn is no change"
    );
    app.on_server(master_at(State::Idle, 2));
    assert!(app.changed.contains_key(&PaneKey::Master));
    let spin = app.spin;
    app.on_tick(NOW + 200);
    assert_eq!(app.spin, spin, "at rest");
}

#[test]
fn the_masters_row_draws_its_glyph_in_its_states_color_and_lights_up() {
    let theme = Theme::default();
    let selection = theme.ui.selection.bg.unwrap();
    let mut app = with_fleet();
    app.on_input(Routed::Command(Command::Focus(1)));
    let master_row = |app: &App| {
        let terminal = render_at(app, FLEET);
        let y = (0..FLEET.rows)
            .find(|&y| side(&terminal, y).contains("master"))
            .unwrap();
        let x = layout::areas(FLEET, layout::Chrome::Sidebar).side.x;
        let y_abs = layout::screen(FLEET).y + y;
        let buffer = terminal.backend().buffer();
        let cell = |dx: u16| buffer[(x + dx, y_abs)].clone();
        (side(&terminal, y), cell(3).fg, cell(20).bg)
    };
    assert_eq!(master_row(&app).0.trim_end(), " 0   master");

    app.on_server(master_at(State::Working, 1));
    let (text, glyph, _) = master_row(&app);
    assert!(
        theme
            .glyphs
            .spinner
            .iter()
            .any(|frame| text.trim_end() == format!(" 0 {frame} master")),
        "{text}"
    );
    assert_eq!(Some(glyph), theme.state.working.fg);

    app.on_server(master_at(State::Idle, 2));
    let (text, glyph, fill) = master_row(&app);
    assert_eq!(
        text.trim_end(),
        format!(" 0 {} master", theme.glyphs.waiting)
    );
    assert_eq!(Some(glyph), theme.state.waiting.fg);
    assert_eq!(fill, selection, "it just changed");
    app.on_tick(NOW + CHANGE_SHOWN_MS);
    assert_ne!(master_row(&app).2, selection);

    app.on_input(Routed::Command(Command::Rail));
    let terminal = render_at(&app, FLEET);
    let rail = (0..FLEET.rows).map(|y| row(&terminal, y)).find(|line| {
        line.trim_end()
            .ends_with(&format!(" 0 {}", theme.glyphs.waiting))
    });
    assert!(rail.is_some(), "the rail shows it too");
}

#[test]
fn notes_stay_behind_the_names_but_the_selected_one() {
    let theme = Theme::default();
    let mut app = with_fleet();
    app.on_input(Routed::Command(Command::Focus(2)));
    let terminal = render_at(&app, FLEET);
    let buffer = terminal.backend().buffer();
    let color_of = |text: &str| {
        let y = (0..FLEET.rows - 2)
            .find(|&y| side(&terminal, y).contains(text))
            .unwrap_or_else(|| panic!("no row with {text}"));
        let x = row(&terminal, y).find(text).unwrap();
        let x = row(&terminal, y)[..x].chars().count() as u16 + layout::MARGIN;
        buffer[(x, y + layout::MARGIN)].fg
    };

    assert_eq!(color_of("migration needs"), theme.ui.faint.fg.unwrap());
    assert_eq!(
        color_of("Which approach"),
        theme.ui.muted.fg.unwrap(),
        "shop-230 is in view"
    );
}

#[test]
fn only_the_current_step_takes_the_color_of_where_the_flow_is() {
    let theme = Theme::default();
    let text = ratatui::style::Color::Reset;
    let blocked = theme.state.blocked.fg.unwrap();
    let faint = theme.ui.faint.fg.unwrap();
    let terminal = render_at(&with_fleet(), FLEET);
    let buffer = terminal.backend().buffer();
    let y = (0..FLEET.rows)
        .find(|&y| side(&terminal, y).contains("●───●───○───○ implement"))
        .expect("shop-221's track")
        + layout::MARGIN;
    let x = (0..FLEET.cols)
        .find(|&x| buffer[(x, y)].symbol() == "●")
        .unwrap();

    assert_eq!(buffer[(x, y)].fg, text, "done");
    assert_eq!(
        buffer[(x + 1, y)].fg,
        text,
        "the join into the current step"
    );
    assert_eq!(buffer[(x + 4, y)].fg, blocked, "current");
    assert_eq!(buffer[(x + 8, y)].fg, faint, "open");
}

/// export-csv, delegated in review: the color of its spinner and of its done, current and open dots.
fn export_csv_colors(level: PhaseColors) -> [ratatui::style::Color; 4] {
    let theme = theme_at(level);
    let terminal = render_with(&with_fleet(), FLEET, &theme);
    let buffer = terminal.backend().buffer();
    let name = (0..FLEET.rows)
        .find(|&y| side(&terminal, y).contains("export-csv"))
        .expect("export-csv's row")
        + layout::MARGIN;
    let spinner = (0..FLEET.cols)
        .find(|&x| theme.glyphs.spinner.contains(&buffer[(x, name)].symbol()))
        .expect("the spinner");
    let track = name + 1;
    let dot = (0..FLEET.cols)
        .find(|&x| buffer[(x, track)].symbol() == "●")
        .unwrap();
    [
        buffer[(spinner, name)].fg,
        buffer[(dot, track)].fg,
        buffer[(dot + 8, track)].fg,
        buffer[(dot + 12, track)].fg,
    ]
}

#[test]
fn the_phase_colors_level_decides_what_takes_the_color_of_the_phase() {
    let theme = Theme::default();
    let review = theme.phase.positions[2].fg.unwrap();
    let working = theme.state.working.fg.unwrap();
    let faint = theme.ui.faint.fg.unwrap();
    // A done dot is text at every level; only the current one takes a color.
    let text = ratatui::style::Color::Reset;

    assert_eq!(
        export_csv_colors(PhaseColors::Off),
        [working, text, working, faint],
        "off: state colors only"
    );
    assert_eq!(
        export_csv_colors(PhaseColors::Steps),
        [working, text, review, faint],
        "steps: only the current dot"
    );
    assert_eq!(
        export_csv_colors(PhaseColors::Full),
        [review, text, review, faint],
        "full: spinner and the current dot"
    );
}

#[test]
fn a_branch_too_cut_to_read_leaves_the_title() {
    let narrow = Size { cols: 96, rows: 30 };
    let mut app = App::new(narrow);
    app.on_server(ServerMsg::Board(fleet()));
    app.on_input(Routed::Command(Command::Focus(2)));

    let title = row(&render_at(&app, narrow), 0);

    assert!(
        title.starts_with(" ● shop-230  ●───○───○───○ plan "),
        "{title}"
    );
    assert!(!title.contains("· …"), "{title}");
}
