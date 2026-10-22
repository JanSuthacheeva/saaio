//! What `saaio trust` prints, and the refusal of a repository not trusted yet

use std::path::{Path, PathBuf};

use serde::Serialize;

use super::{Document, Failure, toon};
use crate::refusal::Refusal;
use crate::workspace::hooks::Hook;

/// The refusal of `saaio start` in `repo` before the person trusts it: the hooks saaio would run,
/// and whether Codex would trust its worktrees.
pub(crate) fn untrusted(repo: &Path, hooks: &[Hook], codex: bool) -> Failure {
    let repo = repo.display();
    Failure::error(
        format!(
            "{repo} {}: {}",
            Refusal::Untrusted.phrase(),
            consequences(hooks, codex)
        ),
        vec![
            format!(
                "Ask the person to read its .saaio scripts, then run `saaio trust` in {repo} in their terminal"
            ),
            "Or start the flow in `saaio`, which asks them".to_string(),
        ],
    )
}

/// trusted, scripts[n] it runs from now on, help.
pub(crate) fn trusted(repo: &Path, hooks: &[Hook]) -> Document {
    #[derive(Serialize)]
    struct Trusted {
        trusted: String,
        scripts: Vec<String>,
        help: Vec<String>,
    }
    Document::Toon(toon(&Trusted {
        trusted: repo.display().to_string(),
        scripts: hooks
            .iter()
            .map(|hook| format!(".saaio/{}", hook.name()))
            .collect(),
        help: vec![format!(
            "Run `saaio trust --revoke {}` to take it back",
            repo.display()
        )],
    }))
}

/// revoked: the repo, or none with why.
pub(crate) fn revoked(repo: &Path, was: bool) -> Document {
    #[derive(Serialize)]
    struct Revoked {
        revoked: String,
        help: Vec<String>,
    }
    let (revoked, help) = if was {
        (
            repo.display().to_string(),
            "saaio asks again before it runs its .saaio scripts".to_string(),
        )
    } else {
        (
            "none".to_string(),
            format!("{} was not trusted", repo.display()),
        )
    };
    Document::Toon(toon(&Revoked {
        revoked,
        help: vec![help],
    }))
}

/// trusted[n], help.
pub(crate) fn list(repos: &[PathBuf]) -> Document {
    #[derive(Serialize)]
    struct Listed {
        trusted: Vec<String>,
        help: Vec<&'static str>,
    }
    Document::Toon(toon(&Listed {
        trusted: repos
            .iter()
            .map(|repo| repo.display().to_string())
            .collect(),
        help: vec!["Run `saaio trust --revoke <folder>` to take one back"],
    }))
}

/// The refusal of `saaio trust` from an agent's shell or without a terminal: only the person
/// trusts a repository.
pub(crate) fn not_a_person(repo: &Path) -> Failure {
    Failure::error(
        "only a person trusts a repository: run `saaio trust` in your own terminal",
        vec![format!(
            "Ask the person to read the .saaio scripts in {repo}, then run `saaio trust` there in their terminal",
            repo = repo.display()
        )],
    )
}

/// "saaio would run .saaio/setup and .saaio/teardown, and Codex would trust its worktrees".
fn consequences(hooks: &[Hook], codex: bool) -> String {
    let scripts: Vec<String> = hooks
        .iter()
        .map(|hook| format!(".saaio/{}", hook.name()))
        .collect();
    let run = match scripts.as_slice() {
        [] => None,
        [one] => Some(format!("saaio would run {one}")),
        [rest @ .., last] => Some(format!("saaio would run {} and {last}", rest.join(", "))),
    };
    match (run, codex) {
        (Some(run), true) => format!("{run}, and Codex would trust its worktrees"),
        (Some(run), false) => run,
        (None, _) => "Codex would trust its worktrees".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_what_would_run_in_an_untrusted_repo() {
        let failure = untrusted(Path::new("/shop"), &[Hook::Setup, Hook::Teardown], false);

        assert_eq!(
            failure.message,
            "/shop is not trusted: saaio would run .saaio/setup and .saaio/teardown"
        );
        assert_eq!(
            failure.help[0],
            "Ask the person to read its .saaio scripts, then run `saaio trust` in /shop in their terminal"
        );
        assert_eq!(Refusal::of(&failure.message), Some(Refusal::Untrusted));
        assert_eq!(
            untrusted(
                Path::new("/shop"),
                &[Hook::Setup, Hook::Pause, Hook::Resume],
                true
            )
            .message,
            "/shop is not trusted: saaio would run .saaio/setup, .saaio/pause and .saaio/resume, \
             and Codex would trust its worktrees"
        );
        assert_eq!(
            untrusted(Path::new("/shop"), &[], true).message,
            "/shop is not trusted: Codex would trust its worktrees"
        );
    }
}
