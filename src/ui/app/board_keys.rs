//! The board's keys: moving over the cards, jumping in, every project, clearing a flow

use super::{App, Effect, Removal, View};
use crate::proto::PaneKey;
use crate::ui::board::{self, BoardState};
use crate::ui::input::Key;
use crate::view::FlowView;

impl App {
    pub(super) fn on_board_key(&mut self, state: BoardState, key: Key) -> Vec<Effect> {
        if state.all {
            return self.on_all_key(state, key);
        }
        let cards = self.workers().len() + 1;
        if let Some(step) = board_step(key) {
            self.view = View::Board(state.moved(step, cards));
            return vec![];
        }
        match key {
            Key::Char(digit @ '0'..='9') => {
                let selected = digit as usize - '0' as usize;
                if selected < cards {
                    self.view = View::Board(BoardState { selected, ..state });
                }
                vec![]
            }
            Key::Enter => {
                self.view = View::Panes;
                let key = match state.selected.checked_sub(1) {
                    None => PaneKey::Master,
                    Some(at) => match self.workers().get(at) {
                        Some(flow) => PaneKey::Flow(flow.id.clone()),
                        None => return vec![],
                    },
                };
                self.focus(key)
            }
            Key::Char('a') => {
                self.view = View::Board(BoardState {
                    selected: 0,
                    all: true,
                });
                if self.overview_running {
                    return vec![];
                }
                self.ask_overview()
            }
            Key::Char('x') => {
                let flow = state
                    .selected
                    .checked_sub(1)
                    .and_then(|at| self.workers().get(at).copied())
                    .cloned();
                self.clear(flow)
            }
            _ => self.on_any_board_key(key),
        }
    }

    /// Keys on every project's board: the cards of other projects are only shown.
    fn on_all_key(&mut self, state: BoardState, key: Key) -> Vec<Effect> {
        if let Some(step) = board_step(key) {
            let cards = self
                .projects
                .as_ref()
                .map_or(0, |projects| board::every_flow(projects).len());
            self.view = View::Board(state.moved(step, cards));
            return vec![];
        }
        let enter = key == Key::Enter;
        match key {
            Key::Char('a') => {
                self.view = View::Board(BoardState::default());
                vec![]
            }
            Key::Enter | Key::Char('x') => {
                let selected = self.projects.as_ref().and_then(|projects| {
                    board::every_flow(projects)
                        .get(state.selected)
                        .map(|(project, flow)| {
                            let here = self.here.as_ref() == Some(&project.root);
                            (here, project.name.clone(), (*flow).clone())
                        })
                });
                let Some((here, project, flow)) = selected else {
                    return vec![];
                };
                if !here {
                    self.notice = Some(format!("{} runs in project {project}", flow.name));
                    return vec![];
                }
                if !enter {
                    return self.clear(Some(flow));
                }
                self.view = View::Panes;
                self.focus(PaneKey::Flow(flow.id))
            }
            other => self.on_any_board_key(other),
        }
    }

    /// Esc, q, o and n do the same on either board.
    fn on_any_board_key(&mut self, key: Key) -> Vec<Effect> {
        match key {
            Key::Esc | Key::Char('q' | 'o') => {
                self.view = View::Panes;
                vec![]
            }
            Key::Char('n') => self.open_new_flow(),
            _ => vec![],
        }
    }

    /// Removes a finished flow of this project in the background; says so for any other.
    fn clear(&mut self, flow: Option<FlowView>) -> Vec<Effect> {
        match flow {
            Some(flow) if flow.is_finished() => {
                if let View::Board(state) = &mut self.view {
                    state.selected = 0;
                }
                self.remove(Removal::of(flow.name))
            }
            _ => {
                self.notice = Some("only a finished flow can be cleared".to_string());
                vec![]
            }
        }
    }
}

/// h j k l and the arrows on a board.
fn board_step(key: Key) -> Option<isize> {
    match key {
        Key::Char(c) => BoardState::step(c),
        Key::Left => Some(-1),
        Key::Right => Some(1),
        Key::Down => BoardState::step('j'),
        Key::Up => BoardState::step('k'),
        _ => None,
    }
}
