//! The text of the file `saaio bug-report` writes, and its name (pure)

use std::fmt::Write;
use std::path::PathBuf;

use crate::proto::{BUILD, PROTOCOL_VERSION};
use crate::system::System;
use crate::text;

/// Lines of each log the report keeps, from its end.
pub const TAIL_LINES: usize = 300;

/// What a bug report says, gathered by `saaio bug-report`.
#[derive(Debug)]
pub struct Facts {
    pub at: u64,
    pub offset_secs: i64,
    pub system: System,
    /// The store's root, or why it could not be opened.
    pub store: Result<PathBuf, String>,
    pub config: FileCheck,
    pub theme: FileCheck,
    /// server.log.1 then server.log, oldest first; empty without a store.
    pub logs: Vec<LogTail>,
}

/// One settings file: where it is and whether it loads.
#[derive(Debug)]
pub struct FileCheck {
    pub path: Option<PathBuf>,
    pub loaded: Loaded,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Loaded {
    Loads,
    Missing,
    Broken(String),
}

/// The end of one log, or why it could not be read (None: it does not exist).
#[derive(Debug)]
pub struct LogTail {
    pub path: PathBuf,
    pub text: Option<Result<String, String>>,
}

/// The n-th report of a day: 1 is `saaio-report-2026-10-18.txt`, 2 is
/// `saaio-report-2026-10-18-2.txt`.
pub fn file_name(at: u64, offset_secs: i64, n: u32) -> String {
    let date = text::date(at, offset_secs);
    match n {
        1 => format!("saaio-report-{date}.txt"),
        n => format!("saaio-report-{date}-{n}.txt"),
    }
}

/// The whole report, each of `homes` replaced by `~` everywhere in it.
pub fn text(facts: &Facts, homes: &[PathBuf]) -> String {
    let mut out = format!(
        "saaio bug report, {}\n\n",
        text::stamp(facts.at, facts.offset_secs)
    );
    let os = facts.system.os.as_deref().unwrap_or("unknown");
    let mut rows = vec![
        (
            "saaio".to_string(),
            format!("{BUILD}, protocol {PROTOCOL_VERSION}"),
        ),
        ("os".to_string(), os.to_string()),
    ];
    rows.extend(
        facts
            .system
            .tools
            .iter()
            .map(|(tool, probe)| ((*tool).to_string(), probe.text())),
    );
    table(&mut out, &rows);
    out.push('\n');
    let store = match &facts.store {
        Ok(root) => root.display().to_string(),
        Err(error) => format!("cannot be opened: {error}"),
    };
    table(
        &mut out,
        &[
            ("store".to_string(), store),
            ("config.toml".to_string(), check(&facts.config)),
            ("theme.toml".to_string(), check(&facts.theme)),
        ],
    );
    for log in &facts.logs {
        let path = log.path.display();
        let _ = match &log.text {
            None => write!(out, "\n== {path} ==\nmissing\n"),
            Some(Err(error)) => write!(out, "\n== {path} ==\ncould not be read: {error}\n"),
            Some(Ok(text)) if text.is_empty() => write!(out, "\n== {path} ==\nempty\n"),
            Some(Ok(text)) => {
                let text = text.strip_suffix('\n').unwrap_or(text);
                write!(out, "\n== {path} (last {TAIL_LINES} lines) ==\n{text}\n")
            }
        };
    }
    tilde(&out, homes)
}

/// The last `lines` lines of `text`.
pub fn tail(text: &str, lines: usize) -> &str {
    if lines == 0 {
        return "";
    }
    let body = text.strip_suffix('\n').unwrap_or(text);
    match body.rmatch_indices('\n').nth(lines - 1) {
        Some((at, _)) => &text[at + 1..],
        None => text,
    }
}

/// Rows of a label and a value, the values in one column.
fn table(out: &mut String, rows: &[(String, String)]) {
    let width = rows.iter().map(|(label, _)| label.len()).max().unwrap_or(0);
    for (label, value) in rows {
        let _ = writeln!(out, "{label:width$}  {value}");
    }
}

/// `~/.config/saaio/config.toml: loads`.
fn check(file: &FileCheck) -> String {
    let Some(path) = &file.path else {
        return "no config folder (HOME and XDG_CONFIG_HOME are unset)".to_string();
    };
    let state = match &file.loaded {
        Loaded::Loads => "loads".to_string(),
        Loaded::Missing => "missing, the built-in defaults apply".to_string(),
        Loaded::Broken(error) => format!("does not load: {error}"),
    };
    format!("{}: {state}", path.display())
}

/// `text` with each of `homes`, and the name Claude Code gives its projects folder
/// (`-Users-jan`), written `~`, the longest first so a real path is not cut by the link to it. A
/// match counts only as a whole name: not a longer one it begins (`/Users/janet` for `/Users/jan`)
/// nor one it ends (`x/Users/jan`); a `/` or `:` before it is no part of a name (`file://`,
/// `PATH`).
fn tilde(text: &str, homes: &[PathBuf]) -> String {
    let mut forms: Vec<(String, char)> = homes
        .iter()
        .map(|home| home.to_string_lossy().trim_end_matches('/').to_string())
        .filter(|home| !home.is_empty())
        .flat_map(|home| [(dashed(&home), '-'), (home, '/')])
        .collect();
    forms.sort_by_key(|(form, _)| std::cmp::Reverse(form.len()));
    forms
        .iter()
        .fold(text.to_string(), |text, (form, separator)| {
            tilde_form(&text, form, *separator)
        })
}

/// How Claude Code names a folder's project: every character but ASCII letters and digits a `-`.
fn dashed(path: &str) -> String {
    path.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

/// `text` with every whole `form` written `~`; a name may go on after it past `separator` or `/`.
fn tilde_form(text: &str, form: &str, separator: char) -> String {
    let in_name = |c: char| c.is_alphanumeric() || matches!(c, '-' | '_' | '.');
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find(form) {
        let after = &rest[at + form.len()..];
        let starts = !rest[..at].chars().next_back().is_some_and(in_name);
        let ends = !after
            .chars()
            .next()
            .is_some_and(|c| c != separator && in_name(c));
        out.push_str(&rest[..at]);
        out.push_str(if starts && ends { "~" } else { form });
        rest = after;
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::system::Probe;

    /// 2026-10-18 14:02:11 +07:00.
    const AT: u64 = 1_792_306_931_000;
    const OFFSET: i64 = 7 * 3_600;

    fn facts() -> Facts {
        let store = PathBuf::from("/Users/jan/.local/state/saaio/projects/shop");
        Facts {
            at: AT,
            offset_secs: OFFSET,
            system: System {
                os: Some("Darwin 25.6.0 arm64".to_string()),
                tools: vec![
                    (
                        "claude",
                        Probe::Version("2.1.302 (Claude Code)".to_string()),
                    ),
                    ("glab", Probe::Missing),
                ],
            },
            store: Ok(store.clone()),
            config: FileCheck {
                path: Some("/Users/jan/.config/saaio/config.toml".into()),
                loaded: Loaded::Loads,
            },
            theme: FileCheck {
                path: Some("/Users/jan/.config/saaio/theme.toml".into()),
                loaded: Loaded::Broken(
                    "/Users/jan/.config/saaio/theme.toml:3: unknown color `bg`".to_string(),
                ),
            },
            logs: vec![
                LogTail {
                    path: store.join("server.log.1"),
                    text: None,
                },
                LogTail {
                    path: store.join("server.log"),
                    text: Some(Ok("one\ntwo\n".to_string())),
                },
            ],
        }
    }

    #[test]
    fn writes_versions_settings_and_the_end_of_each_log() {
        let report = text(&facts(), &[PathBuf::from("/Users/jan")]);

        assert_eq!(
            report,
            format!(
                "saaio bug report, 2026-10-18 14:02:11 +07:00

saaio   {BUILD}, protocol {PROTOCOL_VERSION}
os      Darwin 25.6.0 arm64
claude  2.1.302 (Claude Code)
glab    missing

store        ~/.local/state/saaio/projects/shop
config.toml  ~/.config/saaio/config.toml: loads
theme.toml   ~/.config/saaio/theme.toml: does not load: ~/.config/saaio/theme.toml:3: unknown color `bg`

== ~/.local/state/saaio/projects/shop/server.log.1 ==
missing

== ~/.local/state/saaio/projects/shop/server.log (last {TAIL_LINES} lines) ==
one
two
"
            )
        );
    }

    #[test]
    fn says_why_the_store_or_a_log_is_missing() {
        let mut facts = facts();
        facts.store = Err("cannot find a state directory".to_string());
        facts.config = FileCheck {
            path: None,
            loaded: Loaded::Missing,
        };
        facts.theme.loaded = Loaded::Missing;
        facts.logs = vec![LogTail {
            path: "/state/server.log".into(),
            text: Some(Err("permission denied".to_string())),
        }];

        let report = text(&facts, &[]);

        assert!(
            report.contains("store        cannot be opened: cannot find a state directory\n"),
            "{report}"
        );
        assert!(
            report.contains("config.toml  no config folder (HOME and XDG_CONFIG_HOME are unset)\n"),
            "{report}"
        );
        assert!(
            report.contains(
                "theme.toml   /Users/jan/.config/saaio/theme.toml: missing, the built-in defaults apply\n"
            ),
            "{report}"
        );
        assert!(
            report.ends_with("== /state/server.log ==\ncould not be read: permission denied\n"),
            "{report}"
        );
    }

    #[test]
    fn writes_home_as_a_tilde_only_where_it_is_the_home_folder() {
        let homes = [PathBuf::from("/Users/jan/")];
        assert_eq!(
            tilde("/Users/jan/x and /Users/jan, at /Users/jan", &homes),
            "~/x and ~, at ~"
        );
        assert_eq!(
            tilde("/Users/janet /Users/jan-old x/Users/jan", &homes),
            "/Users/janet /Users/jan-old x/Users/jan"
        );
        assert_eq!(tilde("cwd: \"/Users/jan\"", &homes), "cwd: \"~\"");
    }

    #[test]
    fn writes_home_as_a_tilde_in_urls_lists_and_claude_code_project_names() {
        let homes = [PathBuf::from("/Users/jan")];
        assert_eq!(
            tilde("file:///Users/jan/x.md PATH=/bin:/Users/jan/bin", &homes),
            "file://~/x.md PATH=/bin:~/bin"
        );
        assert_eq!(
            tilde(
                "~/.claude/projects/-Users-jan-projects-shop/a.jsonl -Users-janet",
                &homes
            ),
            "~/.claude/projects/~-projects-shop/a.jsonl -Users-janet"
        );
        assert_eq!(
            tilde("~/.claude/projects/-Users-jan/a.jsonl", &homes),
            "~/.claude/projects/~/a.jsonl"
        );
    }

    #[test]
    fn writes_the_real_path_of_a_linked_home_as_a_tilde() {
        let homes = [PathBuf::from("/home/jan"), PathBuf::from("/data/home/jan")];
        assert_eq!(
            tilde(
                "/data/home/jan/x /home/jan/y -data-home-jan-shop -home-jan-shop",
                &homes
            ),
            "~/x ~/y ~-shop ~-shop"
        );
    }

    #[test]
    fn keeps_the_last_lines() {
        assert_eq!(tail("a\nb\nc\n", 2), "b\nc\n");
        assert_eq!(tail("a\nb\nc", 2), "b\nc");
        assert_eq!(tail("a\nb\n", 5), "a\nb\n");
        assert_eq!(tail("a\n", 0), "");
        assert_eq!(tail("", 3), "");
    }

    #[test]
    fn names_the_file_by_the_local_date() {
        assert_eq!(file_name(AT, OFFSET, 1), "saaio-report-2026-10-18.txt");
        assert_eq!(file_name(AT, OFFSET, 2), "saaio-report-2026-10-18-2.txt");
    }
}
