use serde_json::Value;

/// The URL `gh pr create` or `glab mr create` printed: the first http(s) URL in its stdout.
pub fn created_merge_request(command: &str, stdout: &str) -> Option<String> {
    let words: Vec<&str> = command.split_whitespace().collect();
    let creates = words.windows(3).any(|window| {
        let tool = (window[0], window[1]);
        let opens = tool == ("gh", "pr") || tool == ("glab", "mr");
        opens && (window[2] == "create" || window[2] == "new")
    });
    if !creates {
        return None;
    }
    stdout
        .split_whitespace()
        .find(|word| word.starts_with("https://") || word.starts_with("http://"))
        .map(String::from)
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
        assert_eq!(
            created_merge_request("gh pr create --fill", "https://github.com/a/b/pull/12\n"),
            Some("https://github.com/a/b/pull/12".into())
        );
        assert_eq!(created_merge_request("git push", "https://x.y/z"), None);
        assert_eq!(
            created_merge_request("glab mr create", "error: no permission"),
            None
        );
    }
}
