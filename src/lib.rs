//! The module tree: `cli` public, `fuzzing` too with its feature, the rest crate-only; `commit`,
//! `fixtures`, `layering`, `strategies` in tests

#![cfg_attr(
    test,
    allow(
        clippy::disallowed_methods,
        clippy::unwrap_used,
        clippy::print_stderr,
        reason = "tests set up their files, read PATH, start threads, unwrap and print skips directly"
    )
)]

pub mod cli;

mod adapter;
mod binaries;
mod branching;
mod bug_report;
mod client;
#[cfg(test)]
#[path = "../build/commit.rs"]
mod commit;
mod config;
mod default_file;
mod deliver;
mod event;
mod exclusive;
#[cfg(test)]
mod fixtures;
mod fold;
mod forge;
#[cfg(feature = "fuzzing")]
pub mod fuzzing;
#[cfg(all(test, not(feature = "fuzzing")))]
mod fuzzing;
mod id;
mod identity;
mod launch;
#[cfg(test)]
mod layering;
mod pause;
mod private;
mod processes;
mod program_status;
mod proto;
mod refusal;
mod registry;
mod release;
mod repo_config;
mod server;
mod server_log;
mod slots;
mod stack;
mod store;
#[cfg(test)]
mod strategies;
mod system;
mod text;
mod threads;
mod trust;
mod turn_watch;
mod turns;
mod ui;
mod update;
mod view;
mod wake;
mod workflow;
mod workspace;
