use super::*;

#[test]
fn opens_the_board_on_the_card_in_view_and_draws_every_session() {
    let mut app = board_app();
    app.on_input(Routed::Command(Command::Focus(2)));

    app.on_input(Routed::Command(Command::Board));
    let terminal = render_at(&app, FLEET);

    assert!(app.is_modal());
    assert!(row(&terminal, 0).starts_with(" ▚▄▞ saaio board "));
    assert!(row(&terminal, 0).ends_with("5 sessions · 2 need you · 1 delegated · 1 done "));
    assert!(
        row(&terminal, 3).starts_with("   0   master                              you "),
        "only the selected card is marked"
    );
    assert!(row(&terminal, 3).contains(" 1 ● shop-221                        blocked "));
    assert!(row(&terminal, 3).contains("▌ 2 ● shop-230                waiting for you  "));
    assert!(row(&terminal, 4).contains("     ~/projects/shop · main"));
    assert!(
        row(&terminal, 5).contains("   4/9 tasks"),
        "{}",
        row(&terminal, 5)
    );
    let third = row(&terminal, 5);
    let shop_230 = third.split("29m").nth(1).unwrap_or_default().trim();
    assert!(
        shop_230
            .strip_prefix('▌')
            .and_then(|rest| rest.strip_suffix("31m"))
            .is_some_and(|gap| gap.trim().is_empty()),
        "a flow without tasks draws no bar: {shop_230:?}"
    );
    assert!(row(&terminal, 10).contains(" 3 ◓ export-csv               review · agent "));
    assert!(
        row(&terminal, 16).starts_with(" 2 shop-230  shop-230 · guided · flow/shop-230 · slot 1"),
        "{}",
        row(&terminal, 16)
    );
    assert!(
        row(&terminal, 41)
            .starts_with(" 0-9 select   h j k l move   enter jump in   n new flow   x clear finished flow   a every project   esc back to shop-230")
    );
    let buffer = terminal.backend().buffer();
    let theme = Theme::default();
    let bar = (0..FLEET.cols)
        .find(|&x| buffer[(x, 3 + layout::MARGIN)].symbol() == theme.glyphs.cursor)
        .unwrap();
    for y in 2..8 {
        let card = buffer[(bar, y + layout::MARGIN)].clone();
        assert_eq!(
            card.symbol(),
            theme.glyphs.cursor,
            "the bar down the whole card"
        );
        assert_eq!(card.fg, theme.palette.accent);
        assert_eq!(
            buffer[(bar + 20, y + layout::MARGIN)].bg,
            theme.palette.selection
        );
    }
    let blocked_at = row(&terminal, 2 + 1).find("blocked").unwrap();
    let blocked_x = row(&terminal, 3)[..blocked_at].chars().count() as u16 + layout::MARGIN;
    assert_eq!(
        buffer[(blocked_x, 3 + layout::MARGIN)].fg,
        Theme::default().state.blocked.fg.unwrap(),
        "the state in its color"
    );
    assert_eq!(
        buffer[(blocked_x, 3 + layout::MARGIN)].bg,
        Theme::default()
            .ui
            .background
            .bg
            .unwrap_or(ratatui::style::Color::Reset),
        "no fill"
    );
}

#[test]
fn an_idle_flow_is_ready_not_running() {
    let mut flows = fleet().flows;
    flows.push(worker(
        "shop-250",
        crate::event::Mode::Guided,
        5,
        vec![EventKind::Idle {
            note: "The design is ready for review.".into(),
        }],
    ));
    let mut app = App::new(FLEET);
    app.on_server(ServerMsg::Board(crate::fold::board(flows, NOW)));
    app.on_tick(NOW);
    let terminal = render_at(&app, FLEET);
    let sections: Vec<String> = (0..FLEET.rows - 2)
        .map(|y| side(&terminal, y).trim_end().to_string())
        .filter(|line| {
            line.starts_with(' ')
                && line
                    .trim()
                    .chars()
                    .all(|c| c.is_ascii_uppercase() || c == ' ' || c.is_ascii_digit())
                && !line.trim().is_empty()
        })
        .collect();

    assert_eq!(
        sections,
        [" NEEDS YOU 2", " READY 1", " RUNNING 1", " DONE 1"]
    );
}

#[test]
fn a_worker_that_needs_you_says_so_with_its_glyph_not_a_bar() {
    let theme = Theme::default();
    let lead = |app: &App| {
        let terminal = render_at(app, FLEET);
        let buffer = terminal.backend().buffer();
        let lead = buffer[(1, 1)].symbol().to_string();
        (lead, buffer[(2, 1)].symbol().to_string(), buffer[(2, 1)].fg)
    };
    let mut app = with_fleet();

    app.on_input(Routed::Command(Command::Focus(1)));
    assert_eq!(
        lead(&app),
        (
            " ".to_string(),
            "●".to_string(),
            theme.state.blocked.fg.unwrap()
        ),
        "shop-221 is blocked"
    );
    app.on_input(Routed::Command(Command::Focus(2)));
    assert_eq!(
        lead(&app),
        (
            " ".to_string(),
            "●".to_string(),
            theme.state.waiting.fg.unwrap()
        ),
        "shop-230 waits"
    );
}

#[test]
fn the_board_shows_what_needs_you_under_the_master() {
    let mut app = board_app();
    app.on_input(Routed::Command(Command::Board));
    let terminal = render_at(&app, FLEET);
    let rows: Vec<String> = (12..30).map(|y| row(&terminal, y)).collect();
    let at = rows
        .iter()
        .position(|line| line.starts_with(" 0 master"))
        .unwrap_or_else(|| panic!("{}", rows.join("\n")));

    assert!(
        rows[at].contains("~/projects/shop · main · 4 workers"),
        "{}",
        rows[at]
    );
    assert_eq!(rows[at + 2].trim_end(), "  NEEDS YOU");
    assert!(
        rows[at + 3].starts_with("  ● shop-221  migration needs a default value"),
        "{}",
        rows[at + 3]
    );
    assert!(
        rows[at + 4].starts_with("  ● shop-230  Which approach: a channel column"),
        "{}",
        rows[at + 4]
    );
}

#[test]
fn moves_over_the_board_and_jumps_into_a_card() {
    let mut app = board_app();
    app.on_input(Routed::Command(Command::Board));

    app.on_input(Routed::View(vec![Key::Char('l'), Key::Char('j')]));
    assert_eq!(
        app.view,
        View::Board(BoardState {
            selected: 4,
            all: false
        })
    );
    app.on_input(Routed::View(vec![Key::Char('2')]));
    let terminal = render_at(&app, FLEET);
    assert!(
        row(&terminal, 17).trim_start().starts_with("plan ● 31m"),
        "the steps under the header: {}",
        row(&terminal, 17)
    );
    assert!(
        row(&terminal, 20).contains("16:09  ● started"),
        "{}",
        row(&terminal, 20)
    );

    assert_eq!(
        app.on_input(Routed::View(vec![Key::Enter])),
        vec![Effect::Send(ClientMsg::Focus(PaneKey::Flow(
            "shop-230".into()
        )))]
    );
    assert_eq!(app.view, View::Panes);
    app.on_input(Routed::Command(Command::Board));
    assert_eq!(app.on_input(Routed::View(vec![Key::Esc])), vec![]);
    assert!(!app.is_modal());
}

#[test]
fn starts_from_a_workers_repo_and_clears_only_finished_flows() {
    let mut app = board_app();
    app.on_input(Routed::Command(Command::Focus(1)));
    app.on_input(Routed::Command(Command::NewFlow));
    let View::NewFlow(form) = &app.view else {
        panic!("expected the dialog");
    };
    assert_eq!(form.repo, PathBuf::from("/shop"));
    app.on_input(Routed::View(vec![Key::Esc]));

    app.on_input(Routed::Command(Command::Board));
    app.on_input(Routed::View(vec![Key::Char('1'), Key::Char('x')]));
    assert_eq!(
        app.notice.as_deref(),
        Some("only a finished flow can be cleared")
    );
    let effects = app.on_input(Routed::View(vec![Key::Char('4'), Key::Char('x')]));
    assert_eq!(
        effects,
        vec![Effect::Run(Job::Remove(Removal::of("shop-198")))]
    );
    app.on_ran(
        Job::Remove(Removal::of("shop-198")),
        Err(JobError::other(
            "could not remove shop-198: permission denied",
        )),
    );
    assert_eq!(
        app.notice.as_deref(),
        Some("could not remove shop-198: permission denied")
    );
}

#[test]
fn shows_every_project_and_jumps_only_into_this_ones_flows() {
    let mut app = board_app().with_store("/state/projects/shop".into());
    app.on_input(Routed::Command(Command::Board));

    let effects = app.on_input(Routed::View(vec![Key::Char('a')]));
    assert_eq!(effects, vec![Effect::Run(Job::Overview)]);
    assert_eq!(Job::Overview.args(), ["status", "--all", "--json"]);
    assert!(row(&render_at(&app, FLEET), 2).contains("reading every project…"));

    let json = serde_json::to_string(&every_project()).unwrap();
    app.on_ran(Job::Overview, Ok(json));
    let terminal = render_at(&app, FLEET);
    assert!(
        row(&terminal, 0).contains("saaio board · every project"),
        "{}",
        row(&terminal, 0)
    );
    assert!(
        row(&terminal, 0).contains("2 projects · 5 flows · 2 need you"),
        "{}",
        row(&terminal, 0)
    );
    assert!(
        row(&terminal, 2).starts_with(" shop  4 flows · 2 need you"),
        "{}",
        row(&terminal, 2)
    );
    assert!(row(&terminal, 2).ends_with("this project "));
    let api = (0..42).find(|y| row(&terminal, *y).starts_with(" api  1 flow"));
    assert!(api.is_some(), "this project comes first, then api");
    assert!(
        row(&terminal, 41).starts_with(
            " h j k l move   enter jump in   n new flow   x clear finished flow   a this project"
        ),
        "{}",
        row(&terminal, 41)
    );

    let first = fleet().flows[0].id.clone();
    assert_eq!(
        app.on_input(Routed::View(vec![Key::Enter])),
        vec![Effect::Send(ClientMsg::Focus(PaneKey::Flow(first)))]
    );
    app.on_input(Routed::Command(Command::Board));
    app.on_input(Routed::View(vec![Key::Char('a')]));
    let keys = vec![Key::Char('l'); 4];
    app.on_input(Routed::View(keys));
    assert_eq!(app.on_input(Routed::View(vec![Key::Enter])), vec![]);
    assert_eq!(app.notice.as_deref(), Some("api-7 runs in project api"));
    assert!(app.is_modal());

    app.on_input(Routed::View(vec![Key::Char('a')]));
    assert_eq!(app.view, View::Board(BoardState::default()));
}

#[test]
fn asks_for_every_project_again_every_two_seconds_while_in_view() {
    let mut app = board_app().with_store("/state/projects/shop".into());
    app.on_input(Routed::Command(Command::Board));
    app.on_input(Routed::View(vec![Key::Char('a')]));

    app.on_tick(NOW + 2_500);
    assert_eq!(app.overview_due(), vec![], "the first answer is still out");
    app.on_ran(
        Job::Overview,
        Err(JobError::other("cannot find a state directory")),
    );
    assert_eq!(
        app.notice.as_deref(),
        Some("cannot read every project: cannot find a state directory")
    );
    app.on_tick(NOW + 2_500);
    assert_eq!(app.overview_due(), vec![Effect::Run(Job::Overview)]);
    app.on_ran(
        Job::Overview,
        Ok(serde_json::to_string(&every_project()).unwrap()),
    );
    app.on_tick(NOW + 3_000);
    assert_eq!(app.overview_due(), vec![]);

    app.on_input(Routed::View(vec![Key::Esc]));
    app.on_tick(NOW + 9_000);
    assert_eq!(app.overview_due(), vec![], "not once the board is gone");
}

#[test]
fn a_working_card_takes_the_color_of_its_phase_at_full_only() {
    let mut app = board_app();
    app.on_input(Routed::Command(Command::Board));
    let theme = Theme::default();
    let colors = |level| {
        let terminal = render_with(&app, FLEET, &theme_at(level));
        let buffer = terminal.backend().buffer();
        let line = row(&terminal, 10);
        let x =
            |text: &str| line[..line.find(text).unwrap()].chars().count() as u16 + layout::MARGIN;
        [
            buffer[(x("◓"), 10 + layout::MARGIN)].fg,
            buffer[(x("review · agent"), 10 + layout::MARGIN)].fg,
        ]
    };
    let review = theme.phase.positions[2].fg.unwrap();
    let working = theme.state.working.fg.unwrap();

    assert_eq!(colors(PhaseColors::Off), [working, working], "off");
    assert_eq!(colors(PhaseColors::Steps), [working, working], "steps");
    assert_eq!(colors(PhaseColors::Full), [review, review], "full");
}

#[test]
fn a_notice_is_taken_back_only_when_it_is_the_one_meant() {
    let mut app = board_app();

    app.say("theme: theme.toml:7: accent belongs in [ui], not [state]".to_string());
    app.take_back("theme: ");
    assert_eq!(app.notice, None);

    app.say("only a finished flow can be cleared".to_string());
    app.take_back("theme: ");
    assert_eq!(
        app.notice.as_deref(),
        Some("only a finished flow can be cleared")
    );
}

#[test]
fn draws_the_keys_that_do_nothing_for_the_selected_card_faint() {
    let theme = Theme::default();
    let style_of = |app: &App, key: &str| {
        let terminal = render_at(app, FLEET);
        let footer = row(&terminal, 41);
        let at = footer.find(&format!("   {key} ")).unwrap() + 3;
        let x = footer[..at].chars().count() as u16 + layout::MARGIN;
        let cell = terminal.backend().buffer()[(x, 41 + layout::MARGIN)].clone();
        (cell.fg, cell.modifier)
    };
    let mut app = board_app();
    app.on_input(Routed::Command(Command::Board));

    assert_eq!(
        style_of(&app, "x"),
        (theme.palette.faint, ratatui::style::Modifier::empty()),
        "the master cannot be cleared"
    );
    assert_eq!(style_of(&app, "n").1, ratatui::style::Modifier::BOLD);
    let done = crate::ui::sidebar::workers(app.board.as_ref().unwrap())
        .iter()
        .position(|flow| flow.is_finished())
        .unwrap();
    app.view = View::Board(BoardState {
        selected: done + 1,
        all: false,
    });
    assert_eq!(
        style_of(&app, "x").1,
        ratatui::style::Modifier::BOLD,
        "a finished flow can"
    );
}

#[test]
fn draws_enter_and_x_faint_on_a_card_of_another_project() {
    let theme = Theme::default();
    let style_of = |app: &App, key: &str| {
        let terminal = render_at(app, FLEET);
        let footer = row(&terminal, 41);
        let at = footer.find(&format!("   {key} ")).unwrap() + 3;
        let x = footer[..at].chars().count() as u16 + layout::MARGIN;
        terminal.backend().buffer()[(x, 41 + layout::MARGIN)].fg
    };
    let mut app = board_app().with_store("/state/projects/shop".into());
    app.on_input(Routed::Command(Command::Board));
    app.on_input(Routed::View(vec![Key::Char('a')]));
    let json = serde_json::to_string(&every_project()).unwrap();
    app.on_ran(Job::Overview, Ok(json));
    let finished = crate::ui::board::every_flow(app.projects.as_ref().unwrap())
        .iter()
        .position(|(_, flow)| flow.is_finished())
        .unwrap();
    app.view = View::Board(BoardState {
        selected: finished,
        all: true,
    });
    assert_ne!(
        style_of(&app, "x"),
        theme.palette.faint,
        "a finished flow here"
    );

    app.view = View::Board(BoardState {
        selected: 4,
        all: true,
    });
    assert_eq!(
        style_of(&app, "enter"),
        theme.palette.faint,
        "api-7 runs elsewhere"
    );
    assert_eq!(style_of(&app, "x"), theme.palette.faint);
}
