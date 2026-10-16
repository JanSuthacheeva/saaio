use std::env;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::SystemTime;

use clap::Parser;
use clap::error::ErrorKind;

use flow::cli::output::{Document, Failure};
use flow::cli::{self, Cli, Env};

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
        Err(error) => return fail(&Failure::usage(&error)),
    };
    answer(cli::run(cli, &env(), now_ms))
}

/// The environment variables flow reads, read once here.
fn env() -> Env {
    let path = |name| env::var_os(name).map(PathBuf::from);
    Env {
        flow_home: path("FLOW_HOME"),
        flow_config: path("FLOW_CONFIG"),
        flow_id: env::var("FLOW_ID").ok(),
        home: path("HOME"),
        xdg_state_home: path("XDG_STATE_HOME"),
        xdg_config_home: path("XDG_CONFIG_HOME"),
        claude_config_dir: path("CLAUDE_CONFIG_DIR"),
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
        Ok(document) => {
            println!("{}", document.text());
            ExitCode::SUCCESS
        }
        Err(failure) => fail(&failure),
    }
}

fn fail(failure: &Failure) -> ExitCode {
    println!("{}", failure.to_toon());
    ExitCode::from(failure.code)
}
