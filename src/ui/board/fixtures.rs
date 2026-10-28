//! Flows and board views the tests of `cards` and `detail` share

use std::collections::HashMap;

use ratatui::buffer::Buffer;
use ratatui::text::Line;

use super::{BoardState, BoardView};
use crate::config::prefix::Prefix;
use crate::event::{Event, EventKind, Mode};
use crate::fold;
use crate::proto::PaneKey;
use crate::ui::sidebar::Sidebar;
use crate::ui::theme::Theme;
use crate::view::{Board, FlowView};

pub(super) const MIN: u64 = 60_000;
pub(super) const NOW: u64 = 30 * MIN;

/// A started flow in `mode` with a worktree, then `events` a minute apart from minute 1.
pub(super) fn flow(name: &str, mode: Mode, events: Vec<EventKind>) -> FlowView {
    let mut all = vec![
        Event::new(
            0,
            EventKind::Started {
                name: name.into(),
                request: format!("{} add CSV export", name.to_uppercase()),
                mode,
                phases: None,
                kind: None,
                workflow: None,
                prompt: None,
                merges: true,
            },
        ),
        Event::new(
            0,
            EventKind::Workspace {
                path: format!("/shop/.saaio/worktrees/{name}"),
                branch: Some(format!("flow/{name}")),
                slot: Some(1),
                window: None,
                agent: "claude".into(),
                base: None,
            },
        ),
    ];
    all.extend(
        events
            .into_iter()
            .enumerate()
            .map(|(step, kind)| Event::new((step as u64 + 1) * MIN, kind)),
    );
    fold::fold(name, &all, None, NOW).unwrap()
}

/// Runs `look` on a board view of `flows`, with the master in ~/shop.
pub(super) fn with_view<R>(flows: Vec<FlowView>, look: impl FnOnce(&BoardView) -> R) -> R {
    with_state(flows, BoardState::default(), look)
}

/// with_view, the board's cursor at `state`.
pub(super) fn with_state<R>(
    flows: Vec<FlowView>,
    state: BoardState,
    look: impl FnOnce(&BoardView) -> R,
) -> R {
    let board: Board = fold::board(flows, NOW);
    let theme = Theme::default();
    let changed = HashMap::new();
    let view = BoardView {
        sidebar: Sidebar {
            board: &board,
            panes: &[],
            focus: &PaneKey::Master,
            cursor: None,
            peek: &[],
            master: Some("~/shop · main"),
            master_state: None,
            now: NOW,
            spin: 0,
            changed: &changed,
            home: None,
            prefix: Prefix::default(),
            theme: &theme,
        },
        state,
        back: "master",
        offset_secs: 0,
        notice: None,
        projects: None,
        here: None,
    };
    look(&view)
}

pub(super) fn text(line: &Line) -> String {
    line.spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect()
}

/// Each row of `buf`, without its trailing blanks.
pub(super) fn rows(buf: &Buffer) -> Vec<String> {
    (0..buf.area.height)
        .map(|y| {
            let row: String = (0..buf.area.width)
                .map(|x| buf[(x, y)].symbol().to_string())
                .collect();
            row.trim_end().to_string()
        })
        .collect()
}
