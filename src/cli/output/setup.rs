//! What `saaio setup` prints, and the setup outcome `saaio start` names

use std::fmt;

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

#[cfg(test)]
mod tests {
    use super::*;

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
