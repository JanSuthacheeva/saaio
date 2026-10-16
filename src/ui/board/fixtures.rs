use std::collections::HashMap;

use ratatui::buffer::Buffer;
use ratatui::text::Line;

use super::{BoardState, BoardView};
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
            },
        ),
        Event::new(
            0,
            EventKind::Workspace {
                path: format!("/shop/.flow/worktrees/{name}"),
                branch: format!("flow/{name}"),
                slot: 1,
                window: None,
                agent: "claude".into(),
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
            now: NOW,
            spin: 0,
            changed: &changed,
            theme: &theme,
        },
        state: BoardState::default(),
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
