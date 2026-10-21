use super::*;
use crate::config::prefix::Prefix;

fn c_b() -> Prefix {
    Prefix::try_from("C-b".to_string()).unwrap()
}

fn prefixed(mut app: App) -> App {
    app.set_prefix(c_b());
    app
}

fn screen(terminal: &Terminal<TestBackend>, size: Size) -> String {
    let rows = layout::screen(size).height;
    (0..rows).map(|y| row(terminal, y) + "\n").collect()
}

#[test]
fn names_a_prefix_changed_while_it_runs() {
    let mut app = with_fleet();
    assert_eq!(
        side(&render_at(&app, FLEET), 41).trim_end(),
        " C-space ? keys"
    );

    app.set_prefix(c_b());

    assert_eq!(side(&render_at(&app, FLEET), 41).trim_end(), " C-b ? keys");
}

#[test]
fn the_help_and_the_sidebar_name_the_configured_prefix() {
    let mut app = prefixed(with_fleet());
    app.on_input(Routed::Command(Command::Help));

    let terminal = render_at(&app, FLEET);
    let help = screen(&terminal, FLEET);

    assert!(help.contains("C-b then"), "{help}");
    assert!(help.contains("  C-b      send C-b itself"), "{help}");
    assert!(!help.contains("C-space"), "{help}");
    assert_eq!(side(&terminal, 41).trim_end(), " C-b ? keys");
}

#[test]
fn the_empty_sidebar_and_the_title_row_name_the_configured_prefix() {
    let mut app = prefixed(App::new(FLEET));
    app.on_server(ServerMsg::Board(fold::board(vec![], NOW)));
    let terminal = render_at(&app, FLEET);
    assert_eq!(side(&terminal, 5).trim_end(), " C-b n starts a flow");

    let narrow = Size { cols: 90, rows: 20 };
    let folded = render_at(&prefixed(App::new(narrow)), narrow);
    let top = row(&folded, 0);
    assert!(top.contains("C-b b sidebar"), "{top}");
}

#[test]
fn the_peek_names_the_configured_prefix() {
    let mut app = prefixed(with_fleet());
    app.on_input(Routed::Command(Command::Cursor(Step::Next)));

    let terminal = render_peek(&app, &[PeekLine::Result("Added 22 lines".into())]);
    let rows = layout::screen(FLEET).height;
    let side: String = (0..rows).map(|y| side(&terminal, y) + "\n").collect();

    assert!(side.contains("  C-b enter jump in"), "{side}");
}

#[test]
fn an_ended_session_says_the_configured_prefix_resumes_it() {
    let mut app = prefixed(with_fleet());
    app.on_server(ServerMsg::Panes(vec![PaneInfo {
        key: PaneKey::Master,
        status: PaneStatus::Exited { code: Some(0) },
    }]));
    let exited = screen(&render_at(&app, FLEET), FLEET);
    assert!(exited.contains("C-b r resumes it"), "{exited}");
    assert!(exited.contains("C-b Q stops saaio"), "{exited}");

    app.on_input(Routed::Command(Command::Board));
    let board = screen(&render_at(&app, FLEET), FLEET);
    assert!(board.contains("exited - C-b r resumes it"), "{board}");

    let mut app = prefixed(with_fleet());
    app.on_input(Routed::Command(Command::Focus(1)));
    let not_open = screen(&render_at(&app, FLEET), FLEET);
    assert!(not_open.contains("C-b r resumes it"), "{not_open}");
    assert!(not_open.contains("C-b 0 back to the master"), "{not_open}");
}
