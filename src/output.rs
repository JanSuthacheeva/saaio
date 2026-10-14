use serde::Serialize;

/// List views cut long texts to this many chars, with a hint; --full lifts it.
pub const LIST_TEXT_LIMIT: usize = 80;

/// What a successful command prints: TOON for agents or JSON for programs.
#[derive(Debug, PartialEq)]
pub enum Document {
    Toon(String),
    Json(String),
}

/// An error an agent can act on: a message and next steps, printed as TOON on stdout.
#[derive(Debug, PartialEq)]
pub struct Failure {
    pub message: String,
    pub help: Vec<String>,
    pub code: u8,
}

impl Document {
    pub fn text(&self) -> &str {
        match self {
            Document::Toon(text) | Document::Json(text) => text,
        }
    }
}

impl Failure {
    /// Exit code 1.
    pub fn error(message: impl Into<String>, help: Vec<String>) -> Failure {
        Failure {
            message: message.into(),
            help,
            code: 1,
        }
    }

    /// Exit code 2: clap's message and where to read the usage.
    pub fn usage(clap_error: &clap::Error) -> Failure {
        let rendered = clap_error.to_string();
        let message = rendered
            .lines()
            .next()
            .unwrap_or_default()
            .trim_start_matches("error: ")
            .to_string();
        let command = rendered
            .lines()
            .find_map(|line| line.strip_prefix("Usage: "))
            .map(|usage| {
                usage
                    .split_whitespace()
                    .take_while(|word| !word.starts_with(['[', '<']))
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .unwrap_or_else(|| "flow".to_string());
        Failure {
            message,
            help: vec![format!("Run `{command} --help` for the usage")],
            code: 2,
        }
    }

    /// Exit code 1: the error chain as the message.
    pub fn from_io(error: anyhow::Error) -> Failure {
        Failure::error(format!("{error:#}"), Vec::new())
    }

    /// error: ..., help[n]: ...
    pub fn to_toon(&self) -> String {
        #[derive(Serialize)]
        struct ErrorDocument<'a> {
            error: &'a str,
            #[serde(skip_serializing_if = "<[String]>::is_empty")]
            help: &'a [String],
        }
        toon(&ErrorDocument {
            error: &self.message,
            help: &self.help,
        })
    }
}

/// 45s, 12m, 3h, 2d.
pub fn short_duration(ms: u64) -> String {
    let seconds = ms / 1_000;
    match seconds {
        0..60 => format!("{seconds}s"),
        60..3_600 => format!("{}m", seconds / 60),
        3_600..86_400 => format!("{}h", seconds / 3_600),
        _ => format!("{}d", seconds / 86_400),
    }
}

/// Text cut to `limit` chars with " (truncated, N chars - use --full)" unless `full`.
pub fn truncated(text: &str, limit: usize, full: bool) -> String {
    let length = text.chars().count();
    if full || length <= limit {
        return text.to_string();
    }
    let kept: String = text.chars().take(limit).collect();
    format!("{kept} (truncated, {length} chars - use --full)")
}

fn toon<T: Serialize>(document: &T) -> String {
    serde_toon::to_string(document).expect("a document always encodes")
}

#[cfg(test)]
mod tests {
    use clap::{CommandFactory, Parser, Subcommand};

    use super::*;

    #[derive(Debug, Parser)]
    #[command(name = "flow")]
    struct Cli {
        #[command(subcommand)]
        command: Command,
    }

    #[derive(Debug, Subcommand)]
    enum Command {
        Report {
            #[arg(long)]
            flow: Option<String>,
        },
    }

    #[test]
    fn prints_an_error_with_next_steps_as_toon() {
        let failure = Failure::error(
            "no flow named shop-999",
            vec!["Run `flow` to list the flows".into()],
        );

        assert_eq!(failure.code, 1);
        assert_eq!(
            failure.to_toon(),
            "error: no flow named shop-999\nhelp[1]: Run `flow` to list the flows"
        );
    }

    #[test]
    fn turns_a_clap_error_into_a_usage_failure_naming_the_command() {
        Cli::command().debug_assert();
        let error = Cli::try_parse_from(["flow", "report", "--nope"]).unwrap_err();

        let failure = Failure::usage(&error);

        assert_eq!(failure.code, 2);
        assert_eq!(failure.message, "unexpected argument '--nope' found");
        assert_eq!(failure.help, vec!["Run `flow report --help` for the usage"]);
    }

    #[test]
    fn names_the_whole_error_chain_for_io_failures() {
        let error = anyhow::anyhow!("Permission denied").context("could not create flow in /x");

        let failure = Failure::from_io(error);

        assert_eq!(failure.code, 1);
        assert_eq!(
            failure.message,
            "could not create flow in /x: Permission denied"
        );
        assert_eq!(
            failure.to_toon(),
            "error: \"could not create flow in /x: Permission denied\""
        );
    }

    #[test]
    fn shortens_durations_to_one_unit() {
        assert_eq!(short_duration(0), "0s");
        assert_eq!(short_duration(45_000), "45s");
        assert_eq!(short_duration(12 * 60_000 + 59_000), "12m");
        assert_eq!(short_duration(3 * 3_600_000), "3h");
        assert_eq!(short_duration(49 * 3_600_000), "2d");
    }

    #[test]
    fn truncates_long_texts_with_a_hint_unless_full() {
        let long = "x".repeat(LIST_TEXT_LIMIT + 5);

        assert_eq!(
            truncated(&long, LIST_TEXT_LIMIT, false),
            format!(
                "{} (truncated, 85 chars - use --full)",
                "x".repeat(LIST_TEXT_LIMIT)
            )
        );
        assert_eq!(truncated(&long, LIST_TEXT_LIMIT, true), long);
        assert_eq!(truncated("short", LIST_TEXT_LIMIT, false), "short");
    }
}
