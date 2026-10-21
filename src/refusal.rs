/// Work `saaio remove` refuses to lose unless told to; saaio ui asks the person instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// The worktree has uncommitted work (--force discards it).
    Uncommitted,
    /// The branch has commits on no other branch (--delete-unmerged deletes them).
    Unmerged,
}

impl Refusal {
    /// The refusal a failure's message says, for a program that reads what `saaio remove` printed.
    pub fn of(message: &str) -> Option<Refusal> {
        [Refusal::Uncommitted, Refusal::Unmerged]
            .into_iter()
            .find(|refusal| message.contains(refusal.phrase()))
    }

    /// The words `saaio remove`'s message carries for this refusal.
    pub fn phrase(self) -> &'static str {
        match self {
            Refusal::Uncommitted => "has uncommitted work",
            Refusal::Unmerged => "on no other branch",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_refusal_from_the_words_it_carries() {
        assert_eq!(
            Refusal::of("shop-230 has uncommitted work in /shop"),
            Some(Refusal::Uncommitted)
        );
        assert_eq!(
            Refusal::of("flow/shop-230 has 2 commits on no other branch"),
            Some(Refusal::Unmerged)
        );
        assert_eq!(Refusal::of("no flow named shop-230"), None);
    }
}
