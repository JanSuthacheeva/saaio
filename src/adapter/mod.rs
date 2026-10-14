pub mod claude_code;

use std::path::Path;

use serde_json::Value;

use crate::config::PhaseMap;
use crate::event::{EventKind, Phase};

/// The longest text an adapter puts into an event: a question, a reason, a description.
pub const REPORT_TEXT_LIMIT: usize = 200;

/// A harness flow observes through its own hooks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Adapter {
    ClaudeCode,
}

impl Adapter {
    /// "claude-code" -> ClaudeCode; None for a name flow does not know.
    pub fn named(name: &str) -> Option<Adapter> {
        match name {
            "claude-code" => Some(Adapter::ClaudeCode),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Adapter::ClaudeCode => "claude-code",
        }
    }

    /// The arguments that make the harness report to `<flow_exe> hook <name>`, inserted right after
    /// the program in the agent's argv, fresh and resumed.
    pub fn wiring(self, flow_exe: &Path) -> Vec<String> {
        match self {
            Adapter::ClaudeCode => vec!["--settings".to_string(), claude_code::settings(flow_exe)],
        }
    }

    /// The flow events one hook payload means; empty for one that means nothing.
    pub fn translate(self, payload: &Value, phases: &PhaseMap) -> Vec<EventKind> {
        match self {
            Adapter::ClaudeCode => claude_code::translate(payload, phases),
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
        assert_eq!(Adapter::named("codex"), None);
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
    fn cuts_long_texts_on_characters() {
        let long = "界".repeat(REPORT_TEXT_LIMIT + 5);

        assert_eq!(cut(&long).chars().count(), REPORT_TEXT_LIMIT);
        assert_eq!(cut("short"), "short");
    }
}
