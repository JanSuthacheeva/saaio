mod labels;
mod rows;

pub use labels::last_note;

use std::collections::HashMap;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

use crate::proto::{PaneInfo, PaneKey, PaneStatus};
use crate::ui::layout::SIDEBAR_WIDTH;
use crate::ui::peek::PeekLine;
use crate::ui::theme::Theme;
use crate::view::{Board, FlowView, Group};

const WIDTH: usize = SIDEBAR_WIDTH as usize;
/// Rows the key hint takes at the bottom.
const FOOTER_ROWS: u16 = 1;
/// The most rows a peek takes, with its rule and its hint.
const PEEK_ROWS: usize = 14;
/// Fewer rows than this and the peek is left out.
const PEEK_MIN_ROWS: usize = 4;

/// The right-hand overview: the master, then every worker grouped by what it needs.
#[derive(Debug)]
pub struct Sidebar<'a> {
    pub board: &'a Board,
    pub panes: &'a [PaneInfo],
    pub focus: &'a PaneKey,
    /// The flow whose peek shows (C-space j/k).
    pub cursor: Option<&'a str>,
    pub peek: &'a [PeekLine],
    /// Where the master runs: ~/projects/shop · main.
    pub master: Option<&'a str>,
    pub now: u64,
    pub spin: usize,
    /// Flows whose state changed a moment ago, by id; their rows stand out.
    pub changed: &'a HashMap<String, u64>,
    pub theme: &'a Theme,
}

/// One row of the sidebar or the rail and the pane a click on it focuses.
#[derive(Debug)]
pub(super) struct Row {
    pub line: Line<'static>,
    pub key: Option<PaneKey>,
    pub selected: bool,
}

/// The flows that get a pane, in sidebar order (needs you, ready, running, done); digit n focuses the nth.
pub fn workers(board: &Board) -> Vec<&FlowView> {
    board
        .flows
        .iter()
        .filter(|flow| flow.workspace.is_some())
        .collect()
}

impl Sidebar<'_> {
    /// The pane a click on this row of the sidebar focuses.
    pub fn key_at(&self, row: u16, height: u16) -> Option<PaneKey> {
        self.rows(height).into_iter().nth(row as usize)?.key
    }

    fn rows(&self, height: u16) -> Vec<Row> {
        let room = usize::from(height.saturating_sub(FOOTER_ROWS));
        let mut rows = self.master_rows();
        let listed = workers(self.board);
        let mut flows = self.flow_blocks(&listed);
        let used = rows.len() + flows.iter().map(Vec::len).sum::<usize>();
        let budget = room.saturating_sub(used).min(PEEK_ROWS);
        let peeked = self
            .cursor
            .and_then(|id| listed.iter().position(|flow| flow.id == id));
        if let Some(index) = peeked
            && budget >= PEEK_MIN_ROWS
        {
            self.insert_peek(&mut flows[index], listed[index], budget);
        }
        self.fit(&mut rows, flows, room);
        rows.extend(self.footer());
        rows
    }

    /// One block of rows per worker, each group's first led by its heading and count.
    fn flow_blocks(&self, listed: &[&FlowView]) -> Vec<Vec<Row>> {
        let theme = self.theme;
        let mut group = None;
        let mut flows = Vec::new();
        for (index, flow) in listed.iter().copied().enumerate() {
            let heading = flow.group();
            let mut block = Vec::new();
            if group != Some(heading) {
                group = Some(heading);
                let count = listed
                    .iter()
                    .filter(|other| other.group() == heading)
                    .count();
                let tone = if heading == Group::NeedsYou {
                    theme.state.waiting
                } else {
                    theme.ui.faint
                };
                block.push(plain(Line::from(vec![
                    Span::styled(format!(" {} ", heading.label()), theme.ui.faint),
                    Span::styled(count.to_string(), tone),
                ])));
            }
            block.extend(self.flow_rows(flow, index));
            block.push(plain(Line::raw("")));
            flows.push(block);
        }
        flows
    }

    /// As many whole flow blocks as fit in `room` rows, "N more" for the rest, padded to `room`.
    fn fit(&self, rows: &mut Vec<Row>, flows: Vec<Vec<Row>>, room: usize) {
        let mut used = rows.len();
        let shown = flows
            .iter()
            .take_while(|block| {
                used += block.len();
                used <= room
            })
            .count();
        let hidden = flows.len() - shown;
        rows.extend(flows.into_iter().take(shown).flatten());
        if hidden > 0 && rows.len() < room {
            rows.push(plain(Line::styled(
                format!(" {} {hidden} more", self.theme.glyphs.more),
                self.theme.ui.faint,
            )));
        }
        rows.truncate(room);
        rows.resize_with(room, || plain(Line::raw("")));
    }

    fn status(&self, flow: &FlowView) -> Option<&PaneStatus> {
        self.panes
            .iter()
            .find(|pane| matches!(&pane.key, PaneKey::Flow(id) if *id == flow.id))
            .map(|pane| &pane.status)
    }

    /// One quiet line at the foot: the prefix and the key that lists the rest.
    fn footer(&self) -> Vec<Row> {
        let theme = self.theme;
        vec![plain(Line::from(vec![
            Span::raw(" "),
            Span::styled("C-space", theme.ui.key),
            Span::raw(" "),
            Span::styled("?", theme.ui.key),
            Span::styled(" keys", theme.ui.faint),
        ]))]
    }
}

impl Widget for Sidebar<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        for (row, line) in self.rows(area.height).into_iter().zip(area.rows()) {
            if row.selected {
                buf.set_style(line, self.theme.ui.selection);
            }
            buf.set_line(line.x, line.y, &row.line, line.width);
        }
    }
}

/// The digit that focuses the worker at `index` among them: 1-9, then none.
pub fn digit(index: usize) -> String {
    if index < 9 {
        (index + 1).to_string()
    } else {
        " ".to_string()
    }
}

pub(super) fn plain(line: Line<'static>) -> Row {
    Row {
        line,
        key: None,
        selected: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn working(id: &str) -> FlowView {
        use crate::event::{Event, EventKind, Mode};
        let events = vec![
            Event::new(
                0,
                EventKind::Started {
                    name: id.into(),
                    request: id.into(),
                    mode: Mode::Auto,
                },
            ),
            Event::new(
                0,
                EventKind::Workspace {
                    path: format!("/shop/{id}"),
                    branch: format!("flow/{id}"),
                    slot: 1,
                    window: None,
                    agent: "claude".into(),
                },
            ),
        ];
        crate::fold::fold(id, &events, None, 0).unwrap()
    }

    #[test]
    fn lists_the_flows_that_do_not_fit_as_more_and_cuts_none_in_half() {
        let flows: Vec<FlowView> = ["a", "b", "c", "d", "e"].map(working).into();
        let board = Board {
            version: 1,
            now: 0,
            count: flows.len(),
            needs_you: 0,
            flows,
        };
        let theme = Theme::default();
        let sidebar = Sidebar {
            board: &board,
            panes: &[],
            focus: &PaneKey::Master,
            cursor: None,
            peek: &[],
            master: None,
            now: 0,
            spin: 0,
            changed: &HashMap::new(),
            theme: &theme,
        };

        let text: Vec<String> = sidebar
            .rows(14)
            .iter()
            .map(|row| {
                row.line
                    .spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect()
            })
            .collect();

        assert!(text.iter().any(|row| row.ends_with(" 3 more")), "{text:#?}");
        let started = text.iter().filter(|row| row.contains("starting")).count();
        let listed = text
            .iter()
            .filter(|row| {
                [" a ", " b ", " c ", " d ", " e "]
                    .iter()
                    .any(|n| row.contains(n))
            })
            .count();
        assert_eq!(started, listed, "every listed flow shows both its rows");
    }

    #[test]
    fn shows_no_task_count_before_there_are_tasks() {
        let mut flow = working("a");
        flow.tasks = Some(crate::view::Tasks { done: 0, total: 0 });
        let board = Board {
            version: 1,
            now: 0,
            count: 1,
            needs_you: 0,
            flows: vec![flow],
        };
        let theme = Theme::default();
        let sidebar = Sidebar {
            board: &board,
            panes: &[],
            focus: &PaneKey::Master,
            cursor: None,
            peek: &[],
            master: None,
            now: 0,
            spin: 0,
            changed: &HashMap::new(),
            theme: &theme,
        };

        let rows = sidebar.flow_rows(&board.flows[0], 0);

        assert!(
            !rows[1].line.to_string().contains("0/0"),
            "{}",
            rows[1].line
        );
    }
}
