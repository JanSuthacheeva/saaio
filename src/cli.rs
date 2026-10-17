mod commands;
mod env;
pub mod output;

pub use crate::event::unix_ms;
pub use env::Env;

use clap::{Parser, Subcommand};

use commands::{clear, home, hook, new, remove, report, send, server, start, status, ui, wait};
use output::{Document, Failure};

/// Tracks parallel feature flows: what each is doing, where it is blocked, how far it got.
#[derive(Debug, Parser)]
#[command(version, about)]
pub struct Cli {
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
    /// Start a worker: a flow, a worktree, the setup hook and an agent flow ui opens
    Start(start::Args),
    /// Stop a flow's worker, tear down its worktree and forget it
    Remove(remove::Args),
    /// Tell a flow's worker something: an answer, an instruction, a correction
    Send(send::Args),
    /// Forget flows that are done or ended
    Clear,
    /// Open the terminal UI: the master, every worker and the board
    Ui(ui::Args),
    /// What a session's asyncRewake hook runs: sleeps until there is news for it, prints it, exits 2
    #[command(hide = true)]
    Wait(wait::Args),
    /// The session server flow ui starts on its own
    #[command(hide = true)]
    Server,
    /// What a harness hook runs: reports the event on stdin into $FLOW_ID's flow
    #[command(hide = true)]
    Hook(hook::Args),
}

/// Runs the command `cli` names: what to print, or the failure to print instead.
///
/// # Errors
///
/// The command's failure, with its message, next steps and exit code. Hook and Wait run without
/// a store; every other command fails without one.
pub fn run(cli: Cli, env: &Env, clock: fn() -> u64) -> Result<Document, Failure> {
    match &cli.command {
        Some(Command::Hook(args)) => return Ok(hook::run(args, env, clock)),
        Some(Command::Wait(args)) => return Ok(wait::run(args, env, clock)),
        _ => {}
    }
    let Some(store) = env.store() else {
        return Err(Failure::error(
            "cannot find a state directory",
            vec!["Set FLOW_HOME to a directory".to_string()],
        ));
    };
    let Some(registry) = env.registry() else {
        return Err(Failure::error(
            "cannot find a state directory",
            vec!["Set HOME or XDG_STATE_HOME to a directory".to_string()],
        ));
    };
    // Only for the overview and the slots; a command works without it.
    let _ = registry.register(&store);
    let now = clock();
    let store = &store;
    let registry = &registry;
    match cli.command {
        None => home::run(store, &env.bin_path(), now),
        Some(Command::New(args)) => new::run(args, store, &env.config()?.workflow, now),
        Some(Command::Report(args)) => report::run(args, store, now),
        Some(Command::Status(args)) => status::run(&args, store, registry, env, now),
        Some(Command::Start(args)) => {
            let config = env.config()?;
            let cwd = std::env::current_dir()?;
            start::run(&args, store, registry, &config, &cwd, env.user_home(), now)
        }
        Some(Command::Remove(args)) => remove::run(&args, store, now),
        Some(Command::Send(args)) => send::run(&args, store, &env.config()?, env.flow_id(), now),
        Some(Command::Clear) => clear::run(store, now),
        Some(Command::Ui(args)) => ui::run(&args, store, env, clock),
        Some(Command::Server) => server::run(store.clone(), env, clock),
        Some(Command::Hook(_) | Command::Wait(_)) => Ok(Document::Silent),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_without_a_store(args: &[&str]) -> Result<Document, Failure> {
        let cli = Cli::try_parse_from(std::iter::once("flow").chain(args.iter().copied())).unwrap();
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
            vec!["Set FLOW_HOME to a directory".to_string()]
        );
        assert_eq!(failure.code, 1);
    }
}
