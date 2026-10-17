use crate::event::{Event, EventKind, Mode, Phase};
use crate::fold;
use crate::view::{Board, FlowView};

pub(super) const MIN: u64 = 60_000;

pub(super) fn flow(name: &str, events: Vec<EventKind>, now: u64) -> FlowView {
    let mut all = vec![Event::new(
        0,
        EventKind::Started {
            name: name.into(),
            request: format!("{} add CSV export", name.to_uppercase()),
            mode: Mode::Auto,
            phases: None,
        },
    )];
    all.extend(
        events
            .into_iter()
            .enumerate()
            .map(|(step, kind)| Event::new((step as u64 + 1) * MIN, kind)),
    );
    let mut view = fold::fold(&format!("{name}-id"), &all, None, now).unwrap();
    view.id = format!("{name}-id");
    view
}

pub(super) fn sample_board(now: u64) -> Board {
    fold::board(
        vec![
            flow(
                "shop-221",
                vec![
                    EventKind::Phase {
                        phase: Phase::of("implement"),
                    },
                    EventKind::Tasks { done: 4, total: 9 },
                    EventKind::Blocked {
                        reason: "Task 5: migration needs a default value".into(),
                    },
                ],
                now,
            ),
            flow(
                "shop-230",
                vec![EventKind::Phase {
                    phase: Phase::of("design"),
                }],
                now,
            ),
            flow(
                "export-csv",
                vec![EventKind::MergeRequest {
                    url: "https://x/88".into(),
                }],
                now,
            ),
        ],
        now,
    )
}
