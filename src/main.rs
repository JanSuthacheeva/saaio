//! The process boundary: `Cli::try_parse`, the environment into `cli::Env`, the clock, exit codes

use std::env;
use std::io::{self, IsTerminal, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::SystemTime;

use clap::Parser;
use clap::error::ErrorKind;

use saaio::cli::output::{Document, Failure};
use saaio::cli::{self, Cli, Env};

fn main() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error)
            if matches!(
                error.kind(),
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
            ) =>
        {
            let _ = error.print();
            return ExitCode::SUCCESS;
        }
        Err(error) => {
            let args: Vec<_> = env::args_os().collect();
            return fail(&Failure::usage(
                &error,
                Cli::subcommand_in(&args).as_deref(),
            ));
        }
    };
    answer(cli::run(cli, &env(), now_ms))
}

/// The environment variables and the terminal saaio runs in, read once here.
#[allow(
    clippy::disallowed_methods,
    reason = "the one place the environment is read"
)]
fn env() -> Env {
    let path = |name| env::var_os(name).map(PathBuf::from);
    Env {
        saaio_home: path("SAAIO_HOME"),
        saaio_config: path("SAAIO_CONFIG"),
        flow_id: env::var("SAAIO_FLOW_ID").ok(),
        saaio_master: env::var("SAAIO_MASTER").is_ok_and(|value| value == "1"),
        saaio_proof: env::var("SAAIO_PROOF").ok(),
        home: path("HOME"),
        xdg_state_home: path("XDG_STATE_HOME"),
        xdg_config_home: path("XDG_CONFIG_HOME"),
        xdg_runtime_dir: path("XDG_RUNTIME_DIR"),
        tmpdir: path("TMPDIR"),
        claude_config_dir: path("CLAUDE_CONFIG_DIR"),
        codex_home: path("CODEX_HOME"),
        saaio_releases_url: env::var("SAAIO_RELEASES_URL").ok(),
        terminal: io::stdin().is_terminal() && io::stdout().is_terminal(),
        agent_shell: env::var_os("CLAUDECODE").is_some()
            || env::var_os("CODEX_THREAD_ID").is_some(),
        ssh: env::var_os("SSH_CONNECTION").is_some() || env::var_os("SSH_TTY").is_some(),
        wayland: env::var_os("WAYLAND_DISPLAY").is_some(),
        x11: env::var_os("DISPLAY").is_some(),
    }
}

fn now_ms() -> u64 {
    cli::unix_ms(SystemTime::now())
}

/// Prints a command's answer where it belongs and exits with its code.
fn answer(result: Result<Document, Failure>) -> ExitCode {
    match result {
        Ok(Document::Silent) => ExitCode::SUCCESS,
        // Exit 2 with the news on stderr is what makes Claude Code wake the session with it.
        Ok(Document::Wake(news)) => {
            eprintln!("{news}");
            ExitCode::from(2)
        }
        Ok(document) => print(document.text(), ExitCode::SUCCESS),
        Err(failure) => fail(&failure),
    }
}

fn fail(failure: &Failure) -> ExitCode {
    print(&failure.to_toon(), ExitCode::from(failure.code))
}

/// Writes `text` and a line break to stdout, then exits with `code`. A reader that went away, as
/// `saaio status | head -1` does, is not an error; any other failure to write is.
fn print(text: &str, code: ExitCode) -> ExitCode {
    match writeln!(io::stdout().lock(), "{text}") {
        Ok(()) => code,
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => code,
        Err(error) => {
            eprintln!("saaio: could not write to stdout: {error}");
            ExitCode::FAILURE
        }
    }
}
