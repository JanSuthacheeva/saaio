#[allow(dead_code)]
mod event;
#[allow(dead_code)]
mod fold;
#[allow(dead_code)]
mod output;
#[allow(dead_code)]
mod store;
#[allow(dead_code)]
mod view;

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
