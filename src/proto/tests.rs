use std::io::Cursor;
use std::path::Path;

use super::*;
use crate::event::{Event, EventKind, Mode};
use crate::fixtures;
use crate::fold;
use crate::view::{FlowView, VIEW_VERSION};

#[test]
fn a_printable_peer_has_no_control_characters() {
    let peer = Peer {
        pid: 7,
        cwd: "/tmp/\x1b]52;c;aGk=\x07x".into(),
        build: "0.1.0\u{9b}2J".into(),
    };

    let clean = peer.printable();

    assert_eq!(clean.pid, 7);
    assert_eq!(clean.cwd, Path::new("/tmp/]52;c;aGk=x"));
    assert_eq!(clean.build, "0.1.02J");
}

#[test]
fn a_printable_peer_keeps_at_most_4096_chars_of_each_text() {
    let peer = Peer {
        pid: 7,
        cwd: "/".to_string().repeat(5000).into(),
        build: "界".repeat(5000),
    };

    let clean = peer.printable();

    assert_eq!(clean.cwd.as_os_str().len(), 4096);
    assert_eq!(clean.build.chars().count(), 4096);
}

/// A flow whose harness picked its own session, as a Codex worker's does.
fn flow_with_session() -> FlowView {
    let events = [
        Event::new(
            1,
            EventKind::Started {
                name: "csv".into(),
                request: "add csv".into(),
                mode: Mode::Guided,
                phases: None,
                kind: None,
                workflow: None,
                prompt: None,
                merges: true,
            },
        ),
        Event::new(
            2,
            EventKind::Session {
                id: fixtures::session_id("01a1"),
                transcript: Some("/home/dev/.codex/sessions/rollout-01a1.jsonl".into()),
            },
        ),
    ];
    fold::fold(fixtures::flow_id("7f3c"), &events, None, 3).unwrap()
}

/// Hands out one byte per read, like a socket under load.
struct Trickle(Cursor<Vec<u8>>);

impl Read for Trickle {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let end = buf.len().min(1);
        self.0.read(&mut buf[..end])
    }
}

fn cell(text: &str) -> Cell {
    Cell {
        text: text.into(),
        fg: Color::Indexed(4),
        bg: Color::Rgb(1, 2, 3),
        flags: Flags::BOLD | Flags::WIDE,
    }
}

fn peer() -> Peer {
    Peer {
        pid: 4321,
        cwd: "/Users/jan/projects/flow".into(),
        build: "0.1.0 2904734".into(),
    }
}

fn client_messages() -> Vec<ClientMsg> {
    let size = Size {
        cols: 120,
        rows: 40,
    };
    vec![
        ClientMsg::Hello {
            version: PROTOCOL_VERSION,
            role: Role::Attach { size },
            peer: peer(),
        },
        ClientMsg::Hello {
            version: PROTOCOL_VERSION,
            role: Role::Control,
            peer: peer(),
        },
        ClientMsg::Focus(PaneKey::Flow(fixtures::flow_id("7f3c"))),
        ClientMsg::Input(vec![0, 27, b'[', b'A']),
        ClientMsg::Resize(size),
        ClientMsg::History(Some(12)),
        ClientMsg::History(None),
        ClientMsg::Reopen(PaneKey::Master),
        ClientMsg::CloseFlow(fixtures::flow_id("7f3c")),
        ClientMsg::StopServer,
        ClientMsg::EndFlows,
    ]
}

fn server_messages() -> Vec<ServerMsg> {
    vec![
        ServerMsg::Welcome {
            version: PROTOCOL_VERSION,
            server: Binary {
                exe: "/home/jan/.local/bin/saaio".into(),
                build: "0.2.0 (abc1234)".into(),
            },
        },
        ServerMsg::Refused {
            reason: "protocol 2".into(),
        },
        ServerMsg::Board(Board {
            version: VIEW_VERSION,
            now: 5,
            count: 0,
            needs_you: 0,
            flows: vec![],
        }),
        ServerMsg::Board(Board {
            version: VIEW_VERSION,
            now: 5,
            count: 1,
            needs_you: 0,
            flows: vec![flow_with_session()],
        }),
        ServerMsg::Panes(vec![
            PaneInfo {
                key: PaneKey::Master,
                status: PaneStatus::Running,
            },
            PaneInfo {
                key: PaneKey::Flow(fixtures::flow_id("a")),
                status: PaneStatus::Exited { code: Some(1) },
            },
            PaneInfo {
                key: PaneKey::Flow(fixtures::flow_id("b")),
                status: PaneStatus::Failed {
                    reason: "no agent".into(),
                },
            },
        ]),
        ServerMsg::Frame(Frame {
            key: PaneKey::Master,
            full: true,
            rows: vec![(0, vec![cell("a"), cell("界")])],
            cursor: Some((3, 0)),
            modes: Modes {
                paste: true,
                mouse: Mouse::Drag,
                sgr_mouse: true,
                app_cursor: false,
                focus: true,
                alt_screen: true,
            },
            history: None,
        }),
        ServerMsg::Closed(fixtures::flow_id("a")),
        ServerMsg::Detached { by: peer() },
        ServerMsg::NotOpen(fixtures::flow_id("b")),
        ServerMsg::Master {
            cwd: "/shop".into(),
            branch: Some("main".into()),
            turn: Some(MasterTurn {
                state: crate::view::State::Waiting,
                at: 7,
            }),
        },
        ServerMsg::Clipboard("copied".into()),
    ]
}

#[test]
fn every_message_survives_a_round_trip() {
    let mut wire = Vec::new();
    for message in client_messages() {
        write_msg(&mut wire, &message).unwrap();
    }
    let mut reader = Cursor::new(wire);
    for message in client_messages() {
        assert_eq!(
            read_msg::<_, ClientMsg>(&mut reader).unwrap(),
            Some(message)
        );
    }
    assert_eq!(read_msg::<_, ClientMsg>(&mut reader).unwrap(), None);

    let mut wire = Vec::new();
    for message in server_messages() {
        write_msg(&mut wire, &message).unwrap();
    }
    let mut reader = Trickle(Cursor::new(wire));
    for message in server_messages() {
        assert_eq!(
            read_msg::<_, ServerMsg>(&mut reader).unwrap(),
            Some(message)
        );
    }
    assert_eq!(read_msg::<_, ServerMsg>(&mut reader).unwrap(), None);
}

#[test]
fn an_older_server_still_reads_the_version_of_a_newer_hello() {
    #[derive(Deserialize)]
    enum OlderClientMsg {
        Hello { version: u32, role: Role },
    }
    let mut wire = Vec::new();
    write_msg(
        &mut wire,
        &ClientMsg::Hello {
            version: PROTOCOL_VERSION,
            role: Role::Control,
            peer: peer(),
        },
    )
    .unwrap();

    let Some(OlderClientMsg::Hello { version, role }) = read_msg(&mut Cursor::new(wire)).unwrap()
    else {
        panic!("no hello");
    };
    assert_eq!((version, role), (PROTOCOL_VERSION, Role::Control));
}

#[test]
fn a_connection_says_hello_once_and_control_only_closes_a_flow() {
    let size = Size { cols: 1, rows: 1 };
    let hello = client_messages().remove(0);
    let close = ClientMsg::CloseFlow(fixtures::flow_id("7f3c"));

    assert!(allowed(None, &hello));
    assert!(!allowed(None, &close));
    assert!(!allowed(None, &ClientMsg::StopServer));
    for role in [Role::Attach { size }, Role::Control] {
        assert!(!allowed(Some(role), &hello), "{role:?}");
        assert!(allowed(Some(role), &close), "{role:?}");
    }
    for message in client_messages().into_iter().skip(2) {
        assert!(
            allowed(Some(Role::Attach { size }), &message),
            "{message:?}"
        );
        let control = matches!(message, ClientMsg::CloseFlow(_));
        assert_eq!(
            allowed(Some(Role::Control), &message),
            control,
            "{message:?}"
        );
    }
}

#[test]
fn writes_a_flow_id_as_the_text_it_was_before() {
    #[derive(Serialize)]
    enum TextKey {
        #[expect(dead_code, reason = "only the variant order matters")]
        Master,
        Flow(String),
    }
    let id = fixtures::flow_id("7f3c");
    let text = postcard::to_stdvec(&id.to_string()).unwrap();

    let close = postcard::to_stdvec(&ClientMsg::CloseFlow(id)).unwrap();
    assert_eq!(close[1..], text[..]);
    assert_eq!(
        postcard::to_stdvec(&PaneKey::Flow(id)).unwrap(),
        postcard::to_stdvec(&TextKey::Flow(id.to_string())).unwrap()
    );

    let mut forged = close[..1].to_vec();
    forged.extend(postcard::to_stdvec("../7f3c").unwrap());
    assert!(postcard::from_bytes::<ClientMsg>(&forged).is_err());
}

#[test]
fn a_clamped_size_is_at_least_one_cell_and_at_most_the_largest() {
    let size = |cols, rows| Size { cols, rows };

    assert_eq!(size(0, 0).clamped(), size(1, 1));
    assert_eq!(size(80, 24).clamped(), size(80, 24));
    assert_eq!(size(u16::MAX, u16::MAX).clamped(), MAX_SIZE);
}

#[test]
fn knows_the_uid_at_the_other_end_of_a_socket() {
    let (ours, _theirs) = UnixStream::pair().unwrap();

    assert_eq!(peer_uid(&ours).unwrap(), rustix::process::getuid().as_raw());
}

#[test]
fn names_a_peer_by_pid_folder_and_build() {
    assert_eq!(
        peer().to_string(),
        "pid 4321 in /Users/jan/projects/flow (saaio 0.1.0 2904734)"
    );
    let lost = Peer {
        cwd: PathBuf::new(),
        ..peer()
    };
    assert_eq!(
        lost.to_string(),
        "pid 4321 in an unknown folder (saaio 0.1.0 2904734)"
    );
}

#[test]
fn a_stream_cut_inside_a_message_is_an_error() {
    let mut wire = Vec::new();
    write_msg(&mut wire, &ClientMsg::CloseFlow(fixtures::flow_id("abc"))).unwrap();
    wire.truncate(wire.len() - 1);

    let inside = read_msg::<_, ClientMsg>(&mut Cursor::new(wire)).unwrap_err();
    let length = read_msg::<_, ClientMsg>(&mut Cursor::new(vec![9, 0])).unwrap_err();

    assert!(matches!(inside, Error::Cut), "{inside:?}");
    assert!(matches!(length, Error::Cut), "{length:?}");
    assert_eq!(inside.to_string(), "the stream ended inside a message");
}

/// Ends after `left` bytes, remembering the largest buffer it was handed.
struct Short {
    left: usize,
    largest: usize,
}

impl Read for Short {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.largest = self.largest.max(buf.len());
        let read = buf.len().min(self.left);
        buf[..read].fill(0);
        self.left -= read;
        Ok(read)
    }
}

#[test]
fn a_length_alone_allocates_nothing_near_it() {
    let mut wire = Cursor::new(((MAX_MESSAGE - 1) as u32).to_le_bytes().to_vec()).chain(Short {
        left: 10,
        largest: 0,
    });

    assert!(read_msg::<_, ClientMsg>(&mut wire).is_err());
    let (_, short) = wire.into_inner();
    assert!(short.largest <= 64 << 10, "{}", short.largest);
}

#[test]
fn random_bytes_decode_or_fail_but_never_panic() {
    let mut noise = crate::fixtures::Noise::new(0x5aa10);
    let seeds: Vec<Vec<u8>> = client_messages()
        .iter()
        .map(|message| postcard::to_stdvec(message).unwrap())
        .collect();
    for case in 0..2000 {
        let bytes = match case % 3 {
            0 => noise.bytes(64),
            _ => noise.mutate(&seeds[case % seeds.len()]),
        };
        let mut wire = u32::try_from(bytes.len()).unwrap().to_le_bytes().to_vec();
        wire.extend(&bytes);
        let _ = read_msg::<_, ClientMsg>(&mut Cursor::new(&wire));
        let _ = read_msg::<_, ClientMsg>(&mut Cursor::new(&bytes));
    }
}

#[test]
fn refuses_oversized_and_undecodable_messages() {
    let oversized = ((MAX_MESSAGE + 1) as u32).to_le_bytes().to_vec();
    let error = read_msg::<_, ClientMsg>(&mut Cursor::new(oversized)).unwrap_err();
    assert!(
        matches!(error, Error::TooLarge(len) if len == MAX_MESSAGE + 1),
        "{error:?}"
    );
    assert_eq!(
        error.to_string(),
        format!("message of {} bytes is over the limit", MAX_MESSAGE + 1)
    );

    let mut garbage = 3u32.to_le_bytes().to_vec();
    garbage.extend([0xff, 0xff, 0xff]);
    let error = read_msg::<_, ClientMsg>(&mut Cursor::new(garbage)).unwrap_err();
    assert!(matches!(error, Error::Decode(_)), "{error:?}");
    assert_eq!(error.to_string(), "could not decode a message");

    let huge = ClientMsg::Input(vec![0; MAX_MESSAGE + 1]);
    let error = write_msg(&mut Vec::new(), &huge).unwrap_err();
    assert!(matches!(error, Error::TooLarge(_)), "{error:?}");
}

#[test]
fn only_a_passed_deadline_is_a_time_out() {
    let quiet = [ErrorKind::WouldBlock, ErrorKind::TimedOut];
    let broken = [ErrorKind::BrokenPipe, ErrorKind::ConnectionReset];

    for kind in quiet {
        assert!(Error::from(io::Error::from(kind)).timed_out(), "{kind:?}");
    }
    for kind in broken {
        assert!(!Error::from(io::Error::from(kind)).timed_out(), "{kind:?}");
    }
    assert!(!Error::Cut.timed_out());
    assert!(!Error::TooLarge(1).timed_out());
}
