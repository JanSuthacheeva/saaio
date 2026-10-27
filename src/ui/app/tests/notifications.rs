//! Tests of `App`'s desktop notifications: which state changes raise one, and what it says

use super::*;
use crate::event::{Mode, Question};

fn board(flows: Vec<FlowView>) -> ServerMsg {
    ServerMsg::Board(fold::board(flows, NOW))
}

fn shop(events: Vec<EventKind>) -> FlowView {
    worker("shop-230", Mode::Auto, 5, events)
}

fn working() -> ServerMsg {
    board(vec![shop(vec![EventKind::Working])])
}

fn waiting() -> ServerMsg {
    board(vec![shop(vec![EventKind::waiting("Which approach?")])])
}

fn blocked() -> ServerMsg {
    board(vec![shop(vec![EventKind::Blocked {
        reason: "migration needs a default".into(),
    }])])
}

fn notifications(effects: Vec<Effect>) -> Vec<String> {
    effects
        .into_iter()
        .filter_map(|effect| match effect {
            Effect::Notify(text) => Some(text),
            _ => None,
        })
        .collect()
}

fn watching_working() -> App {
    let mut app = App::new(SIZE);
    app.on_server(working());
    app
}

#[test]
fn the_first_board_raises_none() {
    let mut app = App::new(SIZE);

    assert_eq!(
        notifications(app.on_server(waiting())),
        Vec::<String>::new()
    );
}

#[test]
fn a_flow_out_of_view_that_starts_waiting_notifies_once() {
    let mut app = watching_working();

    assert_eq!(
        notifications(app.on_server(waiting())),
        vec!["shop-230 waits for you"]
    );
    assert_eq!(
        notifications(app.on_server(waiting())),
        Vec::<String>::new()
    );
}

#[test]
fn a_flow_that_appears_already_blocked_notifies() {
    let mut app = App::new(SIZE);
    app.on_server(board(vec![]));
    let failed = worker(
        "export-csv",
        Mode::Auto,
        1,
        vec![EventKind::Blocked {
            reason: "setup failed".into(),
        }],
    );

    assert_eq!(
        notifications(app.on_server(board(vec![failed]))),
        vec!["export-csv is blocked: setup failed"]
    );
}

#[test]
fn a_flow_that_goes_from_waiting_to_blocked_notifies_again() {
    let mut app = watching_working();
    app.on_server(waiting());

    assert_eq!(
        notifications(app.on_server(blocked())),
        vec!["shop-230 is blocked: migration needs a default"]
    );
}

#[test]
fn a_flow_that_asks_names_its_first_question() {
    let mut app = watching_working();
    let asks = board(vec![shop(vec![EventKind::Waiting {
        question: "two questions".into(),
        questions: vec![
            Question {
                header: "Store".into(),
                question: "Where do exports go?".into(),
                options: vec![],
                multi_select: false,
            },
            Question {
                header: "Format".into(),
                question: "CSV or JSON?".into(),
                options: vec![],
                multi_select: false,
            },
        ],
        permission: false,
    }])]);

    assert_eq!(
        notifications(app.on_server(asks)),
        vec!["shop-230 asks: Where do exports go?"]
    );
}

#[test]
fn the_flow_in_view_raises_none_while_the_terminal_has_focus() {
    let mut app = watching_working();
    app.focus(PaneKey::Flow("shop-230".into()));

    assert_eq!(
        notifications(app.on_server(waiting())),
        Vec::<String>::new()
    );
}

#[test]
fn the_flow_in_view_notifies_once_the_terminal_lost_focus() {
    let mut app = watching_working();
    app.focus(PaneKey::Flow("shop-230".into()));

    assert_eq!(app.on_input(Routed::Focus(false)), vec![]);

    assert_eq!(
        notifications(app.on_server(waiting())),
        vec!["shop-230 waits for you"]
    );
    app.on_input(Routed::Focus(true));
    assert_eq!(
        notifications(app.on_server(blocked())),
        Vec::<String>::new()
    );
}

#[test]
fn turned_off_raises_none() {
    let mut app = watching_working();
    app.set_notify(false);

    assert_eq!(
        notifications(app.on_server(waiting())),
        Vec::<String>::new()
    );
    app.on_server(working());
    app.set_notify(true);
    assert_eq!(
        notifications(app.on_server(waiting())),
        vec!["shop-230 waits for you"]
    );
}

#[test]
fn a_flow_that_comes_to_rest_without_asking_raises_none() {
    let mut app = watching_working();
    let idle = board(vec![shop(vec![EventKind::Idle {
        note: "done for now".into(),
    }])]);

    assert_eq!(notifications(app.on_server(idle)), Vec::<String>::new());
}

#[test]
fn the_master_out_of_view_notifies_when_it_waits_or_is_blocked() {
    let mut app = watching_working();
    app.focus(PaneKey::Flow("shop-230".into()));
    app.on_server(master_at(State::Working, NOW));

    assert_eq!(
        notifications(app.on_server(master_at(State::Waiting, NOW))),
        vec!["master waits for you"]
    );
    assert_eq!(
        notifications(app.on_server(master_at(State::Blocked, NOW))),
        vec!["master is blocked"]
    );
    app.on_server(master_at(State::Working, NOW));
    assert_eq!(
        notifications(app.on_server(master_at(State::Idle, NOW))),
        Vec::<String>::new()
    );
}

#[test]
fn the_masters_first_turn_raises_none() {
    let mut app = watching_working();
    app.focus(PaneKey::Flow("shop-230".into()));

    assert_eq!(
        notifications(app.on_server(master_at(State::Waiting, NOW))),
        Vec::<String>::new()
    );
}

#[test]
fn the_master_in_view_raises_none_while_the_terminal_has_focus() {
    let mut app = watching_working();
    app.on_server(master_at(State::Working, NOW));

    assert_eq!(
        notifications(app.on_server(master_at(State::Waiting, NOW))),
        Vec::<String>::new()
    );
}

#[test]
fn a_focus_event_keeps_the_notice_and_the_selection() {
    let mut app = App::new(SIZE);
    app.on_server(ServerMsg::Frame(frame_with(
        "hello world",
        Modes::default(),
    )));
    mouse(&mut app, PointerKind::Press, 2, 1);
    mouse(&mut app, PointerKind::Drag, 6, 1);
    app.say("theme.toml: bad color".into());

    app.on_input(Routed::Focus(false));

    assert_eq!(app.notice.as_deref(), Some("theme.toml: bad color"));
    assert!(app.selection.is_some());
}
