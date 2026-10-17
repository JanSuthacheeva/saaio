mod labels;
mod rows;

pub use labels::last_note;

use std::collections::HashMap;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

use crate::proto::{PaneInfo, PaneKey, PaneStatus};
use crate::ui::fit;
use crate::ui::focus;
use crate::ui::layout::SIDEBAR_WIDTH;
use crate::ui::peek::PeekLine;
use crate::ui::prefix::Prefix;
use crate::ui::theme::Theme;
use crate::view::{Board, FlowView, Group, State};

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
    /// The flow whose peek shows (the prefix j/k).
    pub cursor: Option<&'a str>,
    pub peek: &'a [PeekLine],
    /// Where the master runs: ~/projects/shop · main.
    pub master: Option<&'a str>,
    /// Row 0's glyph: App::master_state.
    pub master_state: Option<State>,
    pub now: u64,
    pub spin: usize,
    /// Panes whose state changed a moment ago; their rows stand out.
    pub changed: &'a HashMap<PaneKey, u64>,
    /// The user's home, shown as ~ in paths.
    pub home: Option<&'a str>,
    /// The key before every command, for the hints that name it.
    pub prefix: Prefix,
    pub theme: &'a Theme,
}

/// How a row stands out: focused (fill and bar), lit (fill: the peek, a state that just changed).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Mark {
    None,
    Lit,
    Focus,
}

/// One row of the sidebar or the rail and the pane a click on it focuses.
#[derive(Debug)]
pub(super) struct Row {
    pub line: Line<'static>,
    pub key: Option<PaneKey>,
    pub mark: Mark,
}

impl Row {
    /// The row's line, then its mark over it.
    pub fn render(&self, area: Rect, buf: &mut Buffer, theme: &Theme) {
        buf.set_line(area.x, area.y, &self.line, area.width);
        match self.mark {
            Mark::None => {}
            Mark::Lit => focus::light(buf, area, theme),
            Mark::Focus => focus::mark(buf, area, theme),
        }
    }
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
    /// `path` with ~ for the user's home.
    pub fn tilde(&self, path: &str) -> String {
        fit::tilde(path, self.home)
    }

    /// The pane a click on this row of the sidebar focuses.
    pub fn key_at(&self, row: u16, height: u16) -> Option<PaneKey> {
        self.rows(height).into_iter().nth(row as usize)?.key
    }

    fn rows(&self, height: u16) -> Vec<Row> {
        let room = usize::from(height.saturating_sub(FOOTER_ROWS));
        let mut rows = self.master_rows();
        let listed = workers(self.board);
        if listed.is_empty() {
            rows.extend(self.empty_rows());
        }
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
                    Span::raw(" "),
                    theme.heading(heading.label()),
                    Span::raw(" "),
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

    /// Under the master while there are no workers: "no workers yet" and the prefix n.
    fn empty_rows(&self) -> Vec<Row> {
        let theme = self.theme;
        vec![
            plain(Line::styled(" no workers yet", theme.ui.faint)),
            plain(Line::from(vec![
                Span::raw(" "),
                Span::styled(self.prefix.then("n"), theme.ui.key),
                Span::styled(" starts a flow", theme.ui.faint),
            ])),
        ]
    }

    /// One quiet line at the foot: the prefix and the key that lists the rest.
    fn footer(&self) -> Vec<Row> {
        let theme = self.theme;
        vec![plain(Line::from(vec![
            Span::raw(" "),
            Span::styled(self.prefix.name(), theme.ui.key),
            Span::raw(" "),
            Span::styled("?", theme.ui.key),
            Span::styled(" keys", theme.ui.faint),
        ]))]
    }
}

impl Widget for Sidebar<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        for (row, line) in self.rows(area.height).into_iter().zip(area.rows()) {
            row.render(line, buf, self.theme);
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
        mark: Mark::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::VIEW_VERSION;

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
                    branch: Some(format!("flow/{id}")),
                    slot: Some(1),
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
            version: VIEW_VERSION,
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
            master_state: None,
            now: 0,
            spin: 0,
            changed: &HashMap::new(),
            home: None,
            prefix: Prefix::default(),
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
    fn marks_the_focused_flow_and_lights_the_peeked_and_the_changed() {
        let flows: Vec<FlowView> = ["a", "b", "c", "d"].map(working).into();
        let board = Board {
            version: VIEW_VERSION,
            now: 0,
            count: flows.len(),
            needs_you: 0,
            flows,
        };
        let theme = Theme::default();
        let changed = HashMap::from([(PaneKey::Flow("c".into()), 0)]);
        let focus = PaneKey::Flow("a".into());
        let sidebar = Sidebar {
            board: &board,
            panes: &[],
            focus: &focus,
            cursor: Some("b"),
            peek: &[],
            master: None,
            master_state: None,
            now: 0,
            spin: 0,
            changed: &changed,
            home: None,
            prefix: Prefix::default(),
            theme: &theme,
        };

        let marks: Vec<Mark> = board
            .flows
            .iter()
            .enumerate()
            .map(|(index, flow)| sidebar.flow_rows(flow, index)[0].mark)
            .collect();
        let master = sidebar.master_rows()[2].mark;

        assert_eq!(marks, [Mark::Focus, Mark::Lit, Mark::Lit, Mark::None]);
        assert_eq!(master, Mark::None, "the master is not in view");
        assert!(
            sidebar.flow_rows(&board.flows[0], 0)[0]
                .line
                .to_string()
                .starts_with(" 1 "),
            "the bar is drawn over the blank lead, not in the line"
        );
    }

    #[test]
    fn draws_the_bar_in_the_accent_over_the_rows_of_the_focused_flow() {
        let board = Board {
            version: VIEW_VERSION,
            now: 0,
            count: 1,
            needs_you: 0,
            flows: vec![working("a")],
        };
        let theme = Theme::default();
        let focus = PaneKey::Flow("a".into());
        let area = Rect::new(0, 0, 40, 12);
        let mut buf = Buffer::empty(area);

        Sidebar {
            board: &board,
            panes: &[],
            focus: &focus,
            cursor: None,
            peek: &[],
            master: None,
            master_state: None,
            now: 0,
            spin: 0,
            changed: &HashMap::new(),
            home: None,
            prefix: Prefix::default(),
            theme: &theme,
        }
        .render(area, &mut buf);

        let bars: Vec<u16> = (0..12)
            .filter(|&y| buf[(0, y)].symbol() == theme.glyphs.cursor)
            .collect();
        assert_eq!(bars.len(), 2, "both rows of the flow: {bars:?}");
        for y in bars {
            assert_eq!(buf[(0, y)].fg, theme.palette.accent);
            assert_eq!(buf[(39, y)].bg, theme.palette.selection);
        }
    }

    #[test]
    fn shows_no_task_count_before_there_are_tasks() {
        let mut flow = working("a");
        flow.tasks = Some(crate::view::Tasks { done: 0, total: 0 });
        let board = Board {
            version: VIEW_VERSION,
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
            master_state: None,
            now: 0,
            spin: 0,
            changed: &HashMap::new(),
            home: None,
            prefix: Prefix::default(),
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
