//! What `saaio setup` prints, and the setup outcome `saaio start` names

use std::fmt;
use std::path::Path;

use serde::Serialize;

use super::remove::Teardown;
use super::{Document, Failure, toon};
use crate::view::FlowView;

/// What a run of the repo's .saaio/setup came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Setup {
    Ran { log: String },
    None,
    Failed { reason: String, log: String },
}

/// ran, none, failed: <reason> (full output: <log>)
impl fmt::Display for Setup {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Setup::Ran { .. } => f.write_str("ran"),
            Setup::None => f.write_str("none"),
            Setup::Failed { reason, log } => write!(f, "failed: {reason} (full output: {log})"),
        }
    }
}

/// flow, teardown, setup, log, help.
pub(crate) fn reran(flow: &FlowView, teardown: &Teardown, setup: &Setup) -> Document {
    #[derive(Serialize)]
    struct Reran<'a> {
        flow: &'a str,
        teardown: String,
        setup: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        log: Option<&'a str>,
        help: Vec<String>,
    }
    let log = match setup {
        Setup::Ran { log } | Setup::Failed { log, .. } => Some(log.as_str()),
        Setup::None => None,
    };
    Document::Toon(toon(&Reran {
        flow: &flow.name,
        teardown: teardown.to_string(),
        setup: setup.to_string(),
        log,
        help: vec![format!("Run `saaio status {}` to see the flow", flow.name)],
    }))
}

/// error "setup failed: <reason> (full output: <log>)", help: read the log, what the teardown
/// said when it failed, run it again once fixed.
pub(crate) fn failed(flow: &FlowView, teardown: &Teardown, reason: &str, log: &str) -> Failure {
    let mut help = vec!["Read its whole output in the log".to_string()];
    if let Teardown::Failed(why) = teardown {
        help.push(format!("Its teardown failed first: {why}"));
    }
    help.push(format!(
        "Run `saaio setup {}` again once the cause is fixed",
        flow.name
    ));
    Failure::error(format!("setup failed: {reason} (full output: {log})"), help)
}

/// Why `saaio setup` runs nothing for `name`: no worktree, no .saaio/setup, ...
pub(crate) fn refused(name: &str, why: &str) -> Failure {
    Failure::error(
        format!("{name} {why}"),
        [format!("Run `saaio status {name}` to see the flow")],
    )
}

/// The refusal to run the scripts of a repository the person has not trusted.
pub(crate) fn untrusted(name: &str, repo: &Path) -> Failure {
    let repo = repo.display();
    Failure::error(
        format!("{repo}, the repository of {name}, is not trusted"),
        [format!(
            "Ask the person to read its .saaio scripts, then run `saaio trust` in {repo} in their terminal"
        )],
    )
}

#[cfg(test)]
mod tests {
    use super::super::fixtures::flow;
    use super::*;

    const LOG: &str = "/store/hooks/0b6e/setup.log";

    #[test]
    fn says_what_ran_and_where_the_output_is() {
        let flow = flow("shop-240", vec![], 0);

        let document = reran(&flow, &Teardown::Ran, &Setup::Ran { log: LOG.into() });

        assert_eq!(
            document.text(),
            format!(
                "flow: shop-240\nteardown: ran\nsetup: ran\nlog: {LOG}\nhelp[1]: Run `saaio status shop-240` to see the flow"
            )
        );
    }

    #[test]
    fn a_failure_names_its_log_and_a_failed_teardown() {
        let flow = flow("shop-240", vec![], 0);
        let teardown = Teardown::Failed("docker is not running".into());

        let failure = failed(&flow, &teardown, "Please run composer update", LOG);

        assert_eq!(
            failure.message,
            format!("setup failed: Please run composer update (full output: {LOG})")
        );
        assert_eq!(
            failure.help,
            [
                "Read its whole output in the log",
                "Its teardown failed first: docker is not running",
                "Run `saaio setup shop-240` again once the cause is fixed",
            ]
        );
        assert_eq!(failure.code, 1);
        let quiet = failed(&flow, &Teardown::Ran, "Please run composer update", LOG);
        assert_eq!(quiet.help.len(), 2);
    }

    #[test]
    fn a_failed_setup_names_its_log() {
        let log = "/store/hooks/0b6e/setup.log".to_string();

        assert_eq!(Setup::Ran { log: log.clone() }.to_string(), "ran");
        assert_eq!(Setup::None.to_string(), "none");
        assert_eq!(
            Setup::Failed {
                reason: "Please run composer update".into(),
                log,
            }
            .to_string(),
            "failed: Please run composer update (full output: /store/hooks/0b6e/setup.log)"
        );
    }
}
