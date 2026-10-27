//! `Notifier`: desktop notifications through the terminal, by OSC 9, OSC 777 or its bell

use std::io::Write;
use std::time::Duration;

use anyhow::{Context, Result};

use crate::text;
use crate::workspace::process;

/// The most characters a notification says; a terminal shows less anyway.
const TEXT_LIMIT: usize = 200;
/// How long tmux may take to name its client's terminal.
const TMUX_LIMIT: Duration = Duration::from_millis(500);

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
    /// From what the terminal sets itself first: TERM (alacritty* Bell, foot* / rxvt* Osc777,
    /// xterm-ghostty, xterm-kitty, wezterm Osc9), then TERM_PROGRAM (Apple_Terminal Bell, iTerm.app,
    /// WezTerm, ghostty Osc9); only then $VTE_VERSION (Osc777) and $ALACRITTY_WINDOW_ID (Bell), which
    /// a tmux server or a terminal started from another one inherits stale; else Osc9.
    pub fn pick(term: Option<&str>, program: Option<&str>, vte: bool, alacritty: bool) -> Sequence {
        if let Some(sequence) = term.and_then(by_term) {
            return sequence;
        }
        match program {
            Some("Apple_Terminal") => Sequence::Bell,
            Some("iTerm.app" | "WezTerm" | "ghostty") => Sequence::Osc9,
            _ if vte => Sequence::Osc777,
            _ if alacritty => Sequence::Bell,
            _ => Sequence::Osc9,
        }
    }

    /// For the terminal attached to tmux: by its TERM as for pick, else by the version it answered
    /// (ghostty, iTerm2, WezTerm, kitty Osc9; foot, VTE Osc777). One that answered none, such as
    /// Alacritty or Terminal.app, gets Bell; any other Osc9.
    pub fn for_client(client: &TmuxClient) -> Sequence {
        if let Some(sequence) = by_term(&client.term) {
            return sequence;
        }
        let version = client.version.as_str();
        if version.is_empty() {
            Sequence::Bell
        } else if ["foot", "VTE"].iter().any(|name| version.starts_with(name)) {
            Sequence::Osc777
        } else {
            Sequence::Osc9
        }
    }
}

/// The sequence a TERM names: alacritty* Bell, foot* / rxvt* Osc777, xterm-ghostty, xterm-kitty,
/// wezterm Osc9; None for a TERM many terminals use, such as xterm-256color.
fn by_term(term: &str) -> Option<Sequence> {
    if term.starts_with("alacritty") {
        Some(Sequence::Bell)
    } else if term.starts_with("foot") || term.starts_with("rxvt") {
        Some(Sequence::Osc777)
    } else if ["xterm-ghostty", "xterm-kitty", "wezterm"].contains(&term) {
        Some(Sequence::Osc9)
    } else {
        None
    }
}

/// The terminal attached to this tmux session, as tmux knows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TmuxClient {
    /// Its TERM: `xterm-ghostty`.
    pub term: String,
    /// What it answered when tmux asked its version (XTVERSION): `ghostty 1.3.1`; empty for none.
    pub version: String,
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

    /// Whether saaio runs inside tmux, whose client's terminal decides the sequence.
    pub fn in_tmux(&self) -> bool {
        self.tmux
    }

    /// Writes the notification for `text` (control characters dropped, cut to 200 chars) and
    /// flushes; Err when the terminal cannot be written. Inside tmux, `client` is the terminal
    /// attached to it (`tmux_client()`), else the environment's pick stands.
    pub fn notify(
        &self,
        out: &mut impl Write,
        text: &str,
        client: Option<&TmuxClient>,
    ) -> Result<()> {
        let picked = match client {
            Some(client) if self.tmux => Sequence::for_client(client),
            _ => self.sequence,
        };
        let text: String = text::printable(text).chars().take(TEXT_LIMIT).collect();
        let written = sequence(picked, &text);
        let written = if self.tmux && picked != Sequence::Bell {
            passthrough(&written)
        } else {
            written
        };
        out.write_all(written.as_bytes())
            .and_then(|()| out.flush())
            .context("could not write the notification to the terminal")
    }
}

/// The terminal attached to this tmux session; None when tmux does not answer within 500 ms. It
/// blocks, so the UI asks on a thread of its own.
pub fn tmux_client() -> Option<TmuxClient> {
    let format = "#{client_termname}\t#{client_termtype}";
    let argv = ["tmux", "display-message", "-p", format].map(String::from);
    client_of(&argv)
}

fn client_of(argv: &[String]) -> Option<TmuxClient> {
    let answer = process::output(argv, TMUX_LIMIT).ok()?;
    let (term, version) = answer.trim_end_matches('\n').split_once('\t')?;
    (!term.is_empty()).then(|| TmuxClient {
        term: term.to_string(),
        version: version.trim().to_string(),
    })
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
        written_for(notifier, text, None)
    }

    fn written_for(notifier: &Notifier, text: &str, client: Option<&TmuxClient>) -> String {
        let mut out = Vec::new();
        notifier.notify(&mut out, text, client).unwrap();
        String::from_utf8(out).unwrap()
    }

    fn client(term: &str, version: &str) -> TmuxClient {
        TmuxClient {
            term: term.to_string(),
            version: version.to_string(),
        }
    }

    fn sh(script: &str) -> Vec<String> {
        vec!["sh".into(), "-c".into(), script.into()]
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
    fn what_the_terminal_sets_itself_wins_over_inherited_variables() {
        assert_eq!(
            Sequence::pick(Some("xterm-ghostty"), Some("ghostty"), false, true),
            Sequence::Osc9
        );
        assert_eq!(
            Sequence::pick(Some("xterm-256color"), Some("iTerm.app"), true, true),
            Sequence::Osc9
        );
        assert_eq!(
            Sequence::pick(Some("alacritty"), Some("ghostty"), false, false),
            Sequence::Bell
        );
        assert_eq!(
            Sequence::pick(Some("xterm-kitty"), None, true, false),
            Sequence::Osc9
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
    fn inside_tmux_the_attached_clients_terminal_decides() {
        let stale = Notifier::new(Sequence::Bell, true);
        let ghostty = Notifier::new(Sequence::Osc9, true);

        assert_eq!(
            written_for(
                &stale,
                "hi",
                Some(&client("xterm-ghostty", "ghostty 1.3.1"))
            ),
            "\x1bPtmux;\x1b\x1b]9;saaio: hi\x07\x1b\\"
        );
        assert_eq!(
            written_for(&ghostty, "hi", Some(&client("alacritty", ""))),
            "\x07"
        );
        assert_eq!(written_for(&stale, "hi", None), "\x07");
    }

    #[test]
    fn a_client_with_a_common_term_is_known_by_its_version() {
        for (version, sequence) in [
            ("iTerm2 3.5.14", Sequence::Osc9),
            ("WezTerm 20240203", Sequence::Osc9),
            ("kitty(0.42.1)", Sequence::Osc9),
            ("foot(1.21.0)", Sequence::Osc777),
            ("VTE(8000)", Sequence::Osc777),
            ("", Sequence::Bell),
        ] {
            assert_eq!(
                Sequence::for_client(&client("xterm-256color", version)),
                sequence,
                "{version}"
            );
        }
    }

    #[test]
    fn outside_tmux_a_client_changes_nothing() {
        assert_eq!(
            written_for(
                &Notifier::new(Sequence::Bell, false),
                "hi",
                Some(&client("xterm-ghostty", "ghostty 1.3.1"))
            ),
            "\x07"
        );
    }

    #[test]
    fn the_client_is_what_tmux_says_and_none_when_it_cannot() {
        assert_eq!(
            client_of(&sh("printf 'xterm-ghostty\\tghostty 1.3.1\\n'")),
            Some(client("xterm-ghostty", "ghostty 1.3.1"))
        );
        assert_eq!(
            client_of(&sh("printf 'xterm-256color\\t\\n'")),
            Some(client("xterm-256color", ""))
        );
        assert_eq!(client_of(&sh("printf '\\t\\n'")), None);
        assert_eq!(client_of(&sh("echo no server >&2; exit 1")), None);
    }

    #[test]
    fn a_tmux_that_hangs_is_given_up_on() {
        let started = std::time::Instant::now();

        assert_eq!(client_of(&sh("sleep 10")), None);
        assert!(started.elapsed() < Duration::from_secs(5));
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
