//! `saaio turn`: keeps a flow's turn, the locked file on its stdin, while its agent runs a heavy
//! command

use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};

use super::repo_dir;
use crate::cli::Env;
use crate::cli::output::Document;
use crate::exclusive::Exclusive;
use crate::fold;
use crate::processes::{self, Process};
use crate::repo_config::RepoConfig;
use crate::server_log;
use crate::store::Store;
use crate::turn_watch::{Keep, Look, Release, TurnWatch};
use crate::view::State;

/// How often the watch looks at the agent's processes.
const LOOK_STEP: Duration = Duration::from_millis(500);
/// Process lists in a row that may fail before the turn is let go.
const LIST_FAILURES: u32 = 3;

#[derive(Debug, clap::Args)]
pub struct Args {
    /// The flow whose turn it keeps
    #[arg(long)]
    flow: String,
    /// The pid of the flow's agent, whose processes it watches
    #[arg(long)]
    agent: u32,
    /// The processes between the hook and the agent: the harness itself, never a run
    #[arg(long, value_delimiter = ',')]
    harness: Vec<u32>,
}

/// `saaio turn`: looks every LOOK_STEP until TurnWatch lets the turn go, then exits, which closes
/// stdin and so frees the turn; says why in server.log. Prints nothing.
pub fn run(args: &Args, env: &Env, clock: fn() -> u64) -> Document {
    let cwd = std::env::current_dir().ok();
    let Ok(Some(store)) = env.store(cwd.as_deref()) else {
        return Document::Silent;
    };
    let log = |text: &str| {
        let line = format!("turn watch flow {}: {text}", args.flow);
        let _ = server_log::append(&env.server_log(&store), clock(), &line);
    };
    let exclusive = match exclusive(&store, &args.flow) {
        Ok(Some(exclusive)) => exclusive,
        Ok(None) => {
            log("the repo lists no heavy commands; the turn is let go");
            return Document::Silent;
        }
        Err(error) => {
            log(&format!("{error:#}; the turn is let go"));
            return Document::Silent;
        }
    };
    let mut watch = TurnWatch::new(clock());
    let mut failures = 0;
    loop {
        thread::sleep(LOOK_STEP);
        let all = match processes::list() {
            Ok(all) => {
                failures = 0;
                all
            }
            Err(error) => {
                failures += 1;
                if failures >= LIST_FAILURES {
                    log(&format!("{error:#}, {failures} times; the turn is let go"));
                    return Document::Silent;
                }
                continue;
            }
        };
        let asks = asks(&store, &args.flow);
        let found = look(&all, args, &exclusive, asks);
        if let Keep::Release(why) = watch.step(clock(), found) {
            log(match why {
                Release::AgentGone => "the agent is gone; the turn is let go",
                Release::RunEnded => "the run ended; the turn is let go",
                Release::NeverRan => "no run showed; the turn is let go",
            });
            return Document::Silent;
        }
    }
}

/// The heavy commands of the repository the flow works in.
fn exclusive(store: &Store, id: &str) -> Result<Option<Exclusive>> {
    let events = store.events(id)?;
    let flow = fold::fold(id, &events, None, 0).with_context(|| format!("no flow {id}"))?;
    let repo = repo_dir(&flow).with_context(|| format!("flow {id} has no workspace"))?;
    Ok(RepoConfig::load(&repo)?.exclusive)
}

/// Whether the flow waits on the user, a permission prompt among it.
fn asks(store: &Store, id: &str) -> bool {
    store
        .events(id)
        .ok()
        .and_then(|events| fold::fold(id, &events, None, 0))
        .is_some_and(|flow| flow.state == State::Waiting)
}

/// One look: whether the agent lives and whether a process below it, the harness left out, runs
/// a listed command.
fn look(all: &[Process], args: &Args, exclusive: &Exclusive, asks: bool) -> Look {
    let agent_alive = all.iter().any(|process| process.pid == args.agent);
    let runs = processes::under(all, args.agent)
        .iter()
        .filter(|process| !args.harness.contains(&process.pid))
        .any(|process| exclusive.runs(&process.command).is_some());
    Look {
        agent_alive,
        runs,
        asks,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn process(pid: u32, ppid: u32, command: &str) -> Process {
        Process {
            pid,
            ppid,
            command: command.to_string(),
        }
    }

    fn args() -> Args {
        Args {
            flow: "f".into(),
            agent: 100,
            harness: vec![101],
        }
    }

    fn pint() -> Exclusive {
        Exclusive {
            commands: vec!["pint".into()],
        }
    }

    #[test]
    fn sees_a_listed_command_below_the_agent() {
        let all = [
            process(100, 1, "node codex"),
            process(101, 100, "codex 'fix it and run pint'"),
            process(102, 101, "/bin/zsh -c ./vendor/bin/pint"),
        ];
        let found = look(&all, &args(), &pint(), false);
        assert!(found.agent_alive);
        assert!(found.runs);
    }

    #[test]
    fn never_counts_the_harness_or_another_agent_as_a_run() {
        let all = [
            process(100, 1, "node codex"),
            process(101, 100, "codex 'fix it and run pint'"),
            process(200, 1, "./vendor/bin/pint"),
        ];
        let found = look(&all, &args(), &pint(), true);
        assert!(!found.runs);
        assert!(found.asks);
    }

    #[test]
    fn knows_when_the_agent_is_gone() {
        let all = [process(102, 1, "./vendor/bin/pint")];
        assert!(!look(&all, &args(), &pint(), false).agent_alive);
    }
}
