//! `Refusal`: what `saaio remove` refuses to lose and `saaio start` to trust, the key its error
//! prints

use serde::{Deserialize, Serialize};

/// What `saaio remove` refuses to lose unless told to, and the repository `saaio start` refuses to
/// trust on its own; saaio ui asks the person instead. Printed as the error's `refusal` key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Refusal {
    /// The worktree has uncommitted work (--force discards it).
    Uncommitted,
    /// The branch has commits on no other branch (--delete-unmerged deletes them).
    Unmerged,
    /// The repository is not trusted to run its hooks (`saaio trust`).
    Untrusted,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_written_by_its_lowercase_name() {
        let all = [Refusal::Uncommitted, Refusal::Unmerged, Refusal::Untrusted];

        let names = serde_json::to_string(&all).unwrap();

        assert_eq!(names, r#"["uncommitted","unmerged","untrusted"]"#);
        assert_eq!(serde_json::from_str::<Vec<Refusal>>(&names).unwrap(), all);
    }
}
