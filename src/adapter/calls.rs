use serde_json::Value;

/// The merge request a shell command created: when one of its simple commands runs `gh pr create`
/// or `glab mr create` (`mr new`), the last merge request URL in its output. Words that only appear
/// inside a quoted string or as another program's arguments create nothing.
pub fn created_merge_request(command: &str, output: &str) -> Option<String> {
    if !simple_commands(command).iter().any(|words| creates(words)) {
        return None;
    }
    output
        .split_whitespace()
        .filter(|word| word.starts_with("https://") || word.starts_with("http://"))
        .rfind(|url| url.contains("/pull/") || url.contains("/-/merge_requests/"))
        .map(String::from)
}

/// True when `url` is a merge request of the repo `remote` (a remote URL in any form git takes:
/// git@host:path.git, https://host/path, ssh://user@host:port/path) points to: same host, its path
/// followed by /pull/ or /-/merge_requests/. An SSH remote's host may be an alias from the ssh
/// config: when only the host differs, `real_host` names the host it stands for.
pub fn in_repo(url: &str, remote: &str, real_host: impl Fn(&str) -> Option<String>) -> bool {
    let (Some((host, path)), Some((repo_host, repo_path))) =
        (host_and_path(url), host_and_path(remote))
    else {
        return false;
    };
    let rest = path
        .strip_prefix(&repo_path)
        .and_then(|rest| rest.strip_prefix('/'));
    let same_repo = !repo_path.is_empty()
        && rest
            .is_some_and(|rest| rest.starts_with("pull/") || rest.starts_with("-/merge_requests/"));
    same_repo
        && (host == repo_host
            || is_ssh(remote) && web_host_of(remote, real_host).is_some_and(|real| real == host))
}

/// The web host a remote points to, lowercase: its host, or for an SSH remote the host
/// `real_host` resolves its alias to (ssh.github.com -> github.com); the host as written when
/// `real_host` names none. None for no host.
pub fn web_host_of(remote: &str, real_host: impl Fn(&str) -> Option<String>) -> Option<String> {
    let (host, _) = host_and_path(remote)?;
    if !is_ssh(remote) {
        return Some(host);
    }
    let real = real_host(&host).map_or(host, |real| real.to_lowercase());
    Some(web_host(&real).to_string())
}

/// The web host of a provider's SSH-over-443 host (ssh.github.com, altssh.gitlab.com); any other
/// host is its own.
fn web_host(ssh_host: &str) -> &str {
    match ssh_host {
        "ssh.github.com" => "github.com",
        "altssh.gitlab.com" => "gitlab.com",
        _ => ssh_host,
    }
}

/// git@host:path or ssh:// (git+ssh://, ssh+git://): a remote ssh connects to.
fn is_ssh(remote: &str) -> bool {
    match remote.trim().split_once("://") {
        Some((scheme, _)) => matches!(scheme, "ssh" | "git+ssh" | "ssh+git"),
        None => true,
    }
}

/// A URL's or remote's host (no user or port) and path (no .git, no slashes around), lowercase.
fn host_and_path(location: &str) -> Option<(String, String)> {
    let location = location.trim();
    let (authority, path) = match location.split_once("://") {
        Some((_, rest)) => rest.split_once('/').unwrap_or((rest, "")),
        None => location.split_once(':')?,
    };
    let host = authority.rsplit('@').next()?.split(':').next()?;
    let path = path.trim_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    (!host.is_empty()).then(|| (host.to_lowercase(), path.to_lowercase()))
}

/// `gh pr create ...` or `glab mr create|new ...`, after any VAR=value assignments.
fn creates(words: &[String]) -> bool {
    let mut words = words
        .iter()
        .map(String::as_str)
        .skip_while(|word| is_assignment(word));
    let program = words
        .next()
        .map(|program| program.rsplit('/').next().unwrap_or(program));
    let (noun, verb) = (words.next(), words.next());
    matches!(
        (program, noun, verb),
        (Some("gh"), Some("pr"), Some("create"))
            | (Some("glab"), Some("mr"), Some("create" | "new"))
    )
}

fn is_assignment(word: &str) -> bool {
    word.split_once('=').is_some_and(|(name, _)| {
        !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    })
}

/// The words of each simple command in a shell line, split at ; & | and newlines outside quotes,
/// with quotes and backslashes taken out as the shell would.
pub(super) fn simple_commands(line: &str) -> Vec<Vec<String>> {
    let mut commands = Vec::new();
    let mut words: Vec<String> = Vec::new();
    let mut word = String::new();
    let mut quote: Option<char> = None;
    let mut chars = line.chars();
    let mut started = false;
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some(open), c) if c == open => quote = None,
            (Some('"'), '\\') => word.extend(chars.next()),
            (Some(_), c) => word.push(c),
            (None, '\'' | '"') => {
                quote = Some(c);
                started = true;
            }
            (None, '\\') => {
                word.extend(chars.next());
                started = true;
            }
            (None, c) if c.is_whitespace() || matches!(c, ';' | '&' | '|') => {
                if started {
                    words.push(std::mem::take(&mut word));
                    started = false;
                }
                if (c == '\n' || matches!(c, ';' | '&' | '|')) && !words.is_empty() {
                    commands.push(std::mem::take(&mut words));
                }
            }
            (None, c) => {
                word.push(c);
                started = true;
            }
        }
        if quote.is_some() {
            started = true;
        }
    }
    if started {
        words.push(word);
    }
    if !words.is_empty() {
        commands.push(words);
    }
    commands
}

/// Name(target) for a permission question: Bash(command), Write(file_path), ...; a path inside the
/// session's folder is shown relative to it.
pub fn tool_call(name: &str, input: &Value, cwd: &str) -> String {
    let target = [
        "command",
        "file_path",
        "path",
        "url",
        "pattern",
        "description",
    ]
    .iter()
    .find_map(|key| input[*key].as_str())
    .and_then(|value| value.lines().next())
    .unwrap_or("");
    let inside = format!("{}/", cwd.trim_end_matches('/'));
    let target = match target.strip_prefix(&inside) {
        Some(relative) if !cwd.is_empty() => relative,
        _ => target,
    };
    if target.is_empty() {
        name.to_string()
    } else {
        format!("{name}({target})")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_merge_request_a_create_command_printed() {
        let created = |command| created_merge_request(command, "https://github.com/a/b/pull/12\n");

        assert_eq!(
            created("gh pr create --fill"),
            Some("https://github.com/a/b/pull/12".into())
        );
        assert_eq!(
            created("cd /repo && PATH=/opt/bin:$PATH gh pr create --title 'x; y'"),
            Some("https://github.com/a/b/pull/12".into()),
            "after other commands and variables"
        );
        assert_eq!(
            created_merge_request(
                "glab mr create --fill --yes",
                "Creating merge request\nhttps://gitlab.com/g/s/r/-/merge_requests/7\n"
            ),
            Some("https://gitlab.com/g/s/r/-/merge_requests/7".into())
        );
        assert_eq!(
            created_merge_request("git push", "https://github.com/a/b/pull/12"),
            None
        );
        assert_eq!(
            created_merge_request("glab mr create", "error: no permission"),
            None
        );
        assert_eq!(
            created_merge_request(
                "gh pr create",
                "see https://docs.github.com/en/pull-requests"
            ),
            None,
            "a link in the output that names no merge request"
        );
    }

    #[test]
    fn a_create_command_named_in_text_creates_nothing() {
        let probe =
            "Creating pull request for probe into main\nhttps://github.com/probe/repo/pull/7\n";

        assert_eq!(
            created_merge_request(
                "tmux -L flowrec set-buffer -- 'Run exactly: PATH=/tmp/bin:$PATH gh pr create --fill'",
                probe
            ),
            None,
            "today's case: the words were inside a quoted string"
        );
        assert_eq!(created_merge_request(r#"echo "gh pr create""#, probe), None);
        assert_eq!(created_merge_request("echo gh pr create", probe), None);
        assert_eq!(
            created_merge_request("grep -r 'glab mr create' docs", probe),
            None
        );
    }

    fn no_alias(_: &str) -> Option<String> {
        None
    }

    fn never_asked(host: &str) -> Option<String> {
        panic!("asked ssh for {host}")
    }

    #[test]
    fn a_merge_request_belongs_to_the_repo_its_origin_names() {
        let pull = "https://github.com/shop/app/pull/12";

        for remote in [
            "git@github.com:shop/app.git",
            "https://github.com/shop/app",
            "https://jan@github.com/Shop/App.git",
            "ssh://git@github.com:22/shop/app.git",
        ] {
            assert!(in_repo(pull, remote, no_alias), "{remote}");
        }
        assert!(
            !in_repo(pull, "git@github.com:shop/app-admin.git", no_alias),
            "another repo"
        );
        assert!(
            !in_repo(pull, "git@gitlab.com:shop/app.git", no_alias),
            "another host"
        );
        assert!(!in_repo(
            "https://github.com/probe/repo/pull/7",
            "git@github.com:shop/app.git",
            no_alias
        ));
        assert!(!in_repo(
            "https://github.com/shop/app/issues/3",
            "git@github.com:shop/app.git",
            no_alias
        ));
        assert!(in_repo(
            "https://gitlab.com/group/sub/app/-/merge_requests/7",
            "git@gitlab.com:group/sub/app.git",
            no_alias
        ));
        assert!(!in_repo(pull, "", no_alias), "no origin");
    }

    #[test]
    fn an_ssh_remote_through_a_host_alias_names_the_real_host() {
        let pull = "https://github.com/shop/app/pull/12";
        let github = |alias: &str| (alias == "github-work").then(|| "GitHub.com".to_string());

        assert!(
            in_repo(pull, "git@github-work:shop/app.git", github),
            "an alias"
        );
        assert!(
            in_repo(pull, "ssh://git@github-work/shop/app.git", github),
            "an alias in ssh:// form"
        );
        for remote in [
            "git+ssh://git@github-work/shop/app.git",
            "ssh+git://git@github-work/shop/app.git",
        ] {
            assert!(in_repo(pull, remote, github), "{remote}");
        }
        assert!(
            !in_repo(pull, "file://github-work/shop/app.git", never_asked),
            "a file remote is never an ssh alias"
        );
        assert!(
            in_repo(pull, "git@github.com:shop/app.git", never_asked),
            "a plain host matches without ssh"
        );
        assert!(
            !in_repo(pull, "git@gitlab-work:shop/app.git", |alias: &str| Some(
                alias.to_string()
            )),
            "an unknown alias is only itself"
        );
        assert!(
            !in_repo(pull, "https://github-work/shop/app.git", never_asked),
            "an https remote is never an ssh alias"
        );
        assert!(
            !in_repo(pull, "git@github-work:shop/admin.git", never_asked),
            "another repo needs no ssh"
        );
    }

    #[test]
    fn names_the_web_host_a_remote_points_to() {
        let github = |alias: &str| (alias == "github-work").then(|| "GitHub.com".to_string());
        let host = |remote| web_host_of(remote, github);

        assert_eq!(
            host("https://GitHub.com/shop/app").as_deref(),
            Some("github.com")
        );
        assert_eq!(
            host("git@github-work:shop/app.git").as_deref(),
            Some("github.com")
        );
        assert_eq!(
            host("ssh://git@github-work/shop/app.git").as_deref(),
            Some("github.com")
        );
        assert_eq!(
            host("git@gitlab.com:shop/app.git").as_deref(),
            Some("gitlab.com"),
            "no alias"
        );
        assert_eq!(
            web_host_of("git@github-443:shop/app.git", |_| Some(
                "ssh.github.com".into()
            ))
            .as_deref(),
            Some("github.com")
        );
        assert_eq!(
            web_host_of("https://github-work/shop/app", never_asked).as_deref(),
            Some("github-work"),
            "an https remote is never an ssh alias"
        );
        assert_eq!(host(""), None);
    }

    #[test]
    fn an_alias_to_a_providers_ssh_over_443_host_is_its_web_host() {
        let to = |real: &'static str| move |_: &str| Some(real.to_string());

        assert!(in_repo(
            "https://github.com/shop/app/pull/12",
            "git@github-work:shop/app.git",
            to("ssh.github.com")
        ));
        assert!(in_repo(
            "https://gitlab.com/shop/app/-/merge_requests/7",
            "git@gitlab-work:shop/app.git",
            to("altssh.gitlab.com")
        ));
        assert!(!in_repo(
            "https://github.com/shop/app/pull/12",
            "git@github-work:shop/app.git",
            to("ssh.example.com")
        ));
    }
}
