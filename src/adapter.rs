//! `Adapter`: a harness's wiring, its instructions and its hook events as flow events

mod answer;
pub mod calls;
pub mod claude_code;
pub mod codex;

use std::path::Path;

use serde::Deserialize;
use serde_json::Value;

use crate::event::EventKind;
use crate::workflow::Workflow;

/// The longest text an adapter puts into an event: a question, a reason, a description.
pub const REPORT_TEXT_LIMIT: usize = 200;

/// What a worker is told about the master, added to its instructions: the master's messages arrive
/// as hook feedback or as a prompt saaio types in, which it would rightly not take as the user's
/// words otherwise.
pub const WORKER_PROMPT: &str = "You run as a worker of saaio, in your own git worktree. The saaio \
master is the agent session that coordinates the workers for the user. Its messages reach you as \
hook feedback or as a prompt that starts with \"message from the saaio master:\"; they answer what \
you asked or pass on what the user said, so act on them as you would on the user's own answer. To \
ask the master or the user something, end your turn with the question.";

/// Codex has no Skill tool event; a single SKILL.md read is the phase signal.
const CODEX_PHASE_PROMPT: &str = "For each workflow skill, read its SKILL.md only when you begin \
working on that skill. Never read workflow skills ahead, even in separate commands. saaio \
observes the SKILL.md read to start the configured phase; a command reading several phase \
skills at once changes no phase.";

/// What a fresh Codex master is asked first, so its first turn runs at once.
pub const MASTER_OPENING: &str = "saaio just started you. Run `saaio status` and say in one line \
what needs the user, if anything.";

/// Where a session's turn stands after a hook event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Turn {
    /// It runs a turn (a prompt, a compaction): saaio types nothing into it.
    Began,
    /// It ended its turn at a Stop, and its hook can hand news over (exit 2).
    Ended,
    /// It waits at its prompt with no Stop (an Esc, the end of a compaction); its hook can hand
    /// nothing over.
    Idle,
}

/// Which lines of a file a reader looks for.
pub type LineMatch = dyn Fn(&str) -> bool;

/// Who answers a permission request a hook reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Approver {
    /// The user, in the pane.
    User,
    /// A reviewer the harness runs, which answers without the user.
    Reviewer,
}

/// A harness saaio observes through its own hooks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[cfg_attr(test, derive(serde::Serialize))]
#[serde(rename_all = "kebab-case")]
pub enum Adapter {
    ClaudeCode,
    Codex,
}

impl Adapter {
    /// "claude-code" -> ClaudeCode, "codex" -> Codex; None for a name saaio does not know.
    pub fn named(name: &str) -> Option<Adapter> {
        match name {
            "claude-code" => Some(Adapter::ClaudeCode),
            "codex" => Some(Adapter::Codex),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Adapter::ClaudeCode => "claude-code",
            Adapter::Codex => "codex",
        }
    }

    /// The arguments that make the harness report to `<flow_exe> hook <name>` and wait for the
    /// master's messages, inserted right after the program in the agent's argv, fresh and resumed;
    /// with `trusted`, Codex trusts that folder (else it asks in its pane).
    pub fn wiring(self, flow_exe: &Path, trusted: Option<&Path>) -> Vec<String> {
        match self {
            Adapter::ClaudeCode => vec!["--settings".to_string(), claude_code::settings(flow_exe)]
                .into_iter()
                .chain(self.instruct(WORKER_PROMPT))
                .collect(),
            Adapter::Codex => codex::hooks(flow_exe)
                .into_iter()
                .chain(trusted.map(codex::trusted_folder).unwrap_or_default())
                .chain(self.instruct(&format!("{WORKER_PROMPT} {CODEX_PHASE_PROMPT}")))
                .collect(),
        }
    }

    /// The arguments that make a plain session report to `<flow_exe> hook <name>`: no master
    /// waiter, no worker prompt, since it is the user's own; `trusted` as for `wiring`.
    pub fn plain_wiring(self, flow_exe: &Path, trusted: Option<&Path>) -> Vec<String> {
        match self {
            Adapter::ClaudeCode => vec![
                "--settings".to_string(),
                claude_code::plain_settings(flow_exe),
            ],
            Adapter::Codex => codex::hooks(flow_exe)
                .into_iter()
                .chain(trusted.map(codex::trusted_folder).unwrap_or_default())
                .collect(),
        }
    }

    /// The arguments that add `text` to the session's instructions.
    pub fn instruct(self, text: &str) -> Vec<String> {
        match self {
            Adapter::ClaudeCode => vec!["--append-system-prompt".to_string(), text.to_string()],
            Adapter::Codex => vec![
                "-c".to_string(),
                format!("developer_instructions={}", codex::toml_string(text)),
            ],
        }
    }

    /// True when the session wakes itself for news (Claude Code: an asyncRewake `saaio wait`); false
    /// when saaio hands news over at its Stop and saaio server types it in (Codex).
    pub fn wakes_itself(self) -> bool {
        match self {
            Adapter::ClaudeCode => true,
            Adapter::Codex => false,
        }
    }

    /// The master's command line, `argv` being its agent's: right after the program its hooks,
    /// which in the master's pane keep where its turn stands (Claude Code's also arm the
    /// `saaio wait --master` waiter; Codex's record its session and hand news over at Stop) and
    /// its instructions; and,
    /// when it starts `fresh`, an opening prompt for a harness that hears of news only after its
    /// first turn (Codex: before it, its pane may still show a trust prompt, so saaio types nothing).
    pub fn wire_master(
        self,
        mut argv: Vec<String>,
        flow_exe: &Path,
        instructions: &str,
        fresh: bool,
    ) -> Vec<String> {
        let mut wiring = match self {
            Adapter::ClaudeCode => vec![
                "--settings".to_string(),
                claude_code::master_settings(flow_exe),
            ],
            Adapter::Codex => codex::hooks(flow_exe),
        };
        wiring.extend(self.instruct(instructions));
        let at = 1.min(argv.len());
        argv.splice(at..at, wiring);
        if fresh && self == Adapter::Codex {
            argv.push(MASTER_OPENING.to_string());
        }
        argv
    }

    /// Where a hook event leaves the session's turn, so the server types only into an idle
    /// session; None for an event that is neither.
    pub fn turn(self, payload: &Value) -> Option<Turn> {
        match self {
            Adapter::ClaudeCode => claude_code::turn(payload),
            Adapter::Codex => codex::turn(payload),
        }
    }

    /// The payload is the hook before a tool call (PreToolUse in both harnesses).
    pub fn before_tool(self, payload: &Value) -> bool {
        match self {
            Adapter::ClaudeCode | Adapter::Codex => payload["hook_event_name"] == "PreToolUse",
        }
    }

    /// The payload is a tool call that asks the user (Claude Code's AskUserQuestion), so the
    /// flow waits on it rather than works.
    pub fn asks(self, payload: &Value) -> bool {
        match self {
            Adapter::ClaudeCode => {
                self.before_tool(payload) && payload["tool_name"] == "AskUserQuestion"
            }
            Adapter::Codex => false,
        }
    }

    /// Who answers this payload's permission request: Codex's automatic reviewer when the
    /// rollout's turn routes approvals to it, else the user. `last_line` reads the last line that
    /// matches in a file the payload names; it is called only for a Codex PermissionRequest.
    pub fn approver(
        self,
        payload: &Value,
        last_line: impl FnOnce(&Path, &LineMatch) -> Option<String>,
    ) -> Approver {
        match self {
            Adapter::ClaudeCode => Approver::User,
            Adapter::Codex => codex::approver(payload, last_line),
        }
    }

    /// The flow events one hook payload means; empty for one that means nothing. A permission
    /// request the reviewer answers waits for no one.
    pub fn translate(
        self,
        payload: &Value,
        workflow: &Workflow,
        approver: Approver,
    ) -> Vec<EventKind> {
        match self {
            Adapter::ClaudeCode => claude_code::translate(payload, workflow),
            Adapter::Codex => codex::translate(payload, workflow, approver),
        }
    }
}

/// Text cut to REPORT_TEXT_LIMIT chars.
pub fn cut(text: &str) -> String {
    text.chars().take(REPORT_TEXT_LIMIT).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_same_name_from_a_config_file_and_the_command_line() {
        let adapter: Adapter = serde_json::from_value(Value::from("claude-code")).unwrap();

        assert_eq!(adapter, Adapter::ClaudeCode);
        assert_eq!(Adapter::named(adapter.name()), Some(adapter));
    }

    #[test]
    fn knows_claude_code_by_name() {
        assert_eq!(Adapter::named("claude-code"), Some(Adapter::ClaudeCode));
        assert_eq!(Adapter::ClaudeCode.name(), "claude-code");
        assert_eq!(Adapter::named("codex"), Some(Adapter::Codex));
        assert_eq!(Adapter::Codex.name(), "codex");
        assert_eq!(Adapter::named("pi"), None);
    }

    #[test]
    fn tells_a_worker_who_the_flow_master_is() {
        assert!(WORKER_PROMPT.contains("message from the saaio master:"));
        assert!(
            !WORKER_PROMPT.contains("Claude"),
            "the prompt names no harness"
        );
        let wiring =
            Adapter::ClaudeCode.wiring(Path::new("/bin/saaio"), Some(Path::new("/worktree")));
        assert_eq!(wiring[0], "--settings");
        assert_eq!(wiring[2..], ["--append-system-prompt", WORKER_PROMPT]);
    }

    #[test]
    fn claude_code_takes_instructions_on_its_system_prompt() {
        assert_eq!(
            Adapter::ClaudeCode.instruct("be brief"),
            ["--append-system-prompt", "be brief"]
        );
    }

    #[test]
    fn codex_takes_instructions_as_config_and_its_hooks_on_every_launch() {
        let exe = Path::new("/bin/saaio");
        let instructions = Adapter::Codex.instruct("be \"brief\"\nplease");
        assert_eq!(instructions[0], "-c");
        let parsed: toml::Table = toml::from_str(&instructions[1]).unwrap();
        assert_eq!(
            parsed["developer_instructions"].as_str(),
            Some("be \"brief\"\nplease")
        );

        let folder = Path::new("/worktree");
        let worker = Adapter::Codex.wiring(exe, Some(folder));
        let worker_prompt: toml::Table = toml::from_str(&worker[worker.len() - 1]).unwrap();
        let prompt = worker_prompt["developer_instructions"].as_str().unwrap();
        assert!(prompt.contains("read its SKILL.md only when you begin working on that skill"));
        assert!(prompt.contains("Never read workflow skills ahead"));
        assert!(!prompt.contains("saaio report phase"));
        assert_eq!(worker[..worker.len() - 4], codex::hooks(exe));
        assert_eq!(
            worker[worker.len() - 4..worker.len() - 2],
            codex::trusted_folder(folder)
        );
        assert_eq!(
            worker[worker.len() - 2..],
            Adapter::Codex.instruct(&format!("{WORKER_PROMPT} {CODEX_PHASE_PROMPT}"))
        );
        assert_eq!(
            Adapter::Codex.plain_wiring(exe, Some(folder)),
            codex::hooks(exe)
                .into_iter()
                .chain(codex::trusted_folder(folder))
                .collect::<Vec<_>>()
        );
        let untrusted = Adapter::Codex.wiring(exe, None);
        assert!(!untrusted.iter().any(|arg| arg.starts_with("projects=")));
        assert_eq!(untrusted.len(), worker.len() - 2);
        assert_eq!(Adapter::Codex.plain_wiring(exe, None), codex::hooks(exe));
        assert!(!Adapter::Codex.wakes_itself());
        assert!(Adapter::ClaudeCode.wakes_itself());
    }

    #[test]
    fn wires_each_master_its_own_way() {
        let exe = Path::new("/bin/saaio");
        let argv = || vec!["agent".to_string(), "-n".to_string(), "master".to_string()];

        let claude = Adapter::ClaudeCode.wire_master(argv(), exe, "md", true);
        assert_eq!(
            claude,
            [
                "agent",
                "--settings",
                &claude_code::master_settings(exe),
                "--append-system-prompt",
                "md",
                "-n",
                "master"
            ]
        );

        let fresh = Adapter::Codex.wire_master(argv(), exe, "md", true);
        let hooks = codex::hooks(exe);
        assert_eq!(fresh[1..=hooks.len()], hooks[..]);
        assert_eq!(fresh.last().map(String::as_str), Some(MASTER_OPENING));
        let resumed = Adapter::Codex.wire_master(argv(), exe, "md", false);
        assert_eq!(resumed.last().map(String::as_str), Some("master"));
    }

    #[test]
    fn reads_where_a_codex_turn_stands() {
        let event = |name: &str| serde_json::json!({ "hook_event_name": name });

        assert_eq!(
            Adapter::Codex.turn(&event("UserPromptSubmit")),
            Some(Turn::Began)
        );
        assert_eq!(Adapter::Codex.turn(&event("Stop")), Some(Turn::Ended));
        assert_eq!(Adapter::Codex.turn(&event("Interrupt")), Some(Turn::Idle));
        assert_eq!(Adapter::Codex.turn(&event("PreCompact")), Some(Turn::Began));
        assert_eq!(
            Adapter::Codex.turn(&event("PostCompact")),
            Some(Turn::Idle),
            "no Stop follows a compaction"
        );
        assert_eq!(Adapter::Codex.turn(&event("PreToolUse")), None);
        let subagent = serde_json::json!({ "hook_event_name": "Stop", "agent_id": "01a1" });
        assert_eq!(
            Adapter::Codex.turn(&subagent),
            None,
            "a subagent's turn is not the session's"
        );
        assert_eq!(Adapter::ClaudeCode.turn(&event("Stop")), Some(Turn::Ended));
    }

    #[test]
    fn knows_the_hook_before_a_tool_call() {
        let event =
            |name: &str| serde_json::json!({ "hook_event_name": name, "tool_name": "Bash" });

        for adapter in [Adapter::ClaudeCode, Adapter::Codex] {
            assert!(adapter.before_tool(&event("PreToolUse")));
            for other in [
                "PostToolUse",
                "UserPromptSubmit",
                "PermissionRequest",
                "Stop",
            ] {
                assert!(!adapter.before_tool(&event(other)), "{other}");
            }
        }
    }

    #[test]
    fn only_a_tool_call_that_asks_the_user_asks() {
        let ask =
            serde_json::json!({"hook_event_name": "PreToolUse", "tool_name": "AskUserQuestion"});
        let bash = serde_json::json!({"hook_event_name": "PreToolUse", "tool_name": "Bash"});
        let permission = serde_json::json!({"hook_event_name": "PermissionRequest", "tool_name": "AskUserQuestion"});

        assert!(Adapter::ClaudeCode.asks(&ask));
        assert!(!Adapter::ClaudeCode.asks(&bash));
        assert!(!Adapter::ClaudeCode.asks(&permission));
        assert!(!Adapter::Codex.asks(&ask));
    }

    #[test]
    fn cuts_long_texts_on_characters() {
        let long = "界".repeat(REPORT_TEXT_LIMIT + 5);

        assert_eq!(cut(&long).chars().count(), REPORT_TEXT_LIMIT);
        assert_eq!(cut("short"), "short");
    }
}
