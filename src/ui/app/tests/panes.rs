//! Tests of `App` on the panes: keys, modes, sizes, the master, history, any terminal size

use super::*;
use crate::proto::Peer;

#[test]
fn sends_keys_to_the_pane_and_mirrors_the_modes_it_asks_for() {
    let mut app = App::new(SIZE);

    assert_eq!(
        app.on_input(Routed::Pane(b"hi".to_vec())),
        vec![Effect::Send(ClientMsg::Input(b"hi".to_vec()))]
    );
    let modes = Modes {
        paste: true,
        mouse: Mouse::Drag,
        ..Modes::default()
    };
    assert_eq!(
        app.on_server(ServerMsg::Frame(frame_with("x", modes))),
        vec![Effect::Mirror(modes)]
    );
    assert_eq!(
        app.on_server(ServerMsg::Frame(frame_with("y", modes))),
        vec![]
    );
}

#[test]
fn tells_the_server_the_pane_size_after_a_resize() {
    let mut app = App::new(SIZE);

    assert_eq!(
        app.on_resize(Size {
            cols: 150,
            rows: 30
        }),
        vec![Effect::Send(ClientMsg::Resize(Size {
            cols: 107,
            rows: 27
        }))]
    );
    assert_eq!(
        app.on_resize(Size {
            cols: 100,
            rows: 30
        }),
        vec![Effect::Send(ClientMsg::Resize(Size { cols: 91, rows: 27 }))],
        "narrower than RAIL_BELOW the sidebar folds to the rail"
    );
}

#[test]
fn folds_to_the_rail_below_its_width_until_c_space_b_chooses() {
    let wide = Size {
        cols: 150,
        rows: 30,
    };
    let narrow = Size { cols: 96, rows: 30 };
    let mut app = App::new(narrow);
    assert!(app.folded());

    app.on_input(Routed::Command(Command::Rail));
    assert!(!app.folded(), "C-space b brings the sidebar back");
    app.on_resize(Size { cols: 90, rows: 30 });
    assert!(!app.folded(), "a resize keeps the choice");
    app.on_input(Routed::Command(Command::Rail));
    app.on_resize(wide);
    assert!(app.folded(), "folded by hand stays folded when wide");

    let mut fresh = App::new(wide);
    assert!(!fresh.folded());
    fresh.on_resize(narrow);
    assert!(fresh.folded());
}

#[test]
fn draws_a_question_longer_than_any_terminal() {
    let mut app = App::new(SIZE);
    app.view = View::Confirm {
        question: "x".repeat(usize::from(u16::MAX) - 2),
        then: Removal::of("shop-230"),
    };

    let terminal = render(&app);

    assert!((0..8).any(|y| row(&terminal, y).contains("xxx")));
}

#[test]
fn lets_go_when_another_ui_attaches_and_says_which() {
    let mut app = App::new(SIZE).with_home(Some("/Users/jan".into()));
    let by = Peer {
        pid: 4321,
        cwd: "/Users/jan/projects/flow".into(),
        build: "0.1.0 2904734".into(),
    };

    let lost = Peer {
        cwd: "".into(),
        ..by.clone()
    };

    assert_eq!(
        app.on_server(ServerMsg::Detached { by }),
        vec![Effect::Quit(
            "another terminal took over: pid 4321 in ~/projects/flow (saaio 0.1.0 2904734)"
                .to_string()
        )]
    );
    assert_eq!(
        app.on_server(ServerMsg::Detached { by: lost }),
        vec![Effect::Quit(
            "another terminal took over: pid 4321 in an unknown folder (saaio 0.1.0 2904734)"
                .to_string()
        )]
    );
}

#[test]
fn quits_with_text_a_terminal_cannot_take_as_commands() {
    let mut app = App::new(SIZE);
    let by = Peer {
        pid: 4321,
        cwd: "/tmp/\x1b]52;c;aGk=\x07x".into(),
        build: "0.1.0\x1b[2J".into(),
    };

    assert_eq!(
        app.on_server(ServerMsg::Detached { by }),
        vec![Effect::Quit(
            "another terminal took over: pid 4321 in /tmp/]52;c;aGk=x (saaio 0.1.0[2J)".to_string()
        )]
    );
    assert_eq!(
        app.on_server(ServerMsg::Refused {
            reason: "old\x1b]0;title\x07".into()
        }),
        vec![Effect::Quit("old]0;title".to_string())]
    );
}

#[test]
fn draws_the_master_with_its_title_its_cursor_and_the_rail() {
    let mut app = App::new(SIZE);
    app.on_server(ServerMsg::Frame(frame_with("hello", Modes::default())));

    let mut terminal = render(&app);

    assert!(
        row(&terminal, 0).starts_with(" master "),
        "{}",
        row(&terminal, 0)
    );
    assert!(
        row(&terminal, 1).starts_with("hello"),
        "{}",
        row(&terminal, 1)
    );
    assert!(
        row(&terminal, 0).ends_with("│ ▚▄▞  "),
        "80 columns fold the sidebar: {}",
        row(&terminal, 0)
    );
    assert!(
        row(&terminal, 2).ends_with("│▌0    "),
        "{}",
        row(&terminal, 2)
    );
    terminal.backend_mut().assert_cursor_position((6, 2));
}

#[test]
fn shows_when_the_master_has_exited() {
    let mut app = App::new(SIZE);
    app.on_server(ServerMsg::Panes(vec![PaneInfo {
        key: PaneKey::Master,
        status: PaneStatus::Exited { code: Some(0) },
    }]));

    let terminal = render(&app);

    let screen: String = (0..8).map(|y| row(&terminal, y)).collect();
    assert!(screen.contains("The session ended."), "{screen}");
    assert!(screen.contains("C-space r resumes it"), "{screen}");
    assert_eq!(
        app.on_input(Routed::Command(Command::Reopen)),
        vec![Effect::Send(ClientMsg::Reopen(PaneKey::Master))]
    );
}

#[test]
fn scrolls_the_history_and_goes_back_to_live() {
    let mut app = App::new(SIZE);

    assert_eq!(
        app.on_input(Routed::Command(Command::History)),
        vec![Effect::Send(ClientMsg::History(Some(0)))]
    );
    assert_eq!(
        app.on_input(Routed::View(vec![Key::Char('k'), Key::Char('k')])),
        vec![
            Effect::Send(ClientMsg::History(Some(1))),
            Effect::Send(ClientMsg::History(Some(2)))
        ]
    );
    app.on_input(Routed::View(vec![Key::Char('g')]));
    let mut top = frame_with("1", Modes::default());
    top.history = Some(120);
    app.on_server(ServerMsg::Frame(top));
    assert_eq!(
        app.view,
        View::History(120),
        "the server says where the history ends"
    );
    assert_eq!(
        app.on_input(Routed::View(vec![Key::Ctrl('d')])),
        vec![Effect::Send(ClientMsg::History(Some(117)))]
    );

    let terminal = render(&app);
    assert!(
        row(&terminal, 7).contains("↑ 117 lines back   k j scroll   g G ends   q live"),
        "{}",
        row(&terminal, 7)
    );
    app.on_resize(Size { cols: 36, rows: 10 });
    let narrow = render_at(&app, Size { cols: 36, rows: 10 });
    let last = app.areas().pane.bottom() - 1;
    assert!(
        row(&narrow, last).contains("↑ 117 lines back   q live │"),
        "whole hints give way, q stays: {}",
        row(&narrow, last)
    );
    assert_eq!(
        app.on_input(Routed::View(vec![Key::Char('q')])),
        vec![Effect::Send(ClientMsg::History(None))]
    );
    assert!(!app.is_modal());
}

#[test]
fn shows_where_the_master_runs_with_home_as_a_tilde() {
    let master = ServerMsg::Master {
        cwd: "/Users/jan/projects/shop".into(),
        branch: Some("main".into()),
        turn: None,
    };
    let mut alone = App::new(FLEET).with_home(Some("/Users/jan".into()));
    alone.on_server(master.clone());
    let mut fleet = with_fleet().with_home(Some("/Users/jan".into()));
    fleet.on_server(master);

    let alone = render_at(&alone, FLEET);
    let sidebar = render_at(&fleet, FLEET);

    assert_eq!(side(&alone, 3).trim_end(), "▌    ~/projects/shop · main");
    assert!(
        row(&alone, 0).starts_with(" master  main "),
        "{}",
        row(&alone, 0)
    );
    assert!(
        row(&sidebar, 0).starts_with(" master  main "),
        "the sidebar says where it runs: {}",
        row(&sidebar, 0)
    );
    assert_eq!(side(&sidebar, 3).trim_end(), "▌    ~/projects/shop · main");
    assert_eq!(side(&sidebar, 5).trim_end(), " NEEDS YOU 2");
}

#[test]
fn paints_every_cell_on_the_themes_ground() {
    let app = board_app();
    let theme = Theme::from_palette(crate::ui::theme::Palette::SAAIO);
    let mut terminal = Terminal::new(TestBackend::new(FLEET.cols, FLEET.rows)).unwrap();

    terminal.draw(|frame| app.draw(frame, &theme, &[])).unwrap();

    let buffer = terminal.backend().buffer();
    let ground = theme.palette.background;
    let painted = (0..FLEET.rows)
        .flat_map(|y| (0..FLEET.cols).map(move |x| (x, y)))
        .filter(|(x, y)| buffer[(*x, *y)].bg == ground)
        .count();
    assert!(
        painted > (FLEET.cols as usize * FLEET.rows as usize) * 9 / 10,
        "{painted} cells on the ground"
    );
    assert_eq!(buffer[(5, 5)].bg, ground, "the pane's empty cells too");
}

#[test]
fn draws_every_view_in_a_terminal_of_any_size() {
    let views = [
        View::Panes,
        View::Board(BoardState::default()),
        View::Board(BoardState {
            selected: 2,
            all: true,
        }),
        View::NewFlow(NewFlowForm::new(
            PathBuf::from("/shop"),
            "/shop".into(),
            Vec::new(),
        )),
        View::History(3),
        View::Stop(vec!["shop-221".into()]),
        View::Help,
        View::Confirm {
            question: "shop-230 has uncommitted changes. Discard them?".into(),
            then: Removal::of("shop-230"),
        },
    ];
    for view in views {
        for (cols, rows) in (0..12).flat_map(|cols| (0..12).map(move |rows| (cols * 7, rows * 3))) {
            let mut app = board_app();
            app.rail = Some(cols % 2 == 0);
            app.on_resize(Size { cols, rows });
            app.view = view.clone();
            render_at(&app, Size { cols, rows });
        }
    }
}

#[test]
fn reads_what_a_flow_command_printed() {
    assert_eq!(
        outcome(true, "flow: shop-240\nid: x\n"),
        Ok("flow: shop-240".into())
    );
    assert_eq!(
        outcome(false, "error: all 20 slots are in use\nhelp[1]: ..."),
        Err(JobError::other("all 20 slots are in use"))
    );
}

#[test]
fn reads_an_error_toon_quoted_without_its_quotes() {
    let failure = Failure::error(
        "could not create flow in /shop: Permission denied (os error 13)",
        vec!["Set SAAIO_HOME to a directory".to_string()],
    );
    let printed = format!("{}\n", failure.to_toon());
    assert!(
        printed.starts_with("error: \""),
        "TOON quotes it: {printed}"
    );

    assert_eq!(
        outcome(false, &printed),
        Err(JobError::other(
            "could not create flow in /shop: Permission denied (os error 13)"
        ))
    );
    assert_eq!(
        outcome(false, "error: \"say \\\"hi\\\": now\"\n"),
        Err(JobError::other("say \"hi\": now")),
        "escapes inside the quotes are undone"
    );
}

#[test]
fn draws_for_a_terminal_that_shrank_before_its_resize_arrived() {
    let mut app = board_app();
    let theme = Theme::default();
    let shrunk = Size {
        cols: FLEET.cols - 20,
        rows: FLEET.rows - 5,
    };
    let mut terminal = Terminal::new(TestBackend::new(shrunk.cols, shrunk.rows)).unwrap();

    terminal.draw(|frame| app.draw(frame, &theme, &[])).unwrap();

    let buffer = terminal.backend().buffer();
    let header: String = (shrunk.cols - 40..shrunk.cols)
        .map(|x| buffer[(x, 1)].symbol().to_string())
        .collect();
    assert!(
        header.contains("saaio"),
        "the sidebar at the new edge: {header:?}"
    );
    app.on_input(Routed::Command(Command::Board));
    terminal.draw(|frame| app.draw(frame, &theme, &[])).unwrap();
}
