use std::collections::BTreeMap;

use ratatui::style::Color;

use crate::event::Phase;
use crate::workflow::MAX_PHASES;

/// The colors a theme file names, by its keys; Color::Reset is "default", the terminal's own.
#[derive(Debug, Clone, PartialEq, Eq)]
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
    pub phases: PhasePalette,
}

/// The phases' colors: by position (the built-in themes' four), and by name from [phase].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhasePalette {
    pub positions: [Color; MAX_PHASES],
    pub names: BTreeMap<String, Color>,
}

impl PhasePalette {
    /// The colors of the 1st to 4th phase, none by name.
    const fn at(positions: [Color; MAX_PHASES]) -> PhasePalette {
        PhasePalette {
            positions,
            names: BTreeMap::new(),
        }
    }
}

impl Palette {
    /// ANSI, so saaio takes on the terminal's palette.
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
        phases: PhasePalette::at([
            Color::Indexed(141),
            Color::Indexed(75),
            Color::Indexed(80),
            Color::Indexed(176),
        ]),
    };

    /// The design's dark colors.
    pub const SAAIO: Palette = Palette {
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
        phases: PhasePalette::at([hex(0xb392f0), hex(0x7aa7ff), hex(0x56c2c2), hex(0xe07fd0)]),
    };

    /// The design's light colors.
    pub const SAAIO_LIGHT: Palette = Palette {
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
        phases: PhasePalette::at([hex(0x7a4fc9), hex(0x2f5fd0), hex(0x13807f), hex(0xb0399c)]),
    };

    /// "terminal", "saaio" or "saaio-light"; None for another name.
    pub fn named(name: &str) -> Option<Palette> {
        match name {
            "terminal" => Some(Palette::TERMINAL),
            "saaio" => Some(Palette::SAAIO),
            "saaio-light" => Some(Palette::SAAIO_LIGHT),
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

    pub fn set(&mut self, key: &ColorKey, color: Color) {
        let key = match key {
            ColorKey::Fixed(key) => *key,
            ColorKey::Phase(phase) => {
                self.phases.names.insert(phase.name().to_string(), color);
                return;
            }
        };
        *match key {
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
        } = color;
    }
}

/// A color a theme file sets: a fixed key of [ui] or [state], or a phase by name in [phase].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ColorKey {
    Fixed(PaletteKey),
    Phase(Phase),
}

impl ColorKey {
    /// The key as a theme file writes it.
    pub fn name(&self) -> &str {
        match self {
            ColorKey::Fixed(key) => key.name(),
            ColorKey::Phase(phase) => phase.name(),
        }
    }
}

/// A fixed color a theme file can set, by its key in [ui] or [state].
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

    /// None for a key saaio does not know.
    pub fn named(name: &str) -> Option<PaletteKey> {
        PaletteKey::ALL.into_iter().find(|key| key.name() == name)
    }

    /// The table a theme file sets this key in.
    pub fn section(self) -> Section {
        match self {
            PaletteKey::Background
            | PaletteKey::Panel
            | PaletteKey::Selection
            | PaletteKey::Rule
            | PaletteKey::Text
            | PaletteKey::Strong
            | PaletteKey::Muted
            | PaletteKey::Faint
            | PaletteKey::Accent => Section::Ui,
            PaletteKey::Working
            | PaletteKey::Waiting
            | PaletteKey::Blocked
            | PaletteKey::Done
            | PaletteKey::Ended => Section::State,
        }
    }
}

/// A table of colors in a theme file: [ui], [state] or [phase], whose keys are phase names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Ui,
    State,
    Phase,
}

impl Section {
    pub const ALL: [Section; 3] = [Section::Ui, Section::State, Section::Phase];

    /// The table's name in a theme file: ui, state, phase.
    pub fn name(self) -> &'static str {
        match self {
            Section::Ui => "ui",
            Section::State => "state",
            Section::Phase => "phase",
        }
    }

    /// None for a table saaio does not know.
    pub fn named(name: &str) -> Option<Section> {
        Section::ALL
            .into_iter()
            .find(|section| section.name() == name)
    }
}

/// 0xrrggbb as a terminal color.
pub const fn hex(rgb: u32) -> Color {
    let [_, r, g, b] = rgb.to_be_bytes();
    Color::Rgb(r, g, b)
}
