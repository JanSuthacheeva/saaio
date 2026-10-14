use ratatui::style::{Color, Modifier, Style};

use crate::view::State;

/// Every color and symbol flow ui draws with, by name, so a theme file can replace them later.
#[derive(Debug, Clone)]
pub struct Theme {
    pub ui: UiTokens,
    pub state: StateTokens,
    pub glyphs: Glyphs,
}

#[derive(Debug, Clone)]
pub struct UiTokens {
    pub text: Style,
    pub strong: Style,
    pub muted: Style,
    pub faint: Style,
    pub accent: Style,
    pub rule: Style,
    pub selection: Style,
    pub key: Style,
    /// The bottom bar: a dark panel row with light text on it.
    pub bar: Style,
}

#[derive(Debug, Clone)]
pub struct StateTokens {
    pub working: Style,
    pub waiting: Style,
    pub blocked: Style,
    pub done: Style,
    pub ended: Style,
}

#[derive(Debug, Clone)]
pub struct Glyphs {
    pub mark: &'static str,
    pub master: &'static str,
    pub waiting: &'static str,
    pub blocked: &'static str,
    pub done: &'static str,
    pub ended: &'static str,
    pub spinner: &'static [&'static str],
    pub cursor: &'static str,
    pub separator: &'static str,
    pub in_view: &'static str,
    pub step_done: &'static str,
    pub step_current: &'static str,
    pub step_open: &'static str,
    pub step_join: &'static str,
    pub more: &'static str,
}

impl Theme {
    /// ANSI colors 0-15, so flow takes on the terminal's own palette; the accent (clay), the bar's
    /// dark panel and the selection's slightly lighter one are 256-color.
    pub fn builtin() -> Theme {
        let plain = Style::default();
        Theme {
            ui: UiTokens {
                text: plain,
                strong: plain.add_modifier(Modifier::BOLD),
                muted: plain.fg(Color::Gray),
                faint: plain.fg(Color::DarkGray),
                accent: plain.fg(Color::Indexed(173)),
                rule: plain.fg(Color::DarkGray),
                selection: plain.bg(Color::Indexed(236)),
                key: plain.add_modifier(Modifier::BOLD),
                bar: plain.bg(Color::Indexed(235)).fg(Color::Indexed(252)),
            },
            state: StateTokens {
                working: plain.fg(Color::Blue),
                waiting: plain.fg(Color::Yellow),
                blocked: plain.fg(Color::Red),
                done: plain.fg(Color::Green),
                ended: plain.fg(Color::DarkGray),
            },
            glyphs: Glyphs {
                mark: "≋",
                master: "◎",
                waiting: "◆",
                blocked: "✖",
                done: "✔",
                ended: "○",
                spinner: &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"],
                cursor: "▌",
                separator: "│",
                in_view: "◂",
                step_done: "●",
                step_current: "◉",
                step_open: "○",
                step_join: "─",
                more: "…",
            },
        }
    }

    pub fn state(&self, state: State) -> Style {
        match state {
            State::Working | State::Delegated => self.state.working,
            State::Waiting => self.state.waiting,
            State::Blocked | State::NotResponding => self.state.blocked,
            State::Done => self.state.done,
            State::Ended => self.state.ended,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gives_each_state_a_color_that_tells_it_apart() {
        let theme = Theme::builtin();

        assert_eq!(
            theme.state(State::Blocked),
            theme.state(State::NotResponding)
        );
        assert_ne!(theme.state(State::Waiting), theme.state(State::Blocked));
        assert_ne!(theme.state(State::Working), theme.state(State::Done));
    }
}
