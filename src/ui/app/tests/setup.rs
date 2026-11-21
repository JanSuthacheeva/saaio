//! Tests of `App` re-running a flow's setup: C-space s, the board's s, y, and what it says

use super::*;

/// Where a worker of the fleet is in the sidebar, as the digit that focuses it.
fn digit_of(app: &App, name: &str) -> u8 {
    let at = app
        .workers()
        .iter()
        .position(|flow| flow.name == name)
        .unwrap();
    u8::try_from(at + 1).unwrap()
}

#[test]
fn asks_before_setting_up_the_worker_in_view_and_runs_it_on_y() {
    let mut app = with_fleet();
    let digit = digit_of(&app, "shop-221");
    app.on_input(Routed::Command(Command::Focus(digit)));

    let asked = app.on_input(Routed::Command(Command::Setup));

    assert_eq!(asked, vec![]);
    assert_eq!(
        app.view,
        View::Confirm {
            question: "Run teardown, then setup, for shop-221?".into(),
            then: Confirmed::Setup("shop-221".into()),
        }
    );
    let terminal = render_at(&app, FLEET);
    let screen: Vec<String> = (0..FLEET.rows - 2).map(|y| row(&terminal, y)).collect();
    assert!(
        screen.iter().any(|line| line.contains("─ set up again ─")),
        "{screen:#?}"
    );
    assert!(
        screen
            .iter()
            .any(|line| line.contains("any other key runs nothing")),
        "{screen:#?}"
    );
    let effects = app.on_input(Routed::View(vec![Key::Char('y')]));
    assert_eq!(effects, vec![Effect::Run(Job::Setup("shop-221".into()))]);
    assert_eq!(app.notice.as_deref(), Some("setting up shop-221…"));
    assert_eq!(
        Job::Setup("shop-221".into()).args(),
        ["setup", "--", "shop-221"]
    );

    app.on_ran(Job::Setup("shop-221".into()), Ok(String::new()));
    assert_eq!(app.notice.as_deref(), Some("shop-221 is set up"));
    app.on_ran(
        Job::Setup("shop-221".into()),
        Err(JobError::other(
            "setup failed: Please run composer update (full output: /s/setup.log)",
        )),
    );
    assert_eq!(
        app.notice.as_deref(),
        Some("setup failed: Please run composer update (full output: /s/setup.log)")
    );
}

#[test]
fn any_other_key_than_y_runs_nothing() {
    let mut app = with_fleet();
    let digit = digit_of(&app, "shop-230");
    app.on_input(Routed::Command(Command::Focus(digit)));
    app.on_input(Routed::Command(Command::Setup));

    let effects = app.on_input(Routed::View(vec![Key::Char('n')]));

    assert_eq!(effects, vec![]);
    assert_eq!(app.view, View::Panes);
}

#[test]
fn says_why_the_master_a_plain_session_or_an_ended_flow_has_no_setup() {
    let mut app = with_fleet();
    app.on_input(Routed::Command(Command::Focus(0)));
    assert_eq!(app.on_input(Routed::Command(Command::Setup)), vec![]);
    assert_eq!(
        app.notice.as_deref(),
        Some("setup runs for a worker's flow, not the master")
    );

    let digit = digit_of(&app, "shop-198");
    app.on_input(Routed::Command(Command::Focus(digit)));
    app.on_input(Routed::Command(Command::Setup));
    assert_eq!(app.notice.as_deref(), Some("shop-198 has ended"));
    assert_eq!(app.view, View::Panes);

    let mut board = fleet();
    board
        .flows
        .push(plain_session("scratch", "/tmp/scratch", 5, vec![]));
    app.on_server(ServerMsg::Board(board));
    let digit = digit_of(&app, "scratch");
    app.on_input(Routed::Command(Command::Focus(digit)));
    app.on_input(Routed::Command(Command::Setup));
    assert_eq!(
        app.notice.as_deref(),
        Some("scratch is a plain session: it has no setup")
    );
}

#[test]
fn the_board_sets_up_the_selected_card_after_a_yes() {
    let mut app = board_app();
    app.on_input(Routed::Command(Command::Board));
    let digit = char::from(b'0' + digit_of(&app, "export-csv"));

    app.on_input(Routed::View(vec![Key::Char(digit), Key::Char('s')]));

    assert_eq!(
        app.view,
        View::Confirm {
            question: "Run teardown, then setup, for export-csv?".into(),
            then: Confirmed::Setup("export-csv".into()),
        }
    );
    let effects = app.on_input(Routed::View(vec![Key::Char('y')]));
    assert_eq!(effects, vec![Effect::Run(Job::Setup("export-csv".into()))]);

    app.on_input(Routed::Command(Command::Board));
    app.on_input(Routed::View(vec![Key::Char('0'), Key::Char('s')]));
    assert_eq!(
        app.notice.as_deref(),
        Some("setup runs for a worker's flow, not the master")
    );
}
