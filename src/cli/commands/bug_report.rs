//! `saaio bug-report`: gathers the `Facts` and writes the report file in the current folder

use std::fs;
use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};

use crate::bug_report::{self, Facts, FileCheck, Loaded, LogTail, TAIL_LINES};
use crate::cli::Env;
use crate::cli::output::{self, Document, Failure};
use crate::config::Config;
use crate::private;
use crate::system::System;
use crate::text;
use crate::ui::theme_file;

/// `saaio bug-report`: gathers the Facts, writes bug_report::text to file_name in the current
/// folder; its path and the check before uploading. Runs without a store.
pub fn run(env: &Env, clock: fn() -> u64) -> Result<Document, Failure> {
    let cwd = std::env::current_dir()?;
    let store = match env.store(Some(&cwd)) {
        Ok(Some(store)) => Ok(store),
        Ok(None) => Err("cannot find a state directory".to_string()),
        Err(error) => Err(format!("{error:#}")),
    };
    let logs = match &store {
        Ok(store) => {
            let log = env.server_log(store);
            let mut older = log.clone().into_os_string();
            older.push(".1");
            vec![tail(PathBuf::from(older)), tail(log)]
        }
        Err(_) => Vec::new(),
    };
    let config_file = env.config_file();
    let config = FileCheck {
        loaded: loaded(config_file.as_deref(), |file| {
            Config::read(Some(file)).map(|_| ())
        }),
        path: config_file,
    };
    let theme_file = env.config_dir().map(|dir| dir.join(theme_file::THEME_FILE));
    let theme = FileCheck {
        loaded: loaded(theme_file.as_deref(), |file| {
            let dir = file.parent().unwrap_or(Path::new("."));
            theme_file::load(dir).0.map(|_| ())
        }),
        path: theme_file,
    };
    let now = clock();
    let offset_secs = text::local_offset_secs();
    let facts = Facts {
        at: now,
        offset_secs,
        system: System::probe(),
        store: store.map(|store| store.root().to_path_buf()),
        config,
        theme,
        logs,
    };
    let text = bug_report::text(&facts, &homes(env.user_home()));
    let path = write(&cwd, now, offset_secs, text.as_bytes()).map_err(|(path, error)| {
        Failure::error(
            format!("could not write {}: {error}", path.display()),
            vec!["Run `saaio bug-report` in a folder you can write to".to_string()],
        )
    })?;
    Ok(output::bug_report::written(&path))
}

/// The home folder as given and, when it is a link, the real path it leads to.
fn homes(home: Option<&Path>) -> Vec<PathBuf> {
    let Some(home) = home else {
        return Vec::new();
    };
    let mut homes = vec![home.to_path_buf()];
    homes.extend(fs::canonicalize(home).ok().filter(|real| real != home));
    homes
}

/// The most reports a day gets before saaio stops looking for a free name.
const REPORTS_A_DAY: u32 = 100;

/// Writes the report under the day's first name nothing is at yet, never through a link; the
/// last path tried and why when none works.
fn write(
    dir: &Path,
    at: u64,
    offset_secs: i64,
    text: &[u8],
) -> Result<PathBuf, (PathBuf, io::Error)> {
    let mut n = 1;
    loop {
        let path = dir.join(bug_report::file_name(at, offset_secs, n));
        match private::write_new(&path, text) {
            Ok(()) => return Ok(path),
            Err(error) if error.kind() == ErrorKind::AlreadyExists && n < REPORTS_A_DAY => n += 1,
            Err(error) => return Err((path, error)),
        }
    }
}

/// Whether `file` loads by `load`: Missing when there is none.
fn loaded(file: Option<&Path>, load: impl Fn(&Path) -> anyhow::Result<()>) -> Loaded {
    match file {
        Some(file) if file.exists() => match load(file) {
            Ok(()) => Loaded::Loads,
            Err(error) => Loaded::Broken(format!("{error:#}")),
        },
        _ => Loaded::Missing,
    }
}

/// The end of the log at `path`.
fn tail(path: PathBuf) -> LogTail {
    let text = match fs::read(&path) {
        Ok(bytes) => {
            let text = String::from_utf8_lossy(&bytes);
            Some(Ok(bug_report::tail(&text, TAIL_LINES).to_string()))
        }
        Err(error) if error.kind() == ErrorKind::NotFound => None,
        Err(error) => Some(Err(error.to_string())),
    };
    LogTail { path, text }
}
