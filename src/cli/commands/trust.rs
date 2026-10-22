//! `saaio trust`: lets saaio run a repository's .saaio scripts and Codex trust its worktrees

use std::path::{Path, PathBuf};

use crate::cli::Env;
use crate::cli::output::{Document, Failure, trust as output};
use crate::trust::Trust;
use crate::workspace::git;
use crate::workspace::hooks;

#[derive(Debug, clap::Args)]
pub struct Args {
    /// The repository or folder [default: the one you are in]
    #[arg(conflicts_with = "list")]
    folder: Option<PathBuf>,
    /// Take the trust back: saaio asks again before it runs the scripts
    #[arg(long, conflicts_with = "list")]
    revoke: bool,
    /// Every trusted repository
    #[arg(long)]
    list: bool,
}

/// Trusts the repository of `folder` (or the current one), only for a person at a terminal;
/// revokes it with --revoke, lists every trusted one with --list. Runs without a store.
pub fn run(args: &Args, env: &Env) -> Result<Document, Failure> {
    let Some(registry) = env.registry() else {
        return Err(Failure::error(
            "cannot find a state directory",
            vec!["Set HOME or XDG_STATE_HOME to a directory".to_string()],
        ));
    };
    let trust = Trust::at(&registry);
    if args.list {
        return Ok(output::list(&trust.repos()));
    }
    let cwd = std::env::current_dir()?;
    let root = root(&cwd.join(args.folder.as_deref().unwrap_or(Path::new("."))))?;
    if args.revoke {
        return Ok(output::revoked(&root, trust.revoke(&root)?));
    }
    if !env.is_person() {
        return Err(output::not_a_person(&root));
    }
    trust.grant(&root)?;
    Ok(output::trusted(&root, &hooks::present(&root)))
}

/// What trust is kept for: the repository `folder` is in, else the folder itself.
fn root(folder: &Path) -> Result<PathBuf, Failure> {
    let folder = folder.canonicalize().map_err(|error| {
        Failure::error(
            format!("cannot open {}: {error}", folder.display()),
            vec!["Pass an existing folder".to_string()],
        )
    })?;
    Ok(git::repo_root(&folder).unwrap_or(folder))
}
