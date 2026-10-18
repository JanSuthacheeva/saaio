pub(crate) mod bug_report;
pub(crate) mod flow;
pub(crate) mod news;
pub(crate) mod remove;
pub(crate) mod start;
pub(crate) mod status;

#[cfg(test)]
mod fixtures;

use serde::Serialize;

pub(crate) const DESCRIPTION: &str =
    "Tracks parallel feature flows: what each is doing, where it is blocked, how far it got";

/// List views cut long texts to this many chars, with a hint; --full lifts it.
pub(crate) const LIST_TEXT_LIMIT: usize = 80;

/// What a successful command prints: TOON for agents, JSON for programs, a line for people (saaio ui).
#[derive(Debug, PartialEq, Eq)]
pub enum Document {
    Toon(String),
    Json(String),
    Text(String),
    /// News that wakes a Claude Code session: printed on stderr, exit 2 (`saaio wait`).
    Wake(String),
    /// Nothing at all: what `saaio hook` answers, since a hook's stdout can reach the model.
    Silent,
}

/// An error an agent can act on: a message and next steps, printed as TOON on stdout.
#[derive(Debug, PartialEq, Eq)]
pub struct Failure {
    pub message: String,
    pub help: Vec<String>,
    pub code: u8,
}

impl Document {
    #[must_use]
    pub fn text(&self) -> &str {
        match self {
            Document::Toon(text)
            | Document::Json(text)
            | Document::Text(text)
            | Document::Wake(text) => text,
            Document::Silent => "",
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

    /// Exit code 2: clap's message, with what it lists below a closing colon (the missing
    /// arguments), and where to read the usage: the command of clap's usage line, else
    /// `subcommand` (clap prints none for a flag missing its value).
    #[must_use]
    pub fn usage(clap_error: &clap::Error, subcommand: Option<&str>) -> Failure {
        let rendered = clap_error.to_string();
        let mut lines = rendered.lines();
        let mut message = lines
            .next()
            .unwrap_or_default()
            .trim_start_matches("error: ")
            .to_string();
        if message.ends_with(':') {
            let listed: Vec<&str> = lines
                .take_while(|line| line.starts_with(char::is_whitespace))
                .map(str::trim)
                .collect();
            message = format!("{message} {}", listed.join(", "));
        }
        let command = rendered
            .lines()
            .find_map(|line| line.strip_prefix("Usage: "))
            .map(|usage| {
                usage
                    .split_whitespace()
                    .take_while(|word| !word.starts_with(['[', '<', '-']))
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .unwrap_or_else(|| match subcommand {
                Some(name) => format!("saaio {name}"),
                None => "saaio".to_string(),
            });
        Failure {
            message,
            help: vec![format!("Run `{command} --help` for the usage")],
            code: 2,
        }
    }

    /// error: ..., help[n]: ...
    #[must_use]
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

/// Exit code 1: the error chain as the message.
impl From<anyhow::Error> for Failure {
    fn from(error: anyhow::Error) -> Failure {
        Failure::error(format!("{error:#}"), Vec::new())
    }
}

impl From<std::io::Error> for Failure {
    fn from(error: std::io::Error) -> Failure {
        Failure::from(anyhow::Error::from(error))
    }
}

/// Text cut to `limit` chars with " (truncated, N chars - use --full)" unless `full`.
pub(crate) fn truncated(text: &str, limit: usize, full: bool) -> String {
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
    #[command(name = "saaio")]
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
        Start {
            #[arg(required_unless_present = "plain")]
            request: Option<String>,
            #[arg(long, conflicts_with = "request")]
            plain: Option<String>,
        },
    }

    #[test]
    fn prints_an_error_with_next_steps_as_toon() {
        let failure = Failure::error(
            "no flow named shop-999",
            vec!["Run `saaio` to list the flows".into()],
        );

        assert_eq!(failure.code, 1);
        assert_eq!(
            failure.to_toon(),
            "error: no flow named shop-999\nhelp[1]: Run `saaio` to list the flows"
        );
    }

    #[test]
    fn turns_a_clap_error_into_a_usage_failure_naming_the_command() {
        Cli::command().debug_assert();
        let error = Cli::try_parse_from(["saaio", "report", "--nope"]).unwrap_err();

        let failure = Failure::usage(&error, None);

        assert_eq!(failure.code, 2);
        assert_eq!(failure.message, "unexpected argument '--nope' found");
        assert_eq!(
            failure.help,
            vec!["Run `saaio report --help` for the usage"]
        );
    }

    #[test]
    fn names_the_missing_arguments_clap_lists_below_its_message() {
        let error = Cli::try_parse_from(["saaio", "start"]).unwrap_err();

        let failure = Failure::usage(&error, None);

        assert_eq!(
            failure.message,
            "the following required arguments were not provided: <REQUEST>"
        );
        assert_eq!(failure.help, vec!["Run `saaio start --help` for the usage"]);
    }

    #[test]
    fn names_the_subcommand_when_clap_prints_no_usage_line() {
        let error = Cli::try_parse_from(["saaio", "report", "--flow"]).unwrap_err();

        assert_eq!(
            Failure::usage(&error, Some("report")).help,
            vec!["Run `saaio report --help` for the usage"]
        );
        assert_eq!(
            Failure::usage(&error, None).help,
            vec!["Run `saaio --help` for the usage"]
        );
    }

    #[test]
    fn names_the_command_without_the_flags_clap_puts_in_its_usage() {
        let error = Cli::try_parse_from(["saaio", "start", "--plain", "x", "req"]).unwrap_err();

        let failure = Failure::usage(&error, None);

        assert_eq!(failure.help, vec!["Run `saaio start --help` for the usage"]);
    }

    #[test]
    fn names_the_whole_error_chain_for_io_failures() {
        let error = anyhow::anyhow!("Permission denied").context("could not create flow in /x");

        let failure = Failure::from(error);

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
