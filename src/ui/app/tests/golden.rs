//! Tests of `App`'s look: every view against its file in `src/ui/golden`

use super::*;

/// The whole screen as text, compared with src/ui/golden/<name>.txt. SAAIO_UPDATE_GOLDEN=1
/// writes the file instead, for a change to the look that is meant; a missing file is written
/// and fails, so a new view is looked at once.
fn golden(name: &str, terminal: &Terminal<TestBackend>) {
    let buffer = terminal.backend().buffer();
    let text: String = (0..buffer.area.height)
        .map(|y| {
            let line: String = (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol().to_string())
                .collect();
            line.trim_end().to_string() + "\n"
        })
        .collect();
    let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src/ui/golden")
        .join(format!("{name}.txt"));
    let update = std::env::var_os("SAAIO_UPDATE_GOLDEN").is_some();
    match std::fs::read_to_string(&file) {
        Ok(expected) if !update => assert!(
            expected == text,
            "{name} looks different; if that is meant, run SAAIO_UPDATE_GOLDEN=1 cargo test\n{text}"
        ),
        found => {
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(&file, &text).unwrap();
            assert!(
                update || found.is_ok(),
                "wrote {}; look at it, then run the tests again",
                file.display()
            );
        }
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "one golden file per view, listed in a row"
)]
#[test]
fn every_view_looks_as_it_did() {
    let mut alone = App::new(FLEET).with_home(Some("/Users/jan".into()));
    alone.on_server(ServerMsg::Master {
        cwd: "/Users/jan/projects/shop".into(),
        branch: Some("main".into()),
        turn: None,
    });
    golden("master-alone", &render_at(&alone, FLEET));
    alone.on_input(Routed::Command(Command::Board));
    golden("empty-board", &render_at(&alone, FLEET));

    golden("fleet", &render_at(&board_app(), FLEET));

    let mut long = board_app();
    let mut board = super::fleet();
    board.flows[0].name = "a-very-long-feature-name".into();
    board.flows[0].workspace.as_mut().unwrap().agent = "my-agent".into();
    long.on_server(ServerMsg::Board(board));
    golden("long-agent", &render_at(&long, FLEET));

    let mut worker = board_app();
    worker.on_input(Routed::Command(Command::Focus(2)));
    golden("worker", &render_at(&worker, FLEET));

    let mut peek = board_app();
    peek.on_input(Routed::Command(Command::Cursor(Step::Next)));
    golden(
        "peek",
        &render_peek(
            &peek,
            &[PeekLine::Text(
                "**Done** with `code`:\n\n- first item\n- second item".into(),
            )],
        ),
    );

    let mut rail = board_app();
    rail.on_input(Routed::Command(Command::Rail));
    golden("rail", &render_at(&rail, FLEET));

    let update = Some("0.1.1 is out: saaio update".to_string());
    let mut release = board_app();
    release.set_update(update.clone());
    golden("update", &render_at(&release, FLEET));
    release.on_input(Routed::Command(Command::Rail));
    golden("rail-update", &render_at(&release, FLEET));
    let mut cramped = board_app();
    cramped.set_update(update);
    let tight = Size { cols: 70, rows: 20 };
    cramped.on_resize(tight);
    golden("narrow-update", &render_at(&cramped, tight));

    let mut working = board_app();
    working.on_server(master_at(State::Working, 1));
    golden("master-working", &render_at(&working, FLEET));
    let mut waiting = board_app();
    waiting.on_input(Routed::Command(Command::Focus(2)));
    waiting.on_server(master_at(State::Idle, 1));
    golden("master-waiting", &render_at(&waiting, FLEET));
    waiting.on_input(Routed::Command(Command::Rail));
    golden("rail-master-waiting", &render_at(&waiting, FLEET));

    let mut board = board_app();
    board.on_input(Routed::Command(Command::Board));
    golden("board", &render_at(&board, FLEET));
    let mut detail = board_app();
    let mut fleet = super::fleet();
    let blocked = fleet
        .flows
        .iter_mut()
        .find(|flow| flow.name == "shop-221")
        .unwrap();
    blocked.decisions.push("exported_at stays nullable".into());
    blocked.answer = Some(
        "**The migration** adds `orders.exported_at`, but the column has no default. \
         Which default should old orders get?\n\n- `created_at`\n- `null`"
            .into(),
    );
    detail.on_server(ServerMsg::Board(fleet));
    detail.view = View::Board(BoardState {
        selected: 1,
        all: false,
    });
    golden("board-detail", &render_at(&detail, FLEET));

    let mut paused = board_app();
    let mut fleet = super::fleet();
    let waiting = fleet
        .flows
        .iter_mut()
        .find(|flow| flow.name == "shop-230")
        .unwrap();
    waiting.services = Some(crate::view::Services {
        status: crate::event::ServiceStatus::Paused,
        since: waiting.changed_at + 5 * 60_000,
        reason: None,
    });
    let selected = fleet
        .flows
        .iter()
        .position(|flow| flow.name == "shop-230")
        .unwrap();
    paused.on_server(ServerMsg::Board(fleet));
    paused.view = View::Board(BoardState {
        selected: selected + 1,
        all: false,
    });
    golden("board-paused", &render_at(&paused, FLEET));

    let narrow = Size { cols: 96, rows: 30 };
    board.on_resize(narrow);
    golden("narrow-board", &render_at(&board, narrow));

    let mut new_flow = board_app();
    new_flow.set_agents(vec!["claude".into(), "codex".into()]);
    new_flow.on_input(Routed::Command(Command::NewFlow));
    golden("new-flow", &render_at(&new_flow, FLEET));
    new_flow.on_input(Routed::View(vec![Key::Tab]));
    golden("new-flow-agent", &render_at(&new_flow, FLEET));

    let mut stacked = board_app();
    stacked.on_input(Routed::Command(Command::Focus(2)));
    stacked.on_input(Routed::Command(Command::NewFlow));
    stacked.on_input(Routed::View(
        "SHOP-240 export button".chars().map(Key::Char).collect(),
    ));
    stacked.on_input(Routed::View(vec![Key::Tab, Key::Down]));
    golden("new-flow-base", &render_at(&stacked, FLEET));

    let mut new_session = board_app();
    new_session.on_input(Routed::Command(Command::NewFlow));
    new_session.on_input(Routed::View(vec![Key::Tab, Key::Tab, Key::Right]));
    golden("new-flow-plain", &render_at(&new_session, FLEET));

    let mut plain = board_app();
    let mut board = super::fleet();
    board.flows.push(plain_session(
        "scratch",
        "/Users/jan/scratch",
        12,
        vec![EventKind::Idle {
            note: "The tree has 14 files under src.".into(),
        }],
    ));
    board.flows.push(plain_session(
        "notes",
        "/Users/jan/notes",
        200,
        vec![EventKind::Ended],
    ));
    plain.on_server(ServerMsg::Board(fold::board(board.flows, NOW)));
    let digit = crate::ui::sidebar::workers(plain.board.as_ref().unwrap())
        .iter()
        .position(|flow| flow.name == "scratch")
        .unwrap()
        + 1;
    plain.on_input(Routed::Command(Command::Focus(digit as u8)));
    golden("plain", &render_at(&plain, FLEET));
    plain.view = View::Board(BoardState {
        selected: digit,
        all: false,
    });
    golden("board-plain", &render_at(&plain, FLEET));

    let mut help = board_app();
    help.on_input(Routed::Command(Command::Help));
    golden("help", &render_at(&help, FLEET));

    let mut stop = board_app();
    stop.on_input(Routed::Command(Command::StopServer));
    golden("stop", &render_at(&stop, FLEET));
}
