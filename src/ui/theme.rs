mod palette;

use std::collections::BTreeMap;

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;

use crate::event::Phase;
use crate::view::{FlowView, State, Tone};
use crate::workflow::MAX_PHASES;

pub use palette::{ColorKey, Palette, PaletteKey, Section, hex};

/// How much of a working flow takes its phase's color: theme.toml's phase_colors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PhaseColors {
    /// State colors only.
    #[default]
    Off,
    /// The current step dot; spinner, label, rail and title keep the state color.
    Steps,
    /// Everything that takes the state color.
    Full,
}

impl PhaseColors {
    /// "off", "steps" or "full"; None for another name.
    pub fn named(name: &str) -> Option<PhaseColors> {
        match name {
            "off" => Some(PhaseColors::Off),
            "steps" => Some(PhaseColors::Steps),
            "full" => Some(PhaseColors::Full),
            _ => None,
        }
    }
}

/// Every color and symbol saaio ui draws with, by name; a theme file sets them through its Palette.
#[derive(Debug, Clone)]
pub struct Theme {
    pub ui: UiTokens,
    pub state: StateTokens,
    pub phase: PhaseTokens,
    pub phase_colors: PhaseColors,
    pub glyphs: Glyphs,
    pub palette: Palette,
}

#[derive(Debug, Clone)]
pub struct UiTokens {
    /// The ground every cell saaio ui draws is painted on.
    pub background: Style,
    pub text: Style,
    pub strong: Style,
    pub muted: Style,
    pub faint: Style,
    pub accent: Style,
    pub rule: Style,
    pub selection: Style,
    pub key: Style,
    /// The title row above the pane: a dark panel row with light text on it.
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
pub struct PhaseTokens {
    pub positions: [Style; MAX_PHASES],
    pub names: BTreeMap<String, Style>,
}

/// saaio's mark: a goose in flight, in block characters every font has.
pub const MARK: &str = "▚▄▞";

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
            phase: PhaseTokens {
                positions: palette.phases.positions.map(fg),
                names: palette
                    .phases
                    .names
                    .iter()
                    .map(|(name, color)| (name.clone(), fg(*color)))
                    .collect(),
            },
            phase_colors: PhaseColors::default(),
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

    /// The phase's color by name, else by its position among the flow's steps.
    pub fn phase_style(&self, phase: &Phase, position: usize) -> Style {
        self.phase
            .names
            .get(phase.name())
            .copied()
            .unwrap_or_else(|| self.phase.positions[position.min(MAX_PHASES - 1)])
    }

    /// The phase's color at Full while the flow works in one, else its state's.
    pub fn flow_style(&self, flow: &FlowView) -> Style {
        self.tinted(flow, self.phase_colors == PhaseColors::Full)
    }

    /// The phase's color at Steps or Full while the flow works in one, else its state's.
    pub fn step_style(&self, flow: &FlowView) -> Style {
        self.tinted(flow, self.phase_colors != PhaseColors::Off)
    }

    /// The phase's color when `colored` and the flow works in one, else its state's.
    fn tinted(&self, flow: &FlowView, colored: bool) -> Style {
        match (flow.state, &flow.phase) {
            (State::Working | State::Delegated, Some(phase)) if colored => {
                let at = flow.steps.iter().position(|step| step.phase == *phase);
                let position = at.unwrap_or(0);
                self.phase_style(phase, position)
            }
            (state, _) => self.state_style(state),
        }
    }

    /// A section's heading: its name in capitals, faint, as the sidebar's NEEDS YOU.
    pub fn heading(&self, text: &str) -> Span<'static> {
        Span::styled(text.to_uppercase(), self.ui.faint)
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
    use crate::fixtures;

    #[test]
    fn every_key_belongs_in_one_section() {
        let names = |section| -> Vec<&str> {
            PaletteKey::ALL
                .into_iter()
                .filter(|key| key.section() == section)
                .map(PaletteKey::name)
                .collect()
        };

        assert_eq!(
            names(Section::Ui),
            [
                "background",
                "panel",
                "selection",
                "rule",
                "text",
                "strong",
                "muted",
                "faint",
                "accent"
            ]
        );
        assert_eq!(
            names(Section::State),
            ["working", "waiting", "blocked", "done", "ended"]
        );
        assert!(
            names(Section::Phase).is_empty(),
            "[phase] takes phase names"
        );
        for section in Section::ALL {
            assert_eq!(Section::named(section.name()), Some(section));
        }
        assert_eq!(Section::named("glyphs"), None);
    }

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
        let theme = Theme::from_palette(Palette::SAAIO);

        assert_eq!(theme.ui.background, Style::default().bg(hex(0x141418)));
        assert_eq!(theme.ui.text, Style::default().fg(hex(0xc8c8d0)));
        assert_eq!(
            theme.ui.bar,
            Style::default().bg(hex(0x1f1f26)).fg(hex(0xc8c8d0))
        );
        assert!(Palette::named("saaio-light").is_some());
        assert_eq!(Palette::named("solarized"), None);
    }

    #[test]
    fn a_heading_is_its_name_in_faint_capitals() {
        let theme = Theme::from_palette(Palette::SAAIO);

        let heading = theme.heading("last answer");

        assert_eq!(heading.content, "LAST ANSWER");
        assert_eq!(heading.style, theme.ui.faint);
    }

    #[test]
    fn gets_and_sets_colors_by_their_key() {
        let mut palette = Palette::TERMINAL;
        let accent = PaletteKey::named("accent").unwrap();

        palette.set(&ColorKey::Fixed(accent), Color::Indexed(5));
        palette.set(&ColorKey::Phase(Phase::of("build")), Color::Indexed(6));

        assert_eq!(palette.color(accent), Color::Indexed(5));
        assert_eq!(palette.phases.names["build"], Color::Indexed(6));
        assert_eq!(PaletteKey::named("acent"), None);
        for key in PaletteKey::ALL {
            assert_eq!(PaletteKey::named(key.name()), Some(key));
        }
    }

    /// A flow in the built-in workflow's phases.
    fn flow(state: State, phase: Option<Phase>) -> FlowView {
        FlowView {
            phase,
            state,
            ..fixtures::flow("shop-217", Vec::new(), 0)
        }
    }

    #[test]
    fn at_full_a_working_flow_takes_the_color_of_its_phase() {
        let mut theme = Theme::from_palette(Palette::SAAIO);
        theme.phase_colors = PhaseColors::Full;
        let phase =
            |position: usize| Style::default().fg(Palette::SAAIO.phases.positions[position]);

        assert_eq!(
            theme.flow_style(&flow(State::Working, Some(Phase::of("plan")))),
            phase(0)
        );
        assert_eq!(
            theme.flow_style(&flow(State::Delegated, Some(Phase::of("review")))),
            phase(2)
        );
        assert_eq!(
            theme.flow_style(&flow(State::Working, Some(Phase::of("merge-request")))),
            phase(3)
        );
        assert_eq!(
            theme.flow_style(&flow(State::Working, None)),
            theme.state.working,
            "no phase yet"
        );
        assert_eq!(
            theme.flow_style(&flow(State::Waiting, Some(Phase::of("implement")))),
            theme.state.waiting,
            "a flow that needs you keeps the state's color"
        );
        assert_eq!(
            theme.flow_style(&flow(State::Idle, Some(Phase::of("implement")))),
            theme.ui.muted
        );
    }

    #[test]
    fn the_level_decides_what_takes_the_phase_color() {
        let mut theme = Theme::from_palette(Palette::SAAIO);
        let review = theme.phase.positions[2];
        let working = theme.state.working;
        let flow = flow(State::Delegated, Some(Phase::of("review")));
        let styles = |theme: &Theme| (theme.step_style(&flow), theme.flow_style(&flow));

        assert_eq!(theme.phase_colors, PhaseColors::Off, "the default");
        assert_eq!(styles(&theme), (working, working), "off");
        theme.phase_colors = PhaseColors::Steps;
        assert_eq!(styles(&theme), (review, working), "steps");
        theme.phase_colors = PhaseColors::Full;
        assert_eq!(styles(&theme), (review, review), "full");
    }

    #[test]
    fn a_waiting_flow_keeps_its_state_color_at_every_level() {
        let mut theme = Theme::from_palette(Palette::SAAIO);
        let flow = flow(State::Waiting, Some(Phase::of("implement")));
        for level in [PhaseColors::Off, PhaseColors::Steps, PhaseColors::Full] {
            theme.phase_colors = level;
            assert_eq!(theme.step_style(&flow), theme.state.waiting, "{level:?}");
            assert_eq!(theme.flow_style(&flow), theme.state.waiting, "{level:?}");
        }
    }

    #[test]
    fn names_the_levels() {
        assert_eq!(PhaseColors::named("off"), Some(PhaseColors::Off));
        assert_eq!(PhaseColors::named("steps"), Some(PhaseColors::Steps));
        assert_eq!(PhaseColors::named("full"), Some(PhaseColors::Full));
        assert_eq!(PhaseColors::named("Full"), None);
    }

    #[test]
    fn the_built_in_themes_color_each_phase() {
        let phases = |palette: Palette| palette.phases.positions;

        assert_eq!(
            phases(Palette::TERMINAL),
            [
                Color::Indexed(141),
                Color::Indexed(75),
                Color::Indexed(80),
                Color::Indexed(176)
            ]
        );
        assert_eq!(
            phases(Palette::SAAIO),
            [hex(0xb392f0), hex(0x7aa7ff), hex(0x56c2c2), hex(0xe07fd0)]
        );
        assert_eq!(
            phases(Palette::SAAIO_LIGHT),
            [hex(0x7a4fc9), hex(0x2f5fd0), hex(0x13807f), hex(0xb0399c)]
        );
        assert_eq!(PaletteKey::named("merge-request"), None);
    }

    #[test]
    fn a_phase_takes_its_color_by_name_else_by_its_position() {
        let mut palette = Palette::SAAIO;
        palette.set(&ColorKey::Phase(Phase::of("build")), hex(0x123456));
        let mut theme = Theme::from_palette(palette);
        theme.phase_colors = PhaseColors::Steps;

        assert_eq!(
            theme.phase_style(&Phase::of("build"), 0),
            Style::default().fg(hex(0x123456))
        );
        assert_eq!(
            theme.phase_style(&Phase::of("spec"), 1),
            Style::default().fg(hex(0x7aa7ff)),
            "the 2nd phase's built-in color"
        );
        let mut custom = flow(State::Working, Some(Phase::of("ship")));
        custom.steps.truncate(3);
        custom.steps[2].phase = Phase::of("ship");
        assert_eq!(
            theme.step_style(&custom),
            Style::default().fg(hex(0x56c2c2)),
            "the position among the flow's own steps"
        );
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
