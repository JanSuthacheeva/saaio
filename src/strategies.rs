//! The values property tests generate: texts, ids, workflows and every `EventKind`

use std::collections::BTreeSet;

use proptest::prelude::*;
use uuid::Uuid;

use crate::event::{Choice, Event, EventKind, Mode, Question, ServiceStatus};
use crate::id::SessionId;
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
