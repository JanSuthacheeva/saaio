use std::io::Write;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;

/// Where a copy goes: the terminal through OSC 52, which also reaches over SSH, and the system
/// clipboard command when there is one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Clipboard {
    command: Option<&'static [&'static str]>,
}

impl Clipboard {
    /// pbcopy on macOS; wl-copy under Wayland, else xclip -selection clipboard under X11; none over SSH.
    pub fn new(ssh: bool, wayland: bool, x11: bool) -> Clipboard {
        let command: Option<&'static [&'static str]> = if ssh {
            None
        } else if cfg!(target_os = "macos") {
            Some(&["pbcopy"])
        } else if wayland {
            Some(&["wl-copy"])
        } else if x11 {
            Some(&["xclip", "-selection", "clipboard"])
        } else {
            None
        };
        Clipboard { command }
    }

    /// Writes osc52(text) to `out`, then pipes `text` to the command (waits for it); Err names what
    /// failed. Empty text, such as a double-click on a blank, copies nothing.
    pub fn copy(&self, out: &mut impl Write, text: &str) -> Result<()> {
        if text.is_empty() {
            return Ok(());
        }
        out.write_all(osc52(text).as_bytes())
            .and_then(|()| out.flush())
            .context("could not write OSC 52 to the terminal")?;
        let Some((program, args)) = self.command.and_then(<[_]>::split_first) else {
            return Ok(());
        };
        let mut child = Command::new(program)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .with_context(|| format!("could not run {program}"))?;
        // Waited for even when the write fails: a command that exited first says why in its status.
        let written = child
            .stdin
            .take()
            .map_or(Ok(()), |mut stdin| stdin.write_all(text.as_bytes()));
        let status = child
            .wait()
            .with_context(|| format!("could not wait for {program}"))?;
        if !status.success() {
            bail!("{program} failed: {status}");
        }
        written.with_context(|| format!("could not write to {program}"))
    }
}

/// ESC ] 52 ; c ; base64 BEL.
fn osc52(text: &str) -> String {
    format!("\x1b]52;c;{}\x07", STANDARD.encode(text))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn osc52_sets_the_clipboard_to_the_text_in_base64() {
        assert_eq!(osc52("hi"), "\x1b]52;c;aGk=\x07");
        assert_eq!(osc52(""), "\x1b]52;c;\x07");
    }

    #[test]
    fn over_ssh_there_is_no_command() {
        assert_eq!(Clipboard::new(true, true, true).command, None);
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn macos_uses_pbcopy() {
        assert_eq!(
            Clipboard::new(false, false, false).command,
            Some(&["pbcopy"][..])
        );
    }

    #[test]
    #[cfg(not(target_os = "macos"))]
    fn linux_prefers_wayland_then_x11() {
        assert_eq!(
            Clipboard::new(false, true, true).command,
            Some(&["wl-copy"][..])
        );
        assert_eq!(
            Clipboard::new(false, false, true).command,
            Some(&["xclip", "-selection", "clipboard"][..])
        );
        assert_eq!(Clipboard::new(false, false, false).command, None);
    }

    #[test]
    fn without_a_command_only_osc52_is_written() {
        let mut out = Vec::new();

        Clipboard { command: None }.copy(&mut out, "hi").unwrap();

        assert_eq!(out, b"\x1b]52;c;aGk=\x07");
    }

    #[test]
    fn nothing_to_copy_leaves_the_clipboard_alone() {
        let mut out = Vec::new();

        Clipboard {
            command: Some(&["false"]),
        }
        .copy(&mut out, "")
        .unwrap();

        assert!(out.is_empty());
    }

    #[test]
    fn a_failing_command_is_an_error() {
        let mut out = Vec::new();

        let error = Clipboard {
            command: Some(&["false"]),
        }
        .copy(&mut out, "hi")
        .unwrap_err();

        assert_eq!(error.to_string(), "false failed: exit status: 1");
        assert_eq!(out, b"\x1b]52;c;aGk=\x07");
    }

    #[test]
    fn a_command_that_fails_before_reading_is_named_by_its_status() {
        let text = "x".repeat(1 << 20);

        let error = Clipboard {
            command: Some(&["false"]),
        }
        .copy(&mut Vec::new(), &text)
        .unwrap_err();

        assert_eq!(error.to_string(), "false failed: exit status: 1");
    }
}
