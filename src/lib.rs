#![cfg_attr(
    test,
    allow(
        clippy::disallowed_methods,
        reason = "tests set up their files and read PATH directly"
    )
)]

pub mod cli;

mod adapter;
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
#[cfg(test)]
mod fixtures;
mod fold;
mod forge;
mod launch;
#[cfg(test)]
mod layering;
mod pause;
mod private;
mod proto;
mod refusal;
mod registry;
mod server;
mod server_log;
mod slots;
mod store;
mod system;
mod text;
mod ui;
mod view;
mod wake;
mod workflow;
mod workspace;
