use super::*;

/// The whole screen as text, compared with src/ui/golden/<name>.txt. FLOW_UPDATE_GOLDEN=1
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
    let update = std::env::var_os("FLOW_UPDATE_GOLDEN").is_some();
    match std::fs::read_to_string(&file) {
        Ok(expected) if !update => assert!(
            expected == text,
            "{name} looks different; if that is meant, run FLOW_UPDATE_GOLDEN=1 cargo test\n{text}"
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

#[test]
fn every_view_looks_as_it_did() {
    let mut alone = App::new(FLEET).with_home(Some("/Users/jan".into()));
    alone.on_server(ServerMsg::Master {
        cwd: "/Users/jan/projects/shop".into(),
        branch: Some("main".into()),
    });
    golden("master-alone", &render_at(&alone, FLEET));
    alone.on_input(Routed::Command(Command::Board));
    golden("empty-board", &render_at(&alone, FLEET));

    let fleet = board_app();
    golden("fleet", &render_at(&fleet, FLEET));

    let mut worker = board_app();
    worker.on_input(Routed::Command(Command::Focus(2)));
    golden("worker", &render_at(&worker, FLEET));

    let mut peek = board_app();
    peek.on_input(Routed::Command(Command::Cursor(Step::Next)));
    golden("peek", &render_at(&peek, FLEET));

    let mut rail = board_app();
    rail.on_input(Routed::Command(Command::Rail));
    golden("rail", &render_at(&rail, FLEET));

    let mut board = board_app();
    board.on_input(Routed::Command(Command::Board));
    golden("board", &render_at(&board, FLEET));
    let narrow = Size { cols: 96, rows: 30 };
    board.on_resize(narrow);
    golden("narrow-board", &render_at(&board, narrow));

    let mut new_flow = board_app().with_agents(vec!["claude".into(), "codex".into()]);
    new_flow.on_input(Routed::Command(Command::NewFlow));
    golden("new-flow", &render_at(&new_flow, FLEET));
    new_flow.on_input(Routed::View(vec![Key::Tab]));
    golden("new-flow-agent", &render_at(&new_flow, FLEET));

    let mut new_session = board_app();
    new_session.on_input(Routed::Command(Command::NewFlow));
    new_session.on_input(Routed::View(vec![Key::Tab, Key::Right]));
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
