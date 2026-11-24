//! The values property tests generate: texts, ids, workflows, every `EventKind` and every
//! message between the UI and the server

use std::collections::BTreeSet;

use proptest::prelude::*;
use serde_json::{Map, Number, Value};
use uuid::Uuid;

use crate::binaries::Binary;
use crate::event::{Choice, Event, EventKind, Mode, Question, ServiceStatus};
use crate::fold;
use crate::id::{FlowId, SessionId};
use crate::proto::{
    Cell, ClientMsg, Color, Flags, Frame, Modes, Mouse, PaneInfo, PaneKey, PaneStatus, Peer, Role,
    ServerMsg, Size,
};
use crate::view::{Board, MasterTurn, State};
use crate::workflow::{Phase, PhaseSpec, Workflow};

/// Any text, control characters, quotes, newlines and non-ASCII included, up to 60 chars.
pub fn text() -> impl Strategy<Value = String> {
    let tricky = prop::sample::select(vec![
        '\n',
        '\r',
        '\t',
        '\0',
        '\u{1b}',
        '\u{7f}',
        '\u{85}',
        '\u{2028}',
        '"',
        '\\',
        '{',
        '}',
        'é',
        '\u{1f600}',
    ]);
    let char = prop_oneof![3 => any::<char>(), 1 => tricky, 4 => prop::char::range(' ', '~')];
    prop::collection::vec(char, 0..60).prop_map(String::from_iter)
}

fn texts() -> impl Strategy<Value = Option<String>> {
    prop::option::of(text())
}

/// A valid phase name.
pub fn phase() -> impl Strategy<Value = Phase> {
    "[a-z0-9-]{1,12}".prop_map(|name| Phase::of(&name))
}

/// A valid workflow: 1 to 4 phases of distinct names, no skill starting two.
pub fn workflow() -> impl Strategy<Value = Workflow> {
    prop::collection::btree_set("[a-z0-9-]{1,12}", 1..=4)
        .prop_flat_map(|names| {
            let skills = prop::collection::vec(
                prop::collection::btree_set("[a-z-]{1,10}", 0..=2),
                names.len(),
            );
            (Just(names), skills)
        })
        .prop_map(|(names, skills)| {
            let mut used = BTreeSet::new();
            let specs = names
                .into_iter()
                .zip(skills)
                .map(|(name, skills)| PhaseSpec {
                    name: Phase::of(&name),
                    skills: skills
                        .into_iter()
                        .filter(|skill| used.insert(skill.clone()))
                        .collect(),
                })
                .collect::<Vec<_>>();
            Workflow::try_from(specs).expect("distinct names and skills")
        })
}

fn uuid() -> impl Strategy<Value = String> {
    any::<[u8; 16]>().prop_map(|bytes| Uuid::from_bytes(bytes).to_string())
}

pub fn flow_id() -> impl Strategy<Value = FlowId> {
    uuid().prop_map(|text| FlowId::parse(&text).expect("a uuid's text"))
}

pub fn session_id() -> impl Strategy<Value = SessionId> {
    uuid().prop_map(|text| SessionId::parse(&text).expect("a uuid's text"))
}

fn mode() -> impl Strategy<Value = Mode> {
    prop::sample::select(vec![Mode::Guided, Mode::Auto, Mode::Plain])
}

fn service_status() -> impl Strategy<Value = ServiceStatus> {
    prop::sample::select(vec![
        ServiceStatus::Pausing,
        ServiceStatus::Paused,
        ServiceStatus::Resuming,
        ServiceStatus::Running,
        ServiceStatus::PauseFailed,
        ServiceStatus::ResumeFailed,
    ])
}

fn question() -> impl Strategy<Value = Question> {
    let choice = (text(), text()).prop_map(|(label, description)| Choice { label, description });
    (
        text(),
        text(),
        prop::collection::vec(choice, 0..10),
        any::<bool>(),
    )
        .prop_map(|(header, question, options, multi_select)| Question {
            header,
            question,
            options,
            multi_select,
        })
}

/// Every EventKind variant, fields at any value.
pub fn event_kind() -> impl Strategy<Value = EventKind> {
    let started = (
        (text(), text(), mode()),
        prop::option::of(prop::collection::vec(phase(), 0..5)),
        texts(),
        prop::option::of(workflow()),
        texts(),
        any::<bool>(),
    )
        .prop_map(
            |((name, request, mode), phases, kind, workflow, prompt, merges)| EventKind::Started {
                name,
                request,
                mode,
                phases,
                kind,
                workflow,
                prompt,
                merges,
            },
        );
    let waiting = (
        text(),
        prop::collection::vec(question(), 0..10),
        any::<bool>(),
    )
        .prop_map(|(question, questions, permission)| EventKind::Waiting {
            question,
            questions,
            permission,
        });
    let workspace = (
        (text(), texts(), any::<Option<u8>>()),
        (texts(), text(), texts()),
    )
        .prop_map(
            |((path, branch, slot), (window, agent, base))| EventKind::Workspace {
                path,
                branch,
                slot,
                window,
                agent,
                base,
            },
        );
    prop_oneof![
        started.boxed(),
        phase().prop_map(|phase| EventKind::Phase { phase }).boxed(),
        Just(EventKind::Working).boxed(),
        text()
            .prop_map(|description| EventKind::Delegated { description })
            .boxed(),
        waiting.boxed(),
        text()
            .prop_map(|question| EventKind::PermissionPending { question })
            .boxed(),
        text().prop_map(|note| EventKind::Idle { note }).boxed(),
        text()
            .prop_map(|reason| EventKind::Blocked { reason })
            .boxed(),
        text().prop_map(|text| EventKind::Decision { text }).boxed(),
        any::<(u32, u32)>()
            .prop_map(|(done, total)| EventKind::Tasks { done, total })
            .boxed(),
        any::<(u32, u32)>()
            .prop_map(|(fixed, open)| EventKind::Review { fixed, open })
            .boxed(),
        text()
            .prop_map(|url| EventKind::MergeRequest { url })
            .boxed(),
        (text(), texts())
            .prop_map(|(url, into)| EventKind::Merged { url, into })
            .boxed(),
        text()
            .prop_map(|url| EventKind::MergeRequestClosed { url })
            .boxed(),
        text().prop_map(|text| EventKind::Message { text }).boxed(),
        text().prop_map(|text| EventKind::Typed { text }).boxed(),
        text().prop_map(|text| EventKind::Answer { text }).boxed(),
        Just(EventKind::Ended).boxed(),
        (session_id(), texts())
            .prop_map(|(id, transcript)| EventKind::Session { id, transcript })
            .boxed(),
        workspace.boxed(),
        (text(), text())
            .prop_map(|(base, onto)| EventKind::BaseMerged { base, onto })
            .boxed(),
        (service_status(), texts())
            .prop_map(|(status, reason)| EventKind::Services { status, reason })
            .boxed(),
        (text(), text())
            .prop_map(|(command, holder)| EventKind::TurnWaiting { command, holder })
            .boxed(),
        Just(EventKind::TurnTaken).boxed(),
        Just(EventKind::Prompted).boxed(),
        text().prop_map(|path| EventKind::Edited { path }).boxed(),
    ]
}

/// Event::new(at, kind): an event as saaio writes it, texts cleaned.
pub fn event() -> impl Strategy<Value = Event> {
    (any::<u64>(), event_kind()).prop_map(|(at, kind)| Event::new(at, kind))
}

fn size() -> impl Strategy<Value = Size> {
    any::<(u16, u16)>().prop_map(|(cols, rows)| Size { cols, rows })
}

fn pane_key() -> impl Strategy<Value = PaneKey> {
    prop_oneof![Just(PaneKey::Master), flow_id().prop_map(PaneKey::Flow)]
}

fn peer() -> impl Strategy<Value = Peer> {
    (any::<u32>(), text(), text()).prop_map(|(pid, cwd, build)| Peer {
        pid,
        cwd: cwd.into(),
        build,
    })
}

/// Every ClientMsg variant, fields at any value.
pub fn client_msg() -> impl Strategy<Value = ClientMsg> {
    let role = prop_oneof![
        size().prop_map(|size| Role::Attach { size }),
        Just(Role::Control),
    ];
    prop_oneof![
        (any::<u32>(), role, peer())
            .prop_map(|(version, role, peer)| ClientMsg::Hello {
                version,
                role,
                peer
            })
            .boxed(),
        pane_key().prop_map(ClientMsg::Focus).boxed(),
        prop::collection::vec(any::<u8>(), 0..32)
            .prop_map(ClientMsg::Input)
            .boxed(),
        size().prop_map(ClientMsg::Resize).boxed(),
        any::<Option<u32>>().prop_map(ClientMsg::History).boxed(),
        pane_key().prop_map(ClientMsg::Reopen).boxed(),
        flow_id().prop_map(ClientMsg::CloseFlow).boxed(),
        Just(ClientMsg::StopServer).boxed(),
        Just(ClientMsg::EndFlows).boxed(),
    ]
}

/// Up to three flows, each folded from a Started event and up to eight event_kind()s, without
/// the fields the wire leaves to the server (`#[serde(skip)]`).
fn board() -> impl Strategy<Value = Board> {
    let flow = (
        flow_id(),
        (text(), text(), mode()),
        prop::collection::vec((any::<u64>(), event_kind()), 0..8),
        any::<u64>(),
    )
        .prop_map(|(id, (name, request, mode), events, now)| {
            let started = EventKind::Started {
                name,
                request,
                mode,
                phases: None,
                kind: None,
                workflow: None,
                prompt: None,
                merges: true,
            };
            let events = std::iter::once(Event::new(0, started))
                .chain(events.into_iter().map(|(at, kind)| Event::new(at, kind)))
                .collect::<Vec<_>>();
            let mut flow = fold::fold(id, &events, None, now).expect("a started flow folds");
            flow.workflow = None;
            flow.prompt = None;
            flow.merged_into = None;
            flow
        });
    (prop::collection::vec(flow, 0..=3), any::<u64>())
        .prop_map(|(flows, now)| fold::board(flows, now))
}

fn color() -> impl Strategy<Value = Color> {
    prop_oneof![
        Just(Color::Default),
        any::<u8>().prop_map(Color::Indexed),
        any::<(u8, u8, u8)>().prop_map(|(r, g, b)| Color::Rgb(r, g, b)),
    ]
}

fn flags() -> impl Strategy<Value = Flags> {
    let all = [
        Flags::BOLD,
        Flags::DIM,
        Flags::ITALIC,
        Flags::UNDERLINE,
        Flags::REVERSE,
        Flags::STRIKE,
        Flags::WIDE,
        Flags::SPACER,
    ];
    prop::sample::subsequence(all.to_vec(), 0..=all.len()).prop_map(|flags| {
        flags
            .into_iter()
            .fold(Flags::default(), |all, flag| all | flag)
    })
}

fn frame() -> impl Strategy<Value = Frame> {
    let cell = (text(), color(), color(), flags()).prop_map(|(text, fg, bg, flags)| Cell {
        text,
        fg,
        bg,
        flags,
    });
    let row = (any::<u16>(), prop::collection::vec(cell, 0..4));
    let mouse = prop::sample::select(vec![Mouse::Off, Mouse::Press, Mouse::Drag, Mouse::Motion]);
    let modes = (any::<[bool; 5]>(), mouse).prop_map(
        |([paste, sgr_mouse, app_cursor, focus, alt_screen], mouse)| Modes {
            paste,
            mouse,
            sgr_mouse,
            app_cursor,
            focus,
            alt_screen,
        },
    );
    (
        pane_key(),
        any::<bool>(),
        prop::collection::vec(row, 0..4),
        any::<Option<(u16, u16)>>(),
        modes,
        any::<Option<u32>>(),
    )
        .prop_map(|(key, full, rows, cursor, modes, history)| Frame {
            key,
            full,
            rows,
            cursor,
            modes,
            history,
        })
}

fn master_turn() -> impl Strategy<Value = MasterTurn> {
    let state = prop::sample::select(vec![
        State::Working,
        State::Delegated,
        State::Waiting,
        State::Idle,
        State::Blocked,
        State::NotResponding,
        State::Done,
        State::Ended,
    ]);
    (state, any::<u64>()).prop_map(|(state, at)| MasterTurn { state, at })
}

/// Every ServerMsg variant, fields at any value.
pub fn server_msg() -> impl Strategy<Value = ServerMsg> {
    let status = prop_oneof![
        Just(PaneStatus::Running),
        any::<Option<u32>>().prop_map(|code| PaneStatus::Exited { code }),
        text().prop_map(|reason| PaneStatus::Failed { reason }),
    ];
    let pane = (pane_key(), status).prop_map(|(key, status)| PaneInfo { key, status });
    prop_oneof![
        (any::<u32>(), text(), text())
            .prop_map(|(version, exe, build)| ServerMsg::Welcome {
                version,
                server: Binary {
                    exe: exe.into(),
                    build
                },
            })
            .boxed(),
        text()
            .prop_map(|reason| ServerMsg::Refused { reason })
            .boxed(),
        board().prop_map(ServerMsg::Board).boxed(),
        prop::collection::vec(pane, 0..4)
            .prop_map(ServerMsg::Panes)
            .boxed(),
        frame().prop_map(ServerMsg::Frame).boxed(),
        flow_id().prop_map(ServerMsg::Closed).boxed(),
        peer().prop_map(|by| ServerMsg::Detached { by }).boxed(),
        flow_id().prop_map(ServerMsg::NotOpen).boxed(),
        (text(), texts(), prop::option::of(master_turn()))
            .prop_map(|(cwd, branch, turn)| ServerMsg::Master { cwd, branch, turn })
            .boxed(),
        text().prop_map(ServerMsg::Clipboard).boxed(),
    ]
}

/// Any JSON value, up to depth 3.
pub fn json() -> impl Strategy<Value = Value> {
    let leaf = prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::Bool),
        any::<i64>().prop_map(|number| Value::Number(number.into())),
        any::<f64>().prop_map(|number| Number::from_f64(number).map_or(Value::Null, Value::Number)),
        text().prop_map(Value::String),
    ];
    leaf.prop_recursive(3, 32, 4, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..4).prop_map(Value::Array),
            prop::collection::vec((text(), inner), 0..4)
                .prop_map(|fields| Value::Object(fields.into_iter().collect::<Map<_, _>>())),
        ]
    })
}

/// The JSON pointer of every value inside `value`, nested ones included.
fn pointers(value: &Value, at: &str, all: &mut Vec<String>) {
    let children: Vec<(String, &Value)> = match value {
        Value::Object(fields) => fields
            .iter()
            .map(|(key, child)| (key.replace('~', "~0").replace('/', "~1"), child))
            .collect(),
        Value::Array(items) => items
            .iter()
            .enumerate()
            .map(|(index, child)| (index.to_string(), child))
            .collect(),
        _ => Vec::new(),
    };
    for (key, child) in children {
        let pointer = format!("{at}/{key}");
        all.push(pointer.clone());
        pointers(child, &pointer, all);
    }
}

/// A recorded payload from src/adapter/fixtures/<harness> with up to three of its fields
/// replaced by any JSON value, nested ones included.
pub fn hook_payload(harness: &str) -> impl Strategy<Value = String> {
    let folder = format!(
        "{}/src/adapter/fixtures/{harness}",
        env!("CARGO_MANIFEST_DIR")
    );
    let mut payloads = std::fs::read_dir(&folder)
        .expect("the recorded payloads")
        .map(|entry| entry.expect("a recorded payload").path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .map(|path| {
            let text = std::fs::read_to_string(&path).expect("a recorded payload");
            serde_json::from_str::<Value>(&text).expect("a recorded payload is JSON")
        })
        .collect::<Vec<_>>();
    payloads.sort_by_key(Value::to_string);
    let swaps = prop::collection::vec((any::<prop::sample::Index>(), json()), 0..=3);
    (prop::sample::select(payloads), swaps).prop_map(|(mut payload, swaps)| {
        for (at, value) in swaps {
            let mut all = Vec::new();
            pointers(&payload, "", &mut all);
            if all.is_empty() {
                break;
            }
            let pointer = &all[at.index(all.len())];
            *payload
                .pointer_mut(pointer)
                .expect("a pointer into the payload") = value;
        }
        payload.to_string()
    })
}

/// How many EventKind variants there are; `variant` numbers them.
pub const VARIANTS: usize = 26;

/// The variant's position, by a match without a wildcard: a new variant fails to compile here
/// until event_kind() makes it.
pub fn variant(kind: &EventKind) -> usize {
    match kind {
        EventKind::Started { .. } => 0,
        EventKind::Phase { .. } => 1,
        EventKind::Working => 2,
        EventKind::Delegated { .. } => 3,
        EventKind::Waiting { .. } => 4,
        EventKind::PermissionPending { .. } => 5,
        EventKind::Idle { .. } => 6,
        EventKind::Blocked { .. } => 7,
        EventKind::Decision { .. } => 8,
        EventKind::Tasks { .. } => 9,
        EventKind::Review { .. } => 10,
        EventKind::MergeRequest { .. } => 11,
        EventKind::Merged { .. } => 12,
        EventKind::MergeRequestClosed { .. } => 13,
        EventKind::Message { .. } => 14,
        EventKind::Typed { .. } => 15,
        EventKind::Answer { .. } => 16,
        EventKind::Ended => 17,
        EventKind::Session { .. } => 18,
        EventKind::Workspace { .. } => 19,
        EventKind::BaseMerged { .. } => 20,
        EventKind::Services { .. } => 21,
        EventKind::TurnWaiting { .. } => 22,
        EventKind::TurnTaken => 23,
        EventKind::Prompted => 24,
        EventKind::Edited { .. } => 25,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use proptest::strategy::ValueTree;
    use proptest::test_runner::TestRunner;

    use super::*;

    #[test]
    fn the_strategy_makes_every_kind() {
        let mut runner = TestRunner::deterministic();
        let kinds = event_kind();
        let seen = (0..512)
            .map(|_| variant(&kinds.new_tree(&mut runner).unwrap().current()))
            .collect::<BTreeSet<_>>();

        assert_eq!(seen, (0..VARIANTS).collect());
    }
}
