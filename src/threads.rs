//! Every thread saaio starts, each with a name, so a panic in it says where it happened

use std::io;
use std::thread::{self, JoinHandle};

/// A thread called `name` running `body`; Err when the system cannot start one.
pub fn spawn<T, F>(name: impl Into<String>, body: F) -> io::Result<JoinHandle<T>>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    #[allow(
        clippy::disallowed_methods,
        reason = "the one place a thread starts, with its name"
    )]
    thread::Builder::new().name(name.into()).spawn(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_body_runs_under_its_name_and_returns_its_value() {
        let handle = spawn("probe test", || {
            thread::current().name().map(str::to_string)
        })
        .unwrap();

        assert_eq!(handle.join().unwrap().as_deref(), Some("probe test"));
    }
}
