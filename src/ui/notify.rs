//! `Notifier`: desktop notifications through the terminal, by OSC 9, OSC 777 or its bell

use std::io::Write;

use anyhow::{Context, Result};

use crate::text;

/// The most characters a notification says; a terminal shows less anyway.
const TEXT_LIMIT: usize = 200;

/// The one sequence a terminal turns into a desktop notification; sending several would show it
/// twice in a terminal that reads both.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sequence {
    /// ESC ] 9 ; saaio: text BEL - iTerm2, Ghostty, WezTerm, kitty, and the default.
    Osc9,
    /// ESC ] 777 ; notify ; saaio ; text BEL - foot, urxvt, VTE.
    Osc777,
    /// BEL alone - Alacritty, Terminal.app.
    Bell,
}

impl Sequence {
    /// Bell for Alacritty ($ALACRITTY_WINDOW_ID, TERM alacritty*) and Terminal.app
    /// (TERM_PROGRAM Apple_Terminal); Osc777 for TERM foot* / rxvt* or $VTE_VERSION; else Osc9.
    pub fn pick(term: Option<&str>, program: Option<&str>, vte: bool, alacritty: bool) -> Sequence {
        let term = term.unwrap_or_default();
        if alacritty || term.starts_with("alacritty") || program == Some("Apple_Terminal") {
            Sequence::Bell
        } else if vte || term.starts_with("foot") || term.starts_with("rxvt") {
            Sequence::Osc777
        } else {
            Sequence::Osc9
        }
    }
}

/// How the UI raises a desktop notification: the sequence its terminal reads, wrapped for tmux
/// when saaio runs inside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notifier {
    sequence: Sequence,
    tmux: bool,
}

impl Notifier {
    pub fn new(sequence: Sequence, tmux: bool) -> Notifier {
        Notifier { sequence, tmux }
    }

    /// Writes the notification for `text` (control characters dropped, cut to 200 chars) and
    /// flushes; Err when the terminal cannot be written.
    pub fn notify(&self, out: &mut impl Write, text: &str) -> Result<()> {
        let text: String = text::printable(text).chars().take(TEXT_LIMIT).collect();
        let written = sequence(self.sequence, &text);
        let written = if self.tmux && self.sequence != Sequence::Bell {
            passthrough(&written)
        } else {
            written
        };
        out.write_all(written.as_bytes())
            .and_then(|()| out.flush())
            .context("could not write the notification to the terminal")
    }
}

fn sequence(sequence: Sequence, text: &str) -> String {
    match sequence {
        Sequence::Osc9 => format!("\x1b]9;saaio: {text}\x07"),
        Sequence::Osc777 => format!("\x1b]777;notify;saaio;{text}\x07"),
        Sequence::Bell => "\x07".to_string(),
    }
}

/// ESC P tmux; <sequence, every ESC doubled> ESC \
fn passthrough(sequence: &str) -> String {
    format!("\x1bPtmux;{}\x1b\\", sequence.replace('\x1b', "\x1b\x1b"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn written(notifier: &Notifier, text: &str) -> String {
        let mut out = Vec::new();
        notifier.notify(&mut out, text).unwrap();
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn most_terminals_get_osc9() {
        for (term, program) in [
            (Some("xterm-ghostty"), Some("ghostty")),
            (Some("xterm-256color"), Some("iTerm.app")),
            (Some("xterm-kitty"), None),
            (Some("wezterm"), Some("WezTerm")),
            (None, None),
        ] {
            assert_eq!(
                Sequence::pick(term, program, false, false),
                Sequence::Osc9,
                "{term:?}"
            );
        }
    }

    #[test]
    fn foot_rxvt_and_vte_get_osc777() {
        assert_eq!(
            Sequence::pick(Some("foot"), None, false, false),
            Sequence::Osc777
        );
        assert_eq!(
            Sequence::pick(Some("rxvt-unicode-256color"), None, false, false),
            Sequence::Osc777
        );
        assert_eq!(
            Sequence::pick(Some("xterm-256color"), None, true, false),
            Sequence::Osc777
        );
    }

    #[test]
    fn alacritty_and_terminal_app_get_the_bell() {
        assert_eq!(
            Sequence::pick(Some("alacritty"), None, false, false),
            Sequence::Bell
        );
        assert_eq!(
            Sequence::pick(Some("xterm-256color"), None, false, true),
            Sequence::Bell
        );
        assert_eq!(
            Sequence::pick(Some("xterm-256color"), Some("Apple_Terminal"), false, false),
            Sequence::Bell
        );
    }

    #[test]
    fn each_sequence_reads_as_its_terminals_expect() {
        let text = "shop-230 waits for you";

        assert_eq!(
            written(&Notifier::new(Sequence::Osc9, false), text),
            "\x1b]9;saaio: shop-230 waits for you\x07"
        );
        assert_eq!(
            written(&Notifier::new(Sequence::Osc777, false), text),
            "\x1b]777;notify;saaio;shop-230 waits for you\x07"
        );
        assert_eq!(written(&Notifier::new(Sequence::Bell, false), text), "\x07");
    }

    #[test]
    fn inside_tmux_an_osc_passes_through_with_every_escape_doubled() {
        assert_eq!(
            written(&Notifier::new(Sequence::Osc9, true), "hi"),
            "\x1bPtmux;\x1b\x1b]9;saaio: hi\x07\x1b\\"
        );
        assert_eq!(
            written(&Notifier::new(Sequence::Osc777, true), "hi"),
            "\x1bPtmux;\x1b\x1b]777;notify;saaio;hi\x07\x1b\\"
        );
    }

    #[test]
    fn inside_tmux_the_bell_goes_as_it_is() {
        assert_eq!(written(&Notifier::new(Sequence::Bell, true), "hi"), "\x07");
    }

    #[test]
    fn control_characters_never_end_the_sequence_early() {
        assert_eq!(
            written(
                &Notifier::new(Sequence::Osc9, false),
                "a\x07b\x1b]c\nd\u{9c}e"
            ),
            "\x1b]9;saaio: ab]cde\x07"
        );
    }

    #[test]
    fn a_long_text_is_cut() {
        let out = written(&Notifier::new(Sequence::Osc9, false), &"é".repeat(300));

        assert_eq!(out, format!("\x1b]9;saaio: {}\x07", "é".repeat(200)));
    }
}
