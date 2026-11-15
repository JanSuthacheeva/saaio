//! The command line: `Cli`, `Command` and `run`, which dispatches each command

mod commands;
mod context;
mod env;
pub mod output;

pub use crate::event::unix_ms;
pub use env::Env;

use clap::{Parser, Subcommand};

use commands::{
    bug_report, clear, completions, home, hook, new, remove, report, send, server, start, status,
    stop, trust, turn, ui, update, wait,
};
use context::Context;
use output::{Document, Failure};

/// Tracks parallel feature flows: what each is doing, where it is blocked, how far it got.
#[derive(Debug, Parser)]
#[command(
    version = crate::binaries::BUILD,
    about,
    after_help = "Run alone in a terminal, saaio opens the terminal UI.\nRun by an agent or a script, it prints every flow and what needs you.\nRun saaio --stop to stop the server and every session.",
    args_conflicts_with_subcommands = true
)]
pub struct Cli {
    /// Stop the server and every session in it
    #[arg(long)]
    stop: bool,
    #[command(subcommand)]
    command: Option<Command>,
}

impl Cli {
    /// The first of `args` that names a command (after the program), for a usage error clap
    /// gives without one.
    pub fn subcommand_in<T: AsRef<std::ffi::OsStr>>(args: &[T]) -> Option<String> {
        let command = <Cli as clap::CommandFactory>::command();
        args.iter()
            .skip(1)
            .filter_map(|arg| arg.as_ref().to_str())
            .find_map(|arg| command.find_subcommand(arg))
            .map(|found| found.get_name().to_string())
    }
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Track a new flow, or return the unfinished flow of that name
    New(new::Args),
    /// Append one event to a flow
    Report(report::Args),
    /// Every flow, or one flow with its phases, timeline and decisions
    Status(status::Args),
    /// Start a worker: a flow, a worktree, the setup hook and an agent the UI opens
    Start(start::Args),
    /// Stop a flow's worker, tear down its worktree and forget it
    Remove(remove::Args),
    /// Tell a flow's worker something: an answer, an instruction, a correction
    Send(send::Args),
    /// Forget flows that are done or ended
    Clear,
    /// Write a file for a bug report: versions, settings, the store and the end of server.log
    BugReport,
    /// Print the completion script for a shell
    Completions(completions::Args),
    /// Let saaio run a repository's .saaio scripts and Codex trust its worktrees; --revoke, --list
    Trust(trust::Args),
    /// Install the latest release over this binary, or the release --version names
    Update(update::Args),
    /// What a session's asyncRewake hook runs: sleeps until there is news for it, prints it, exits 2
    #[command(hide = true)]
    Wait(wait::Args),
    /// The session server saaio starts on its own
    #[command(hide = true)]
    Server,
    /// What a harness hook runs: reports the event on stdin into $SAAIO_FLOW_ID's flow
    #[command(hide = true)]
    Hook(hook::Args),
    /// What a hook starts once its flow took the project's turn: keeps it while the run lasts
    #[command(hide = true)]
    Turn(turn::Args),
}

/// Runs the command `cli` names: what to print, or the failure to print instead.
///
/// # Errors
///
/// The command's failure, with its message, next steps and exit code. A command that needs the
/// store fails without one.
pub fn run(cli: Cli, env: &Env, clock: fn() -> u64) -> Result<Document, Failure> {
    let ctx = Context { env, clock };
    match cli.command {
        None if cli.stop => stop::run(&ctx),
        None if env.is_person() => ui::run(&ctx),
        None => home::run(&ctx),
        Some(Command::New(args)) => new::run(args, &ctx),
        Some(Command::Report(args)) => report::run(args, &ctx),
        Some(Command::Status(args)) => status::run(&args, &ctx),
        Some(Command::Start(args)) => start::run(&args, &ctx),
        Some(Command::Remove(args)) => remove::run(&args, &ctx),
        Some(Command::Send(args)) => send::run(&args, &ctx),
        Some(Command::Clear) => clear::run(&ctx),
        Some(Command::BugReport) => bug_report::run(&ctx),
        Some(Command::Completions(args)) => Ok(completions::run(&args)),
        Some(Command::Trust(args)) => trust::run(&args, &ctx),
        Some(Command::Update(args)) => update::run(&args, &ctx),
        Some(Command::Wait(args)) => Ok(wait::run(&args, &ctx)),
        Some(Command::Server) => server::run(&ctx),
        Some(Command::Hook(args)) => Ok(hook::run(&args, &ctx)),
        Some(Command::Turn(args)) => Ok(turn::run(&args, &ctx)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_without_a_store(args: &[&str]) -> Result<Document, Failure> {
        let cli =
            Cli::try_parse_from(std::iter::once("saaio").chain(args.iter().copied())).unwrap();
        run(cli, &Env::default(), || 0)
    }

    #[test]
    fn hook_and_wait_answer_nothing_without_a_store() {
        assert_eq!(
            run_without_a_store(&["hook", "claude-code"]),
            Ok(Document::Silent)
        );
        assert_eq!(run_without_a_store(&["wait"]), Ok(Document::Silent));
        assert_eq!(
            run_without_a_store(&["wait", "--master"]),
            Ok(Document::Silent)
        );
    }

    #[test]
    fn every_other_command_needs_a_store() {
        let failure = run_without_a_store(&["status"]).unwrap_err();

        assert_eq!(failure.message, "cannot find a state directory");
        assert_eq!(
            failure.help,
            vec!["Set SAAIO_HOME to a directory".to_string()]
        );
        assert_eq!(failure.code, 1);
    }
}
