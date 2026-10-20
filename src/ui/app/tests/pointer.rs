use super::*;
use crate::ui::selection::Selection;

/// The text a copy effect takes from what was drawn, as the loop reads it.
fn copied(app: &App, effects: &[Effect]) -> String {
    let [Effect::CopySelection(selection)] = effects else {
        panic!("not one copy: {effects:?}");
    };
    selection_text(app, *selection)
}

fn selection_text(app: &App, selection: Selection) -> String {
    let terminal = render_at(app, FLEET);
    selection.text(terminal.backend().buffer())
}

fn reversed(terminal: &Terminal<TestBackend>) -> Vec<(u16, u16)> {
    let buffer = terminal.backend().buffer();
    buffer
        .content
        .iter()
        .enumerate()
        .filter(|(_, cell)| cell.modifier.contains(Modifier::REVERSED))
        .map(|(i, _)| buffer.pos_of(i))
        .collect()
}

#[test]
fn a_drag_on_the_sidebar_copies_the_text_under_it_on_release() {
    let mut app = with_fleet();

    assert_eq!(mouse(&mut app, PointerKind::Press, 114, 6), vec![]);
    assert_eq!(mouse(&mut app, PointerKind::Drag, 118, 6), vec![]);
    let effects = mouse(&mut app, PointerKind::Release, 121, 6);

    assert_eq!(copied(&app, &effects), "shop-221");
    let terminal = render_at(&app, FLEET);
    assert_eq!(
        reversed(&terminal),
        (114..=121).map(|x| (x, 6)).collect::<Vec<_>>()
    );
}

#[test]
fn a_drag_that_leaves_its_surface_stays_inside_it() {
    let mut app = with_fleet();

    mouse(&mut app, PointerKind::Press, 114, 6);
    let effects = mouse(&mut app, PointerKind::Release, 20, 6);

    assert_eq!(copied(&app, &effects), " 1 ● s");
}

#[test]
fn a_click_focuses_and_selects_nothing() {
    let mut app = with_fleet();

    assert_eq!(
        click(&mut app, 120, 7),
        vec![Effect::Send(ClientMsg::Focus(PaneKey::Flow(
            "shop-221".into()
        )))]
    );
    assert_eq!(reversed(&render_at(&app, FLEET)), vec![]);
}

#[test]
fn the_highlight_stays_after_the_copy_until_the_next_key() {
    let mut app = with_fleet();
    mouse(&mut app, PointerKind::Press, 114, 6);
    mouse(&mut app, PointerKind::Release, 121, 6);
    assert_ne!(reversed(&render_at(&app, FLEET)), vec![]);

    app.on_input(Routed::Pane(b"x".to_vec()));

    assert_eq!(reversed(&render_at(&app, FLEET)), vec![]);
}

#[test]
fn a_double_click_copies_the_word_and_a_triple_click_the_line() {
    let mut app = with_fleet();
    assert_eq!(
        click(&mut app, 117, 6),
        vec![Effect::Send(ClientMsg::Focus(PaneKey::Flow(
            "shop-221".into()
        )))]
    );

    let word = click(&mut app, 117, 6);
    assert_eq!(copied(&app, &word), "shop-221");

    let line = click(&mut app, 117, 6);
    assert_eq!(
        copied(&app, &line),
        "▌1 ● shop-221                claude 29m"
    );
}

#[test]
fn presses_far_apart_in_time_are_single_clicks() {
    let mut app = with_fleet();
    click(&mut app, 117, 6);
    app.on_tick(NOW + 600);

    assert_eq!(
        click(&mut app, 117, 7),
        vec![],
        "a click again, on the flow in view"
    );
    click(&mut app, 117, 7);
    app.on_tick(NOW + 1200);
    assert_eq!(click(&mut app, 117, 7), vec![]);
}

#[test]
fn a_drag_in_a_dialog_stays_inside_the_dialog() {
    let mut app = with_fleet();
    app.on_input(Routed::Command(Command::Help));
    let theme = Theme::default();
    let dialog = app.dialog(&theme).unwrap().area(app.areas().session());
    let inside = (dialog.x + 2, dialog.y + 2);

    mouse(&mut app, PointerKind::Press, inside.0, inside.1);
    let effects = mouse(&mut app, PointerKind::Release, 149, 43);

    assert!(matches!(effects[..], [Effect::CopySelection(_)]));
    let cells = reversed(&render_at(&app, FLEET));
    assert!(!cells.is_empty());
    assert!(
        cells
            .iter()
            .all(|&(x, y)| dialog.contains(Position::new(x, y))),
        "{cells:?} outside {dialog:?}"
    );
}

#[test]
fn the_wheel_over_a_pane_without_the_mouse_scrolls_its_history() {
    let mut app = with_fleet();
    let pane = app.areas().pane;
    let (x, y) = (pane.x + 5, pane.y + 5);
    let history = |back| vec![Effect::Send(ClientMsg::History(back))];

    assert_eq!(mouse(&mut app, PointerKind::WheelDown, x, y), vec![]);
    assert_eq!(
        mouse(&mut app, PointerKind::WheelUp, x, y),
        history(Some(3))
    );
    assert_eq!(app.view, View::History(3));
    assert_eq!(
        mouse(&mut app, PointerKind::WheelUp, x, y),
        history(Some(6))
    );
    assert_eq!(
        mouse(&mut app, PointerKind::WheelDown, x, y),
        history(Some(3))
    );
    assert_eq!(mouse(&mut app, PointerKind::WheelDown, x, y), history(None));
    assert_eq!(app.view, View::Panes);
}

#[test]
fn the_wheel_over_the_sidebar_does_nothing() {
    let mut app = with_fleet();

    assert_eq!(mouse(&mut app, PointerKind::WheelUp, 120, 6), vec![]);
    assert_eq!(app.view, View::Panes);
}

#[test]
fn a_drag_on_the_board_selects_inside_the_board() {
    let mut app = board_app();
    app.on_input(Routed::Command(Command::Board));
    let screen = layout::screen(FLEET);

    assert_eq!(mouse(&mut app, PointerKind::Press, 0, 0), vec![]);
    assert_eq!(mouse(&mut app, PointerKind::Release, 5, 0), vec![]);
    mouse(&mut app, PointerKind::Press, screen.x, screen.y);
    let effects = mouse(&mut app, PointerKind::Release, 149, screen.y);

    assert_eq!(
        copied(&app, &effects),
        row(&render_at(&app, FLEET), 0).trim_end()
    );
}

#[test]
fn copies_what_a_pane_put_on_the_clipboard() {
    let mut app = with_fleet();

    assert_eq!(
        app.on_server(ServerMsg::Clipboard("hi".into())),
        vec![Effect::Copy("hi".into())]
    );
}
