//! saaio-site: builds the saaio.dev docs site from the Markdown in a checkout.

#![cfg_attr(test, allow(clippy::unwrap_used, reason = "tests unwrap directly"))]

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;

use site::Failure;

#[cfg(test)]
mod fixtures;
mod layout;
mod links;
mod output;
mod pages;
mod render;
mod site;
mod slug;

/// Builds the saaio.dev docs site from the Markdown in a checkout.
#[derive(Parser)]
struct Args {
    /// The checkout to read.
    #[arg(long, default_value = ".")]
    root: PathBuf,
    /// The folder to write the site to; empty, missing, or an earlier build.
    out: PathBuf,
}

#[expect(
    clippy::print_stdout,
    clippy::print_stderr,
    reason = "the site builder's process boundary prints"
)]
fn main() -> ExitCode {
    let args = match Args::try_parse() {
        Ok(args) => args,
        Err(error) => {
            let _ = error.print();
            return if error.use_stderr() {
                ExitCode::from(2)
            } else {
                ExitCode::SUCCESS
            };
        }
    };
    match site::build(&args.root, &args.out) {
        Ok(built) => {
            let assets = if built.assets == 1 { "asset" } else { "assets" };
            println!(
                "built {} pages and {} {assets} into {}",
                built.pages,
                built.assets,
                args.out.display()
            );
            ExitCode::SUCCESS
        }
        Err(Failure::Broken(broken)) => {
            for link in &broken {
                eprintln!("{link}");
            }
            eprintln!("{} broken links, nothing built", broken.len());
            ExitCode::FAILURE
        }
        Err(Failure::Io(error)) => {
            eprintln!("saaio-site: {error:#}");
            ExitCode::FAILURE
        }
    }
}
