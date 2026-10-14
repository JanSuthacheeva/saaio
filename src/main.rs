#[allow(dead_code)]
mod event;

use std::process::ExitCode;

use clap::Parser;

/// Tracks parallel feature flows: what each is doing, where it is blocked, how far it got.
#[derive(Debug, Parser)]
#[command(version, about)]
struct Cli {}

fn main() -> ExitCode {
    Cli::parse();
    ExitCode::SUCCESS
}
