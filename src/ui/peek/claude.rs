use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::{Lines, PeekLine, first_line};

/// <claude home>/projects/*/<id>.jsonl: a worker's session id is its flow id.
pub(super) fn transcript(claude_home: &Path, id: &str) -> Option<PathBuf> {
    let name = format!("{id}.jsonl");
    fs::read_dir(claude_home.join("projects"))
        .ok()?
        .flatten()
        .map(|entry| entry.path().join(&name))
        .find(|file| file.is_file())
}

/// The peek lines in a stretch of Claude Code transcript; `cut` when its first line is the end of a
/// longer one. A subagent's lines (isSidechain) are left out.
pub(super) fn parse(text: &str, cut: bool) -> Vec<PeekLine> {
    let mut lines = text.lines();
    if cut {
        lines.next();
    }
    let mut peek = Lines::default();
    for line in lines {
        let Ok(entry) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if entry["isSidechain"] == true {
            continue;
        }
        let Some(blocks) = entry["message"]["content"].as_array() else {
            continue;
        };
        for block in blocks {
            match (entry["type"].as_str(), block["type"].as_str()) {
                (Some("assistant"), Some("text")) => {
                    peek.text(block["text"].as_str().unwrap_or(""));
                }
                (Some("assistant"), Some("tool_use")) => {
                    let name = block["name"].as_str().unwrap_or("Tool").to_string();
                    peek.call(block["id"].as_str(), name, target(&block["input"]));
                }
                (Some("user"), Some("tool_result")) => {
                    if let Some(id) = block["tool_use_id"].as_str() {
                        peek.answer(id, first_line(&block["content"]));
                    }
                }
                _ => {}
            }
        }
    }
    peek.finish()
}

/// The argument that says what a tool works on: a path, a command, a pattern or a description.
fn target(input: &Value) -> String {
    [
        "file_path",
        "path",
        "command",
        "pattern",
        "description",
        "skill",
        "url",
        "query",
        "prompt",
    ]
    .iter()
    .find_map(|key| input[*key].as_str())
    .and_then(|value| value.lines().next())
    .unwrap_or("")
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_a_transcript_in_any_project_folder() {
        let dir = tempfile::TempDir::new().unwrap();
        let project = dir.path().join("projects/-tmp-repo");
        fs::create_dir_all(&project).unwrap();
        fs::write(project.join("7f3c.jsonl"), "").unwrap();

        assert_eq!(
            transcript(dir.path(), "7f3c"),
            Some(project.join("7f3c.jsonl"))
        );
        assert_eq!(transcript(dir.path(), "nope"), None);
    }
}
