use ratatui::style::{Color, Modifier, Style};

use crate::view::{State, Tone};

/// The colors a theme file names, by its keys; Color::Reset is "default", the terminal's own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    pub background: Color,
    pub panel: Color,
    pub selection: Color,
    pub rule: Color,
    pub text: Color,
    pub strong: Color,
    pub muted: Color,
    pub faint: Color,
    pub accent: Color,
    pub working: Color,
    pub waiting: Color,
    pub blocked: Color,
    pub done: Color,
    pub ended: Color,
}

impl Palette {
    /// ANSI, so flow takes on the terminal's palette.
    pub const TERMINAL: Palette = Palette {
        background: Color::Reset,
        panel: Color::Indexed(235),
        selection: Color::Indexed(236),
        rule: Color::DarkGray,
        text: Color::Reset,
        strong: Color::Reset,
        muted: Color::Gray,
        faint: Color::DarkGray,
        accent: Color::Indexed(173),
        working: Color::Blue,
        waiting: Color::Yellow,
        blocked: Color::Red,
        done: Color::Green,
        ended: Color::DarkGray,
    };

    /// The design's dark colors.
    pub const FLOW: Palette = Palette {
        background: hex(0x141418),
        panel: hex(0x1f1f26),
        selection: hex(0x26262e),
        rule: hex(0x34343d),
        text: hex(0xc8c8d0),
        strong: hex(0xf0f0f4),
        muted: hex(0x9a9aa6),
        faint: hex(0x6c6c78),
        accent: hex(0xe07a52),
        working: hex(0x7aa7ff),
        waiting: hex(0xe5b45a),
        blocked: hex(0xf0717f),
        done: hex(0x93c46d),
        ended: hex(0x6c6c78),
    };

    /// The design's light colors.
    pub const FLOW_LIGHT: Palette = Palette {
        background: hex(0xf6f5f1),
        panel: hex(0xecebe5),
        selection: hex(0xe2e0d8),
        rule: hex(0xcfcdc4),
        text: hex(0x2b2b30),
        strong: hex(0x111114),
        muted: hex(0x5d5d66),
        faint: hex(0x73737c),
        accent: hex(0xb8532f),
        working: hex(0x2f5fd0),
        waiting: hex(0x9a6a00),
        blocked: hex(0xc2283f),
        done: hex(0x3d7a22),
        ended: hex(0x73737c),
    };

    /// "terminal", "flow" or "flow-light"; None for another name.
    pub fn named(name: &str) -> Option<Palette> {
        match name {
            "terminal" => Some(Palette::TERMINAL),
            "flow" => Some(Palette::FLOW),
            "flow-light" => Some(Palette::FLOW_LIGHT),
            _ => None,
        }
    }

    pub fn color(&self, key: PaletteKey) -> Color {
        match key {
            PaletteKey::Background => self.background,
            PaletteKey::Panel => self.panel,
            PaletteKey::Selection => self.selection,
            PaletteKey::Rule => self.rule,
            PaletteKey::Text => self.text,
            PaletteKey::Strong => self.strong,
            PaletteKey::Muted => self.muted,
            PaletteKey::Faint => self.faint,
            PaletteKey::Accent => self.accent,
            PaletteKey::Working => self.working,
            PaletteKey::Waiting => self.waiting,
            PaletteKey::Blocked => self.blocked,
            PaletteKey::Done => self.done,
            PaletteKey::Ended => self.ended,
        }
    }

    pub fn color_mut(&mut self, key: PaletteKey) -> &mut Color {
        match key {
            PaletteKey::Background => &mut self.background,
            PaletteKey::Panel => &mut self.panel,
            PaletteKey::Selection => &mut self.selection,
            PaletteKey::Rule => &mut self.rule,
            PaletteKey::Text => &mut self.text,
            PaletteKey::Strong => &mut self.strong,
            PaletteKey::Muted => &mut self.muted,
            PaletteKey::Faint => &mut self.faint,
            PaletteKey::Accent => &mut self.accent,
            PaletteKey::Working => &mut self.working,
            PaletteKey::Waiting => &mut self.waiting,
            PaletteKey::Blocked => &mut self.blocked,
            PaletteKey::Done => &mut self.done,
            PaletteKey::Ended => &mut self.ended,
        }
    }
}

/// A color a theme file can set, by its key in [ui] or [state].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaletteKey {
    Background,
    Panel,
    Selection,
    Rule,
    Text,
    Strong,
    Muted,
    Faint,
    Accent,
    Working,
    Waiting,
    Blocked,
    Done,
    Ended,
}

impl PaletteKey {
    pub const ALL: [PaletteKey; 14] = [
        PaletteKey::Background,
        PaletteKey::Panel,
        PaletteKey::Selection,
        PaletteKey::Rule,
        PaletteKey::Text,
        PaletteKey::Strong,
        PaletteKey::Muted,
        PaletteKey::Faint,
        PaletteKey::Accent,
        PaletteKey::Working,
        PaletteKey::Waiting,
        PaletteKey::Blocked,
        PaletteKey::Done,
        PaletteKey::Ended,
    ];

    /// The key a theme file writes: accent, working, ...
    pub fn name(self) -> &'static str {
        match self {
            PaletteKey::Background => "background",
            PaletteKey::Panel => "panel",
            PaletteKey::Selection => "selection",
            PaletteKey::Rule => "rule",
            PaletteKey::Text => "text",
            PaletteKey::Strong => "strong",
            PaletteKey::Muted => "muted",
            PaletteKey::Faint => "faint",
            PaletteKey::Accent => "accent",
            PaletteKey::Working => "working",
            PaletteKey::Waiting => "waiting",
            PaletteKey::Blocked => "blocked",
            PaletteKey::Done => "done",
            PaletteKey::Ended => "ended",
        }
    }

    /// None for a key flow does not know.
    pub fn named(name: &str) -> Option<PaletteKey> {
        PaletteKey::ALL.into_iter().find(|key| key.name() == name)
    }
}

/// 0xrrggbb as a terminal color.
pub const fn hex(rgb: u32) -> Color {
    let [_, r, g, b] = rgb.to_be_bytes();
    Color::Rgb(r, g, b)
}

/// Every color and symbol flow ui draws with, by name; a theme file sets them through its Palette.
#[derive(Debug, Clone)]
pub struct Theme {
    pub ui: UiTokens,
    pub state: StateTokens,
    pub glyphs: Glyphs,
    pub palette: Palette,
}

#[derive(Debug, Clone)]
pub struct UiTokens {
    /// The ground every cell flow ui draws is painted on.
    pub background: Style,
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

/// flow's mark: a goose in flight, in block characters every font has.
pub const MARK: &str = "▚▄▞";
/// flow's mark with a Nerd Font: Font Awesome's kiwi bird, in the theme's accent like any mark.
/// Terminals draw Nerd Font icons wider than their one cell, so it brings a space of its own.
pub const KIWI: &str = "\u{edff} ";

#[derive(Debug, Clone)]
pub struct Glyphs {
    pub mark: &'static str,
    pub waiting: &'static str,
    /// A worker whose turn ended without a question.
    pub idle: &'static str,
    pub blocked: &'static str,
    pub done: &'static str,
    pub ended: &'static str,
    pub spinner: &'static [&'static str],
    pub cursor: &'static str,
    pub separator: &'static str,
    /// The line between the pane and the sidebar, and where the sidebar's header rule meets it.
    pub rule_v: &'static str,
    pub rule_joint: &'static str,
    pub step_done: &'static str,
    pub step_current: &'static str,
    pub step_open: &'static str,
    pub step_join: &'static str,
    pub more: &'static str,
    pub tool: &'static str,
    pub result: &'static str,
    pub dash: &'static str,
}

/// The terminal theme: today's look.
impl Default for Theme {
    fn default() -> Theme {
        Theme::from_palette(Palette::TERMINAL)
    }
}

impl Theme {
    /// The styles widgets draw with, from a palette's colors; "default" leaves a color to the terminal.
    pub fn from_palette(palette: Palette) -> Theme {
        let plain = Style::default();
        let fg = |color: Color| match color {
            Color::Reset => plain,
            color => plain.fg(color),
        };
        let bg = |color: Color| match color {
            Color::Reset => plain,
            color => plain.bg(color),
        };
        // The terminal's own text may be dark, which the terminal theme's dark panel would swallow.
        let on_panel = match palette.text {
            Color::Reset => Color::Indexed(252),
            text => text,
        };
        Theme {
            ui: UiTokens {
                background: bg(palette.background),
                text: fg(palette.text),
                strong: fg(palette.strong).add_modifier(Modifier::BOLD),
                muted: fg(palette.muted),
                faint: fg(palette.faint),
                accent: fg(palette.accent),
                rule: fg(palette.rule),
                selection: bg(palette.selection),
                key: fg(palette.strong).add_modifier(Modifier::BOLD),
                bar: bg(palette.panel).fg(on_panel),
            },
            state: StateTokens {
                working: fg(palette.working),
                waiting: fg(palette.waiting),
                blocked: fg(palette.blocked),
                done: fg(palette.done),
                ended: fg(palette.ended),
            },
            glyphs: Glyphs {
                mark: MARK,
                waiting: "●",
                idle: "○",
                blocked: "●",
                done: "✓",
                ended: "○",
                spinner: &["◐", "◓", "◑", "◒"],
                cursor: "▌",
                separator: "│",
                rule_v: "│",
                rule_joint: "├",
                step_done: "●",
                step_current: "●",
                step_open: "○",
                step_join: "───",
                more: "…",
                tool: "⏺",
                result: "⎿",
                dash: "─",
            },
            palette,
        }
    }

    pub fn state_style(&self, state: State) -> Style {
        match state {
            State::Working | State::Delegated => self.state.working,
            State::Waiting => self.state.waiting,
            State::Idle => self.ui.muted,
            State::Blocked | State::NotResponding => self.state.blocked,
            State::Done => self.state.done,
            State::Ended => self.state.ended,
        }
    }

    pub fn tone_style(&self, tone: Tone) -> Style {
        match tone {
            Tone::Neutral => self.ui.muted,
            Tone::Progress => self.state.working,
            Tone::Attention => self.state.waiting,
            Tone::Problem => self.state.blocked,
            Tone::Success => self.state.done,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_terminal_theme_looks_as_flow_always_did() {
        let theme = Theme::default();
        let plain = Style::default();

        assert_eq!(theme.ui.text, plain);
        assert_eq!(theme.ui.background, plain);
        assert_eq!(theme.ui.strong, plain.add_modifier(Modifier::BOLD));
        assert_eq!(theme.ui.accent, plain.fg(Color::Indexed(173)));
        assert_eq!(theme.ui.selection, plain.bg(Color::Indexed(236)));
        assert_eq!(
            theme.ui.bar,
            plain.bg(Color::Indexed(235)).fg(Color::Indexed(252))
        );
        assert_eq!(theme.state.working, plain.fg(Color::Blue));
    }

    #[test]
    fn builds_the_designs_dark_theme() {
        let theme = Theme::from_palette(Palette::FLOW);

        assert_eq!(theme.ui.background, Style::default().bg(hex(0x141418)));
        assert_eq!(theme.ui.text, Style::default().fg(hex(0xc8c8d0)));
        assert_eq!(
            theme.ui.bar,
            Style::default().bg(hex(0x1f1f26)).fg(hex(0xc8c8d0))
        );
        assert!(Palette::named("flow-light").is_some());
        assert_eq!(Palette::named("solarized"), None);
    }

    #[test]
    fn gets_and_sets_colors_by_their_key() {
        let mut palette = Palette::TERMINAL;
        let accent = PaletteKey::named("accent").unwrap();

        *palette.color_mut(accent) = Color::Indexed(5);

        assert_eq!(palette.color(accent), Color::Indexed(5));
        assert_eq!(PaletteKey::named("acent"), None);
        for key in PaletteKey::ALL {
            assert_eq!(PaletteKey::named(key.name()), Some(key));
        }
    }

    #[test]
    fn gives_each_state_a_color_that_tells_it_apart() {
        let theme = Theme::default();

        assert_eq!(
            theme.state_style(State::Blocked),
            theme.state_style(State::NotResponding)
        );
        assert_ne!(
            theme.state_style(State::Waiting),
            theme.state_style(State::Blocked)
        );
        assert_ne!(
            theme.state_style(State::Working),
            theme.state_style(State::Done)
        );
    }
}
