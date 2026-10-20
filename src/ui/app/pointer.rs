use ratatui::layout::{Margin, Position, Rect};

use super::{App, Effect, View};
use crate::proto::ClientMsg;
use crate::ui::input::{Pointer, PointerKind};
use crate::ui::layout;
use crate::ui::newflow::NewFlowView;
use crate::ui::rail::Rail;
use crate::ui::selection::{Selection, Unit};
use crate::ui::theme::Theme;

/// Lines one wheel notch scrolls the history.
const WHEEL_LINES: u32 = 3;
/// Presses on one cell this close together count as a double or triple click.
const MULTI_CLICK_MS: u64 = 500;

/// The last press, to count double and triple clicks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Clicks {
    at: Position,
    time: u64,
    count: u8,
}

impl App {
    /// A press starts a selection on the surface under it, a drag extends it, and the release
    /// copies it; a press and release on one cell is a click instead.
    pub(super) fn on_pointer(&mut self, pointer: Pointer) -> Vec<Effect> {
        let at = pointer.at;
        match pointer.kind {
            PointerKind::Press => {
                let unit = match self.count_click(at) {
                    1 => Unit::Cell,
                    2 => Unit::Word,
                    _ => Unit::Line,
                };
                self.selection = self
                    .surface_at(at)
                    .map(|bounds| Selection::new(bounds, at, unit));
                vec![]
            }
            PointerKind::Drag => {
                if let Some(selection) = &mut self.selection {
                    selection.extend(at);
                }
                vec![]
            }
            PointerKind::Release => {
                let Some(selection) = &mut self.selection else {
                    return vec![];
                };
                selection.extend(at);
                if selection.is_click() {
                    self.selection = None;
                    self.notice = None;
                    return self.on_click(at);
                }
                vec![Effect::CopySelection(*selection)]
            }
            PointerKind::WheelUp | PointerKind::WheelDown => self.on_wheel(pointer.kind, at),
        }
    }

    /// Over a pane that leaves the mouse to saaio the wheel scrolls its history, and down past
    /// the newest line leaves it; a program on the alternate screen (less, man) gets arrow keys
    /// instead, as tmux does.
    fn on_wheel(&mut self, kind: PointerKind, at: Position) -> Vec<Effect> {
        if !self.areas().pane.contains(at) {
            return vec![];
        }
        let modes = self.screen.modes;
        if self.view == View::Panes && modes.alt_screen {
            let arrow = match (kind, modes.app_cursor) {
                (PointerKind::WheelUp, true) => "\x1bOA",
                (PointerKind::WheelUp, false) => "\x1b[A",
                (_, true) => "\x1bOB",
                (_, false) => "\x1b[B",
            };
            let lines = usize::try_from(WHEEL_LINES).unwrap_or(1);
            return vec![Effect::Send(ClientMsg::Input(
                arrow.repeat(lines).into_bytes(),
            ))];
        }
        let back = match (&self.view, kind) {
            (View::Panes, PointerKind::WheelUp) => WHEEL_LINES,
            (View::History(back), PointerKind::WheelUp) => back.saturating_add(WHEEL_LINES),
            (View::History(back), _) => back.saturating_sub(WHEEL_LINES),
            _ => return vec![],
        };
        if back == 0 {
            self.view = View::Panes;
            return vec![Effect::Send(ClientMsg::History(None))];
        }
        self.view = View::History(back);
        vec![Effect::Send(ClientMsg::History(Some(back)))]
    }

    /// A click on a row of the sidebar or rail focuses its session.
    fn on_click(&mut self, at: Position) -> Vec<Effect> {
        let side = self.areas().side;
        if self.view != View::Panes || !side.contains(at) {
            return vec![];
        }
        let theme = Theme::default();
        let sidebar = self.sidebar(&theme, &[]);
        let key = if self.folded() {
            Rail { sidebar }.key_at(at.y - side.y, side.height)
        } else {
            sidebar.key_at(at.y - side.y, side.height)
        };
        match key {
            Some(key) => self.focus(key),
            None => vec![],
        }
    }

    /// How many presses in a row this one makes on its cell, 1 to 3, then 1 again.
    fn count_click(&mut self, at: Position) -> u8 {
        let count = match self.clicks {
            Some(last) if last.at == at && self.now.saturating_sub(last.time) <= MULTI_CLICK_MS => {
                last.count % 3 + 1
            }
            _ => 1,
        };
        self.clicks = Some(Clicks {
            at,
            time: self.now,
            count,
        });
        count
    }

    /// The open dialog inside its frame, the board (whole screen), the title row, the pane, the
    /// sidebar or rail.
    fn surface_at(&self, at: Position) -> Option<Rect> {
        if let View::Board(_) = self.view {
            let screen = layout::screen(self.size);
            return screen.contains(at).then_some(screen);
        }
        let areas = self.areas();
        let theme = Theme::default();
        let dialog = match &self.view {
            View::NewFlow(form) => {
                let place = self.tilde(&form.repo.display().to_string());
                let view = NewFlowView {
                    form,
                    flows: self.board.as_ref().map_or(&[], |board| &board.flows),
                    place: &place,
                    theme: &theme,
                };
                Some(view.dialog().area(areas.session()))
            }
            _ => self
                .dialog(&theme)
                .map(|dialog| dialog.area(areas.session())),
        };
        if let Some(dialog) = dialog.filter(|dialog| dialog.contains(at)) {
            return Some(dialog.inner(Margin::new(1, 1)));
        }
        [areas.title, areas.pane, areas.side]
            .into_iter()
            .find(|surface| surface.contains(at))
    }
}
