//! saaio-site: builds the saaio.dev docs site from the Markdown in a checkout.
#![allow(dead_code, reason = "temporary: site::build wires the modules in a later task")]

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;

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
    println!("nothing built into {} yet", args.out.display());
    let _ = args.root;
    ExitCode::SUCCESS
}
