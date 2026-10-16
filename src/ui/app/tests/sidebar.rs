use super::*;

#[test]
fn makes_room_for_the_sidebar_once_the_first_worker_shows_up() {
    let mut app = App::new(FLEET);

    assert_eq!(
        app.on_server(ServerMsg::Board(fold::board(vec![], NOW))),
        vec![]
    );
    assert_eq!(app.areas().side, None);
    assert_eq!(
        app.on_server(ServerMsg::Board(fleet())),
        vec![Effect::Send(ClientMsg::Resize(Size {
            cols: 107,
            rows: 41
        }))]
    );
    assert_eq!(app.on_server(ServerMsg::Board(fleet())), vec![]);
}

#[test]
fn draws_the_fleet_in_the_sidebar_grouped_by_what_it_needs() {
    let terminal = render_at(&with_fleet(), FLEET);

    assert_eq!(
        side(&terminal, 0),
        " ▚▄▞ flow                               "
    );
    assert_eq!(
        side(&terminal, 2),
        "▌0   master                             "
    );
    assert_eq!(side(&terminal, 4).trim_end(), " NEEDS YOU 2");
    assert_eq!(
        side(&terminal, 5),
        " 1 ● shop-221                       29m "
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
        "     ●───○───○───○ design        guided "
    );
    assert_eq!(side(&terminal, 13).trim_end(), " RUNNING 1");
    assert!(side(&terminal, 14).starts_with(" 3 ◓ export-csv"));
    assert_eq!(side(&terminal, 16).trim_end(), "     agent: review changes");
    assert_eq!(side(&terminal, 18).trim_end(), " DONE 1");
    assert_eq!(
        side(&terminal, 20).trim_end(),
        "     ●───●───●───● MR !418        in 0s",
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
        title.starts_with("▌● shop-230  ●───○───○───○ design  · flow/shop-230 · slot 1 "),
        "{title}"
    );
    assert!(
        title.ends_with("  │ ▚▄▞ flow                               "),
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
        app.on_input(Routed::Click { col: 120, row: 7 }),
        vec![Effect::Send(ClientMsg::Focus(PaneKey::Flow(
            "shop-221".into()
        )))]
    );
    assert_eq!(app.on_input(Routed::Click { col: 120, row: 30 }), vec![]);
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
        pane.contains("flow ui has no session for this flow."),
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
        side(&terminal, 14).starts_with("▌3 ◓ export-csv"),
        "{}",
        side(&terminal, 14)
    );
    assert_eq!(
        side(&terminal, 15).trim_end(),
        "▌    ●───●───●───○ review           6/6"
    );
    assert_eq!(
        side(&terminal, 16).trim_end(),
        format!("▌ {}", "─".repeat(37))
    );
    assert_eq!(
        side(&terminal, 17).trim_end(),
        "▌ ⏺ Update(app/Http/Middleware/Thrott…)"
    );
    assert_eq!(side(&terminal, 18).trim_end(), "▌     ⎿  Added 22 lines");
    assert_eq!(
        side(&terminal, 19).trim_end(),
        "▌ ⏺ The limiter keys on the user id, so"
    );
    assert_eq!(
        side(&terminal, 20).trim_end(),
        "▌   one export can't starve the rest."
    );
    assert_eq!(
        side(&terminal, 21).trim_end(),
        "▌ ◓ Write(ThrottleExportTest.php)"
    );
    assert_eq!(side(&terminal, 22).trim_end(), "▌ enter jump in");
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
        worktree: "/shop/.flow/worktrees/shop-198".into(),
        exists: false,
    }];

    let terminal = render_peek(&app, &peek);

    assert_eq!(side(&terminal, 22).trim_end(), "▌ branch   flow/shop-198");
    assert_eq!(side(&terminal, 23).trim_end(), "▌ worktree gone");
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
        rows.iter().any(|row| row == "▌ folder   ~/notes"),
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
    });

    let narrow = Size { cols: 90, rows: 20 };
    let mut solo = App::new(narrow);
    solo.on_server(ServerMsg::Master {
        cwd: "/private/tmp/claude-501/-Users-jan-projects-flow/9e99090c-c805-4120-a341-a400210fb25e/scratchpad/ui-look/repo".into(),
        branch: Some("main".into()),
    });
    let solo = render_at(&solo, narrow);
    let top = row(&solo, 0);
    assert!(top.contains("/private/…/"), "{top}");
    assert!(top.trim_end().ends_with("/repo · main"), "{top}");

    let screen = render_at(&fleet, FLEET);
    assert_eq!(
        side(&screen, 3).trim_end(),
        "     /private/…/ui-look/repo · main"
    );
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
    assert_eq!(rail(9), " 4 ✓  ");
    assert_eq!(rail(41), " C-␣  ");
    assert_eq!(
        app.on_input(Routed::Click { col: 146, row: 6 }),
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
fn what_is_done_takes_the_color_of_where_the_flow_is() {
    let theme = Theme::default();
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

    assert_eq!(buffer[(x, y)].fg, blocked, "done");
    assert_eq!(
        buffer[(x + 1, y)].fg,
        blocked,
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
    let review = theme.phase.review.fg.unwrap();
    let working = theme.state.working.fg.unwrap();
    let faint = theme.ui.faint.fg.unwrap();

    assert_eq!(
        export_csv_colors(PhaseColors::Off),
        [working, working, working, faint],
        "off: state colors only"
    );
    assert_eq!(
        export_csv_colors(PhaseColors::Steps),
        [working, review, review, faint],
        "steps: only the reached dots"
    );
    assert_eq!(
        export_csv_colors(PhaseColors::Full),
        [review, review, review, faint],
        "full: spinner and dots"
    );
}
