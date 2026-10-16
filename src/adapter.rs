mod answer;
mod calls;
pub mod claude_code;
pub mod codex;

use std::path::Path;

use serde::Deserialize;
use serde_json::Value;

use crate::config::PhaseMap;
use crate::event::{EventKind, Phase};

/// The longest text an adapter puts into an event: a question, a reason, a description.
pub const REPORT_TEXT_LIMIT: usize = 200;

/// What a worker is told about the master, added to its instructions: the master's messages arrive
/// as hook feedback or as a prompt flow types in, which it would rightly not take as the user's
/// words otherwise.
pub const WORKER_PROMPT: &str = "You run as a worker of flow, in your own git worktree. The flow \
master is the agent session that coordinates the workers for the user. Its messages reach you as \
hook feedback or as a prompt that starts with \"message from the flow master:\"; they answer what \
you asked or pass on what the user said, so act on them as you would on the user's own answer. To \
ask the master or the user something, end your turn with the question.";

/// A harness flow observes through its own hooks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Adapter {
    ClaudeCode,
    Codex,
}

impl Adapter {
    /// "claude-code" -> ClaudeCode, "codex" -> Codex; None for a name flow does not know.
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
    /// master's messages, inserted right after the program in the agent's argv, fresh and resumed.
    pub fn wiring(self, flow_exe: &Path) -> Vec<String> {
        match self {
            Adapter::ClaudeCode => vec!["--settings".to_string(), claude_code::settings(flow_exe)]
                .into_iter()
                .chain(self.instruct(WORKER_PROMPT))
                .collect(),
            Adapter::Codex => codex::hooks(flow_exe, false)
                .into_iter()
                .chain(self.instruct(WORKER_PROMPT))
                .collect(),
        }
    }

    /// The arguments that make a plain session report to `<flow_exe> hook <name>`: no master
    /// waiter, no worker prompt, since it is the user's own.
    pub fn plain_wiring(self, flow_exe: &Path) -> Vec<String> {
        match self {
            Adapter::ClaudeCode => vec![
                "--settings".to_string(),
                claude_code::plain_settings(flow_exe),
            ],
            Adapter::Codex => codex::hooks(flow_exe, false),
        }
    }

    /// The arguments that bring the master its news, inserted right after the program: Claude
    /// Code's `<flow_exe> wait --master` waiter; Codex's hooks with --master, which record its
    /// session and hand news over at Stop. The master is no flow, so nothing else.
    pub fn master_wiring(self, flow_exe: &Path) -> Vec<String> {
        match self {
            Adapter::ClaudeCode => vec![
                "--settings".to_string(),
                claude_code::master_settings(flow_exe),
            ],
            Adapter::Codex => codex::hooks(flow_exe, true),
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

    /// True when the session wakes itself for news (Claude Code: an asyncRewake `flow wait`); false
    /// when flow hands news over at its Stop and flow server types it in (Codex).
    pub fn wakes_itself(self) -> bool {
        match self {
            Adapter::ClaudeCode => true,
            Adapter::Codex => false,
        }
    }

    /// The flow events one hook payload means; empty for one that means nothing.
    pub fn translate(self, payload: &Value, phases: &PhaseMap) -> Vec<EventKind> {
        match self {
            Adapter::ClaudeCode => claude_code::translate(payload, phases),
            Adapter::Codex => codex::translate(payload, phases),
        }
    }
}

/// The phase a skill starts: "/design-code" and "plugin:design-code" both look up "design-code".
pub fn phase_of(skill: &str, phases: &PhaseMap) -> Option<Phase> {
    let name = skill.trim_start_matches('/');
    let name = name.rsplit(':').next().unwrap_or(name);
    phases.get(name).copied()
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

    fn phases() -> PhaseMap {
        PhaseMap::from([
            ("design-code".to_string(), Phase::Design),
            ("implement-plan".to_string(), Phase::Implement),
        ])
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
    fn finds_the_phase_of_a_skill_with_or_without_a_slash_or_plugin() {
        let phases = phases();

        assert_eq!(phase_of("design-code", &phases), Some(Phase::Design));
        assert_eq!(phase_of("/implement-plan", &phases), Some(Phase::Implement));
        assert_eq!(
            phase_of("buzzwoo-standard:design-code", &phases),
            Some(Phase::Design)
        );
        assert_eq!(phase_of("commit", &phases), None);
    }

    #[test]
    fn tells_a_worker_who_the_flow_master_is() {
        assert!(WORKER_PROMPT.contains("message from the flow master:"));
        assert!(
            !WORKER_PROMPT.contains("Claude"),
            "the prompt names no harness"
        );
        let wiring = Adapter::ClaudeCode.wiring(Path::new("/bin/flow"));
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
        let exe = Path::new("/bin/flow");
        let instructions = Adapter::Codex.instruct("be \"brief\"\nplease");
        assert_eq!(instructions[0], "-c");
        let parsed: toml::Table = toml::from_str(&instructions[1]).unwrap();
        assert_eq!(
            parsed["developer_instructions"].as_str(),
            Some("be \"brief\"\nplease")
        );

        let worker = Adapter::Codex.wiring(exe);
        assert_eq!(worker[..worker.len() - 2], codex::hooks(exe, false));
        assert_eq!(
            worker[worker.len() - 2..],
            Adapter::Codex.instruct(WORKER_PROMPT)
        );
        assert_eq!(Adapter::Codex.plain_wiring(exe), codex::hooks(exe, false));
        assert_eq!(Adapter::Codex.master_wiring(exe), codex::hooks(exe, true));
        assert!(!Adapter::Codex.wakes_itself());
        assert!(Adapter::ClaudeCode.wakes_itself());
    }

    #[test]
    fn cuts_long_texts_on_characters() {
        let long = "界".repeat(REPORT_TEXT_LIMIT + 5);

        assert_eq!(cut(&long).chars().count(), REPORT_TEXT_LIMIT);
        assert_eq!(cut("short"), "short");
    }
}
