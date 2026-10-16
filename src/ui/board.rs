mod cards;
mod detail;
#[cfg(test)]
mod fixtures;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

use crate::proto::{PaneKey, PaneStatus};
use crate::text::plural;
use crate::ui::fit::split;
use crate::ui::sidebar::{self, Sidebar};
use crate::ui::theme::Theme;
use crate::view::{Board, FlowView, Project, Projects, State};

/// Cards per row.
const COLUMNS: usize = 3;
/// A card's rows: its four lines inside a border.
const CARD_ROWS: u16 = 6;
/// Rows every project's cards leave for the selected flow's detail.
const DETAIL_ROWS: u16 = 10;

/// Which card the board's cursor is on: 0 is the master, n the nth worker; with `all`, the nth flow
/// of every project's, counted from 0.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BoardState {
    pub selected: usize,
    pub all: bool,
}

impl BoardState {
    /// h j k l and the arrows move over the grid; a digit picks a card.
    pub fn moved(self, step: isize, cards: usize) -> BoardState {
        self.selected
            .checked_add_signed(step)
            .filter(|&next| next < cards)
            .map_or(self, |selected| BoardState { selected, ..self })
    }

    pub fn step(key: char) -> Option<isize> {
        match key {
            'h' => Some(-1),
            'l' => Some(1),
            'j' => Some(COLUMNS as isize),
            'k' => Some(-(COLUMNS as isize)),
            _ => None,
        }
    }
}

/// Every session as a card, and the timeline, decisions and steps of the selected one.
#[derive(Debug)]
pub struct BoardView<'a> {
    pub sidebar: Sidebar<'a>,
    pub state: BoardState,
    /// The pane esc goes back to.
    pub back: &'a str,
    /// The local time zone's offset from UTC, for the timeline's clock times.
    pub offset_secs: i64,
    /// Shown in place of the keys, such as why clearing a flow failed.
    pub notice: Option<&'a str>,
    /// Every project's flows, this project first, once `flow status --all` answered.
    pub projects: Option<&'a Projects>,
    /// This project's store, to tell its flows from other projects'.
    pub here: Option<&'a str>,
}

/// Every project's flows in board order, each with its project.
pub fn every_flow(projects: &Projects) -> Vec<(&Project, &FlowView)> {
    projects
        .projects
        .iter()
        .flat_map(|project| project.board.flows.iter().map(move |flow| (project, flow)))
        .collect()
}

impl BoardView<'_> {
    fn theme(&self) -> &Theme {
        self.sidebar.theme
    }

    fn board(&self) -> &Board {
        self.sidebar.board
    }

    fn header(&self, width: usize) -> Line<'static> {
        if self.state.all {
            return self.all_header(width);
        }
        let theme = self.theme();
        let flows = sidebar::workers(self.board());
        let count = |states: &[State]| {
            flows
                .iter()
                .filter(|flow| states.contains(&flow.state))
                .count()
        };
        let sessions = flows.len() + 1;
        let mut right = vec![Span::styled(
            format!("{sessions} session{}", plural(sessions)),
            theme.ui.faint,
        )];
        let parts = [
            (
                count(&[State::Waiting, State::Blocked, State::NotResponding]),
                "need you",
                theme.state.waiting,
            ),
            (count(&[State::Working]), "running", theme.ui.faint),
            (count(&[State::Idle]), "ready", theme.ui.faint),
            (count(&[State::Delegated]), "delegated", theme.ui.faint),
            (count(&[State::Done]), "done", theme.ui.faint),
            (count(&[State::Ended]), "ended", theme.ui.faint),
        ];
        for (number, label, style) in parts {
            if number > 0 {
                right.push(Span::styled(" · ", theme.ui.faint));
                right.push(Span::styled(format!("{number} {label}"), style));
            }
        }
        right.push(Span::raw(" "));
        split(
            vec![
                Span::raw(" "),
                Span::styled(theme.glyphs.mark, theme.ui.accent),
                Span::styled(" flow", theme.ui.strong),
                Span::styled(" board", theme.ui.faint),
            ],
            right,
            width,
        )
    }

    fn status(&self, key: &PaneKey) -> Option<&PaneStatus> {
        self.sidebar
            .panes
            .iter()
            .find(|pane| &pane.key == key)
            .map(|pane| &pane.status)
    }

    fn footer(&self) -> Line<'static> {
        let theme = self.theme();
        if let Some(notice) = self.notice {
            return Line::styled(format!(" {notice}"), theme.state.waiting);
        }
        let key = |text: &'static str| Span::styled(text, theme.ui.key);
        let hint = |text: &'static str| Span::styled(text, theme.ui.faint);
        let mut spans = vec![Span::raw(" ")];
        if !self.state.all {
            spans.extend([key("0-9"), hint(" select   ")]);
        }
        spans.extend([
            key("h j k l"),
            hint(" move   "),
            key("enter"),
            hint(" jump in   "),
            key("n"),
            hint(" new flow   "),
            key("x"),
            hint(" clear finished flow   "),
            key("a"),
            hint(if self.state.all {
                " this project   "
            } else {
                " every project   "
            }),
            key("esc"),
            Span::styled(format!(" back to {}", self.back), theme.ui.faint),
        ]);
        Line::from(spans)
    }
}

impl Widget for BoardView<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let theme = self.theme();
        let width = area.width as usize;
        if area.height < 4 {
            return;
        }
        buf.set_line(area.x, area.y, &self.header(width), area.width);
        let rule = Line::styled("─".repeat(width), theme.ui.rule);
        buf.set_line(area.x, area.y + 1, &rule, area.width);
        let footer_y = area.y + area.height - 1;
        buf.set_line(area.x, footer_y - 1, &rule, area.width);
        buf.set_line(area.x, footer_y, &self.footer(), area.width);

        let cards = Rect::new(
            area.x,
            area.y + 2,
            area.width,
            footer_y.saturating_sub(area.y + 3),
        );
        let bottom = match (self.state.all, self.projects) {
            (false, _) => self.render_cards(cards, buf),
            (true, Some(projects)) => self.render_projects(projects, cards, buf),
            (true, None) => {
                let line = Line::styled(" reading every project…", theme.ui.faint);
                buf.set_line(cards.x, cards.y, &line, cards.width);
                return;
            }
        };
        let detail_y = bottom + 1;
        if detail_y + 3 < footer_y - 1 {
            let detail = Rect::new(area.x, detail_y, area.width, footer_y - 1 - detail_y);
            self.render_detail(detail, buf);
        }
    }
}

/// A row of every project's board: a project's line, or one of its cards.
#[derive(Debug)]
enum Item<'p> {
    Project(&'p Project),
    Card {
        flow: &'p FlowView,
        /// Its place among every project's cards, from 0.
        number: usize,
        column: usize,
    },
}

/// The card in `column` with its top at `y`: the columns share the width less their gaps.
fn card_rect(area: Rect, column: usize, y: u16) -> Rect {
    const GAP: u16 = 2;
    let columns = COLUMNS as u16;
    let width = area
        .width
        .saturating_sub(2)
        .saturating_sub(GAP * (columns - 1))
        / columns;
    let x = area.x + 1 + column as u16 * (width + GAP);
    Rect::new(x, y, width, CARD_ROWS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moves_over_the_grid_but_not_off_it() {
        let state = BoardState {
            selected: 1,
            all: false,
        };

        assert_eq!(state.moved(1, 6).selected, 2);
        assert_eq!(state.moved(3, 6).selected, 4);
        assert_eq!(state.moved(-3, 6).selected, 1, "no row above");
        let last = BoardState {
            selected: 5,
            all: true,
        };
        assert_eq!(last.moved(1, 6), last);
        assert!(last.moved(-1, 6).all, "moving keeps every project in view");
    }
}
