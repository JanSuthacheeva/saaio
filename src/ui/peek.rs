mod claude;
mod codex;

use std::borrow::Cow;
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde_json::Value;

use crate::view::FlowView;

/// How much of the end of a transcript the peek reads first; it doubles until it has enough lines,
/// since Claude Code writes attachments of 100 KiB and more between a tool call and the answer.
const TAIL_BYTES: u64 = 64 * 1024;
/// The most it reads.
const MAX_TAIL_BYTES: u64 = 4 * 1024 * 1024;
/// How many transcript lines the peek keeps.
pub const PEEK_LINES: usize = 12;

/// One line of what a worker did lately, read from its harness's transcript, never from its screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PeekLine {
    /// ⏺ Name(target)
    Tool { name: String, target: String },
    /// ⎿ the first line of what the tool answered
    Result(String),
    /// ⏺ what the agent said
    Text(String),
    /// A tool still running.
    Now(String),
    /// Where an ended flow's code is: its branch and worktree.
    Code {
        branch: Option<String>,
        worktree: String,
        exists: bool,
    },
}

/// Whose transcript a file is: Claude Code's session JSONL or Codex's rollout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Claude,
    Codex,
}

/// The flow's transcript: the one its harness reported (Codex's rollout), else Claude Code's,
/// found by the flow id it runs as.
pub fn transcript_of(flow: &FlowView, claude_home: &Path) -> Option<(PathBuf, Format)> {
    let reported = flow
        .session
        .as_ref()
        .and_then(|session| session.transcript.as_ref());
    match reported {
        Some(path) => Some((PathBuf::from(path), Format::Codex)),
        None => claude::transcript(claude_home, &flow.id).map(|file| (file, Format::Claude)),
    }
}

/// The last `max` lines worth showing, read from the end of the file: 64 KiB, more when that holds too few.
pub fn tail(file: &Path, max: usize, format: Format) -> Result<Vec<PeekLine>> {
    let mut handle =
        File::open(file).with_context(|| format!("could not open {}", file.display()))?;
    let len = handle.metadata()?.len();
    let mut window = TAIL_BYTES;
    loop {
        let start = len.saturating_sub(window);
        handle.seek(SeekFrom::Start(start))?;
        let mut bytes = Vec::new();
        handle.read_to_end(&mut bytes)?;
        let text = String::from_utf8_lossy(&bytes);
        let mut peek = match format {
            Format::Claude => claude::parse(&text, start > 0),
            Format::Codex => codex::parse(&text, start > 0),
        };
        if peek.len() >= max || start == 0 || window >= MAX_TAIL_BYTES {
            let skip = peek.len().saturating_sub(max);
            return Ok(peek.split_off(skip));
        }
        window *= 2;
    }
}

/// Peek lines in the order they happened: a tool's result goes right under its call, however
/// many calls ran in between; a call still unanswered at the end is shown running.
#[derive(Debug, Default)]
struct Lines {
    peek: Vec<PeekLine>,
    running: HashMap<String, usize>,
}

impl Lines {
    fn text(&mut self, text: &str) {
        let text = text.trim();
        if !text.is_empty() {
            self.peek.push(PeekLine::Text(text.to_string()));
        }
    }

    fn call(&mut self, id: Option<&str>, name: String, target: String) {
        if let Some(id) = id {
            self.running.insert(id.to_string(), self.peek.len());
        }
        self.peek.push(PeekLine::Tool { name, target });
    }

    /// The result of call `id`, by its first line; nothing for a call it never saw.
    fn answer(&mut self, id: &str, first: Option<String>) {
        let Some(at) = self.running.remove(id) else {
            return;
        };
        if let Some(first) = first {
            self.peek.insert(at + 1, PeekLine::Result(first));
            for index in self.running.values_mut() {
                if *index > at {
                    *index += 1;
                }
            }
        }
    }

    fn finish(mut self) -> Vec<PeekLine> {
        let last_running = self.running.values().max().copied();
        if let Some(at) = last_running
            && at + 1 == self.peek.len()
            && let PeekLine::Tool { name, target } = &self.peek[at]
        {
            self.peek[at] = PeekLine::Now(call(name, target));
        }
        self.peek
    }
}

/// Name(target) as Claude Code shows a tool call.
pub fn call(name: &str, target: &str) -> String {
    if target.is_empty() {
        name.to_string()
    } else {
        format!("{name}({target})")
    }
}

/// The first non-empty line of a tool result, given as text or as text blocks.
fn first_line(content: &Value) -> Option<String> {
    let text: Cow<str> = match content {
        Value::String(text) => Cow::Borrowed(text),
        Value::Array(blocks) => Cow::Owned(
            blocks
                .iter()
                .filter_map(|block| block["text"].as_str())
                .collect::<Vec<_>>()
                .join("\n"),
        ),
        _ => return None,
    };
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(String::from)
}

/// The peek of the flow under the sidebar cursor; reads its transcript again only when it grew.
#[derive(Debug)]
pub struct PeekCache {
    claude_home: PathBuf,
    flow: Option<String>,
    file: Option<(PathBuf, Format)>,
    /// The transcript's length when last read; None to read it again.
    len: Option<u64>,
    lines: Vec<PeekLine>,
}

impl PeekCache {
    pub fn new(claude_home: PathBuf) -> PeekCache {
        PeekCache {
            claude_home,
            flow: None,
            file: None,
            len: None,
            lines: Vec::new(),
        }
    }

    /// Re-reads only when the flow changed or its file grew; true when the lines changed.
    pub fn refresh(&mut self, flow: Option<&FlowView>) -> bool {
        let Some(flow) = flow else {
            self.flow = None;
            let had = !self.lines.is_empty();
            self.lines.clear();
            return had;
        };
        if self.flow.as_deref() != Some(flow.id.as_str()) {
            self.flow = Some(flow.id.clone());
            self.file = None;
            self.len = None;
        }
        if self.file.is_none() {
            self.file = transcript_of(flow, &self.claude_home);
        }
        let len = self
            .file
            .as_ref()
            .and_then(|(file, _)| fs::metadata(file).ok())
            .map_or(0, |metadata| metadata.len());
        let code = code_of(flow);
        if self.len == Some(len) && self.lines.first() == code.as_ref() {
            return false;
        }
        self.len = Some(len);
        self.lines = code.into_iter().collect();
        if let Some((file, format)) = &self.file {
            self.lines
                .extend(tail(file, PEEK_LINES, *format).unwrap_or_default());
        }
        true
    }

    pub fn lines(&self) -> &[PeekLine] {
        &self.lines
    }
}

/// Where a finished flow's code is, for the peek.
fn code_of(flow: &FlowView) -> Option<PeekLine> {
    let space = flow.workspace.as_ref().filter(|_| flow.is_finished())?;
    Some(PeekLine::Code {
        branch: space.branch.clone(),
        worktree: space.path.clone(),
        exists: Path::new(&space.path).exists(),
    })
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use tempfile::TempDir;

    use super::*;

    fn line(value: &Value) -> String {
        value.to_string()
    }

    fn assistant(blocks: &Value) -> String {
        line(
            &serde_json::json!({"type": "assistant", "isSidechain": false, "message": {"content": blocks}}),
        )
    }

    fn result(id: &str, content: &Value) -> String {
        line(
            &serde_json::json!({"type": "user", "isSidechain": false, "message": {"content": [
                {"type": "tool_result", "tool_use_id": id, "content": content}
            ]}}),
        )
    }

    fn write(dir: &Path, lines: &[String]) -> PathBuf {
        let file = dir.join("7f3c.jsonl");
        let mut handle = File::create(&file).unwrap();
        for line in lines {
            writeln!(handle, "{line}").unwrap();
        }
        file
    }

    #[test]
    fn reads_tools_their_results_and_text_in_order() {
        let dir = TempDir::new().unwrap();
        let file = write(
            dir.path(),
            &[
                line(
                    &serde_json::json!({"type": "user", "message": {"content": "add the limiter"}}),
                ),
                assistant(&serde_json::json!([
                    {"type": "thinking", "thinking": "hm"},
                    {"type": "tool_use", "id": "a", "name": "Edit", "input": {"file_path": "app/Throttle.php"}},
                ])),
                result("a", &serde_json::json!("Added 22 lines\nmore")),
                assistant(&serde_json::json!([
                    {"type": "tool_use", "id": "b", "name": "Bash", "input": {"command": "php artisan test\n--filter x"}},
                ])),
                result(
                    "b",
                    &serde_json::json!([{"type": "text", "text": "\n  Tests: 3 passed"}]),
                ),
                line(
                    &serde_json::json!({"type": "assistant", "isSidechain": true, "message": {"content": [
                        {"type": "text", "text": "a subagent talking"}
                    ]}}),
                ),
                assistant(
                    &serde_json::json!([{"type": "text", "text": "The limiter keys on the user id."}]),
                ),
                assistant(&serde_json::json!([
                    {"type": "tool_use", "id": "c", "name": "Write", "input": {"file_path": "ThrottleTest.php"}},
                ])),
            ],
        );

        assert_eq!(
            tail(&file, 10, Format::Claude).unwrap(),
            vec![
                PeekLine::Tool {
                    name: "Edit".into(),
                    target: "app/Throttle.php".into()
                },
                PeekLine::Result("Added 22 lines".into()),
                PeekLine::Tool {
                    name: "Bash".into(),
                    target: "php artisan test".into()
                },
                PeekLine::Result("Tests: 3 passed".into()),
                PeekLine::Text("The limiter keys on the user id.".into()),
                PeekLine::Now("Write(ThrottleTest.php)".into()),
            ]
        );
        assert_eq!(
            tail(&file, 2, Format::Claude).unwrap().len(),
            2,
            "keeps the last lines"
        );
    }

    #[test]
    fn skips_a_truncated_last_line_and_a_cut_first_one() {
        let dir = TempDir::new().unwrap();
        let filler = assistant(&serde_json::json!([{"type": "text", "text": "x".repeat(70_000)}]));
        let mut lines = vec![
            filler,
            assistant(&serde_json::json!([{"type": "text", "text": "kept"}])),
        ];
        lines.push("{\"type\": \"assistant\", \"message\": {\"content\": [".to_string());
        let file = write(dir.path(), &lines);

        assert_eq!(
            tail(&file, 1, Format::Claude).unwrap(),
            vec![PeekLine::Text("kept".into())]
        );
        assert_eq!(
            tail(&file, 10, Format::Claude).unwrap().len(),
            2,
            "reads on past the cut line"
        );
    }

    #[test]
    fn reads_further_back_past_a_large_attachment() {
        let dir = TempDir::new().unwrap();
        let attachment = line(&serde_json::json!({
            "type": "attachment",
            "attachment": {"type": "prompt_snapshot", "text": "x".repeat(200_000)}
        }));
        let file = write(
            dir.path(),
            &[
                assistant(&serde_json::json!([
                    {"type": "tool_use", "id": "a", "name": "Bash", "input": {"command": "ls"}},
                ])),
                result("a", &serde_json::json!(".git")),
                attachment,
                assistant(&serde_json::json!([{"type": "text", "text": "Done."}])),
            ],
        );

        assert_eq!(
            tail(&file, 10, Format::Claude).unwrap(),
            vec![
                PeekLine::Tool {
                    name: "Bash".into(),
                    target: "ls".into()
                },
                PeekLine::Result(".git".into()),
                PeekLine::Text("Done.".into()),
            ]
        );
    }

    #[test]
    fn reads_a_codex_worker_from_the_rollout_its_hooks_named() {
        let dir = TempDir::new().unwrap();
        let events = [
            crate::event::Event::new(
                0,
                crate::event::EventKind::Started {
                    name: "csv".into(),
                    request: "add csv".into(),
                    mode: crate::event::Mode::Guided,
                },
            ),
            crate::event::Event::new(
                1,
                crate::event::EventKind::Session {
                    id: "01a1".into(),
                    transcript: Some("/home/dev/.codex/sessions/rollout-01a1.jsonl".into()),
                },
            ),
        ];
        let codex = crate::fold::fold("7f3c", &events, None, 2).unwrap();
        let claude = crate::fold::fold("7f3c", &events[..1], None, 2).unwrap();

        assert_eq!(
            transcript_of(&codex, dir.path()),
            Some((
                PathBuf::from("/home/dev/.codex/sessions/rollout-01a1.jsonl"),
                Format::Codex
            ))
        );
        assert_eq!(
            transcript_of(&claude, dir.path()),
            None,
            "no Claude transcript yet"
        );
    }
}
