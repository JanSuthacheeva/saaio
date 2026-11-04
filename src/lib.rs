//! The module tree: `cli` public, the rest crate-only; `commit`, `fixtures`, `layering` in tests

#![cfg_attr(
    test,
    allow(
        clippy::disallowed_methods,
        reason = "tests set up their files and read PATH directly"
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
mod identity;
mod launch;
#[cfg(test)]
mod layering;
mod pause;
mod private;
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "the turn watch reads it in a later commit")
)]
mod processes;
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
mod system;
mod text;
mod trust;
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "saaio turn watches in a later commit")
)]
mod turn_watch;
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "the hook takes turns in a later commit")
)]
mod turns;
mod ui;
mod update;
mod view;
mod wake;
mod workflow;
mod workspace;
