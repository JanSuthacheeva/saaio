//! Codex's rollout: the peek lines in it

use serde_json::Value;

use super::{Lines, PeekLine};

/// The peek lines in a stretch of Codex rollout; `cut` when its first line is the end of a longer
/// one.
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
        if entry["type"] != "response_item" {
            continue;
        }
        let item = &entry["payload"];
        let id = item["call_id"].as_str();
        match item["type"].as_str().unwrap_or("") {
            "message" if item["role"] == "assistant" => {
                let parts = item["content"].as_array().into_iter().flatten();
                let text: Vec<&str> = parts.filter_map(|part| part["text"].as_str()).collect();
                peek.text(&text.join("\n"));
            }
            "custom_tool_call" => {
                let name = item["name"].as_str().unwrap_or("tool");
                let input = item["input"].as_str().unwrap_or("");
                match (exec_command(input), patched(input)) {
                    (Some(command), _) if name == "exec" => {
                        peek.call(id, "Bash".to_string(), first(&command));
                    }
                    (None, Some(file)) if name == "exec" => {
                        peek.call(id, "Edit".to_string(), file);
                    }
                    _ => peek.call(id, name.to_string(), String::new()),
                }
            }
            "function_call" => {
                let name = item["name"].as_str().unwrap_or("tool").to_string();
                let arguments = item["arguments"].as_str().unwrap_or("{}");
                let arguments: Value = serde_json::from_str(arguments).unwrap_or(Value::Null);
                peek.call(id, name, target(&arguments));
            }
            "local_shell_call" => {
                let argv = item["action"]["command"].as_array().into_iter().flatten();
                let command: Vec<&str> = argv.filter_map(Value::as_str).collect();
                peek.call(id, "Bash".to_string(), first(&command.join(" ")));
            }
            "custom_tool_call_output" | "function_call_output" => {
                if let Some(id) = id {
                    peek.answer(id, output(&item["output"]));
                }
            }
            _ => {}
        }
    }
    peek.finish()
}

/// The command a code-mode script runs: `tools.exec_command({cmd:"..."})`, the key quoted or not.
fn exec_command(script: &str) -> Option<String> {
    let quoted = ["exec_command({cmd:", "exec_command({\"cmd\":"]
        .iter()
        .find_map(|call| script.find(call).map(|at| &script[at + call.len()..]))?;
    let mut chars = quoted.char_indices();
    if chars.next()?.1 != '"' {
        return None;
    }
    let mut escaped = false;
    let end = chars.find_map(|(at, c)| {
        let closes = c == '"' && !escaped;
        escaped = c == '\\' && !escaped;
        closes.then_some(at)
    })?;
    serde_json::from_str(&quoted[..=end]).ok()
}

/// The first file a code-mode script's `tools.apply_patch` adds, changes or deletes.
fn patched(script: &str) -> Option<String> {
    if !script.contains("tools.apply_patch(") {
        return None;
    }
    let (at, header) = ["*** Add File: ", "*** Update File: ", "*** Delete File: "]
        .iter()
        .filter_map(|header| script.find(header).map(|at| (at, header)))
        .min()?;
    let rest = &script[at + header.len()..];
    let end = rest.find(['\\', '"', '\n']).unwrap_or(rest.len());
    Some(rest[..end].to_string())
}

/// What a function call works on: its task, command or path.
fn target(arguments: &Value) -> String {
    ["task_name", "cmd", "command", "path"]
        .iter()
        .find_map(|key| arguments[*key].as_str())
        .map(first)
        .unwrap_or_default()
}

/// The first non-empty line of a tool's output, after the "Script completed ... Output:" header
/// code mode puts first.
fn output(output: &Value) -> Option<String> {
    let text = match output {
        Value::String(text) => text.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|part| part["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => return None,
    };
    let body = match text.split_once("\nOutput:\n") {
        Some((header, body)) if header.starts_with("Script ") => body,
        _ => text.as_str(),
    };
    body.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(String::from)
}

fn first(text: &str) -> String {
    text.lines().next().unwrap_or("").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::peek::{Format, tail};

    fn rollout() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src/ui/peek/fixtures/codex-rollout.jsonl")
    }

    fn bash(command: &str) -> PeekLine {
        PeekLine::Tool {
            name: "Bash".into(),
            target: command.into(),
        }
    }

    #[test]
    fn reads_what_codex_said_and_ran_with_each_result() {
        let peek = tail(&rollout(), 100, Format::Codex).unwrap();

        assert_eq!(
            peek[peek.len() - 5..],
            [
                PeekLine::Text("Do you prefer tabs or spaces?".into()),
                PeekLine::Text(
                    "I’ll run `sleep 40` in the shell, then report when it finishes.".into()
                ),
                bash("sleep 40"),
                PeekLine::Result("aborted by user after 9.4s".into()),
                PeekLine::Text("ok".into()),
            ]
        );
        let at = |line: &PeekLine| peek.iter().position(|item| item == line).unwrap();
        let gh = at(&bash("gh pr create --fill"));
        assert_eq!(
            peek[gh + 1],
            PeekLine::Result("no git remotes found".into())
        );
        let spawn = at(&PeekLine::Tool {
            name: "spawn_agent".into(),
            target: "count_files".into(),
        });
        assert!(matches!(peek[spawn + 1], PeekLine::Result(_)));
        assert!(
            !peek
                .iter()
                .any(|line| matches!(line, PeekLine::Text(text) if text.contains("Say ok"))),
            "what the person typed is not the agent's"
        );
    }

    #[test]
    fn shows_a_command_still_running() {
        let call = serde_json::json!({"type": "response_item", "payload": {
            "type": "custom_tool_call", "name": "exec", "call_id": "c1",
            "input": "const r = await tools.exec_command({cmd:\"cargo test \\\"a b\\\"\",yield_time_ms:1});\ntext(r.output);"
        }});

        assert_eq!(
            parse(&call.to_string(), false),
            [PeekLine::Now("Bash(cargo test \"a b\")".into())]
        );
    }

    #[test]
    fn reads_a_command_whose_key_is_quoted() {
        let script =
            r#"const r = await tools.exec_command({"cmd":"python3 -m unittest","workdir":"/w"});"#;

        assert_eq!(exec_command(script).as_deref(), Some("python3 -m unittest"));
    }

    #[test]
    fn shows_a_patch_as_an_edit_of_its_first_file() {
        let call = serde_json::json!({"type": "response_item", "payload": {
            "type": "custom_tool_call", "name": "exec", "call_id": "c1",
            "input": "const patch = \"*** Begin Patch\\n*** Update File: /w/shop/orders.py\\n@@\\n-a\\n+b\\n*** Add File: /w/shop/mailer.py\\n+c\\n*** End Patch\";\nconst r = await tools.apply_patch(patch);\ntext(r);\n"
        }});

        assert_eq!(
            parse(&call.to_string(), false),
            [PeekLine::Now("Edit(/w/shop/orders.py)".into())]
        );
    }
}
