//! `Kind`: a kind of flow, `[kinds.<name>]`: its workflow, its first prompt and its branch

use std::collections::BTreeMap;

use serde::Deserialize;

use super::Agent;
use crate::text::fill;
use crate::workflow::{self, PhaseSpec, Workflow};

/// The kind saaio start gives a flow unless [worker] kind or --kind names another.
pub const DEFAULT_KIND: &str = "feature";

/// What a kind of flow walks through, how it is asked first, and the branch it gets.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[cfg_attr(test, derive(serde::Serialize))]
#[serde(deny_unknown_fields)]
pub struct Kind {
    pub phases: Workflow,
    /// The skill its first prompt calls, as {skill}.
    #[serde(default)]
    pub skill: Option<String>,
    /// Its first prompt; unset, "{skill} {request}", or "{request}" without a skill.
    #[serde(default)]
    pub prompt: Option<String>,
    /// The branch types it means, the first the repo lists winning.
    #[serde(default)]
    pub branch_types: Vec<String>,
    /// false: a detached worktree, nothing to merge.
    #[serde(default = "with_branch")]
    pub branch: bool,
}

fn with_branch() -> bool {
    true
}

impl Kind {
    /// feature, fix and research.
    pub fn builtins() -> BTreeMap<String, Kind> {
        let fix = Kind {
            phases: workflow(&[
                ("diagnose", Some("diagnose-bug")),
                ("implement", Some("implement-plan")),
                ("review", Some("review-changes")),
                ("merge-request", Some("write-merge-request")),
            ]),
            skill: Some("ship-fix".to_string()),
            prompt: None,
            branch_types: names(&["fix", "bugfix", "hotfix"]),
            branch: true,
        };
        let research = Kind {
            phases: workflow(&[("research", None)]),
            skill: None,
            prompt: Some(
                "Research this and answer here. Change no code and commit nothing: {request}"
                    .to_string(),
            ),
            branch_types: Vec::new(),
            branch: false,
        };
        BTreeMap::from([
            (DEFAULT_KIND.to_string(), Kind::feature()),
            ("fix".to_string(), fix),
            ("research".to_string(), research),
        ])
    }

    /// The built-in feature kind: ship-feature through the agent-skills chain.
    pub fn feature() -> Kind {
        Kind {
            phases: Workflow::builtin(),
            skill: Some("ship-feature".to_string()),
            prompt: None,
            branch_types: names(&["feature", "feat"]),
            branch: true,
        }
    }

    /// The first prompt `agent` gets: its own, else this kind's, else "{skill} {request}"
    /// ("{request}" without a skill); {skill} as the agent's harness calls it, {request}, {id}
    /// and {name} left for the pane. Without the skill, a prompt that would start by calling it
    /// is `fallback_prompt` instead, since Claude Code runs no turn for an unknown command.
    pub fn prompt_for(&self, agent: &Agent, has_skill: bool) -> String {
        if let Some(prompt) = &agent.prompt {
            return prompt.clone();
        }
        let template = match (&self.prompt, &self.skill) {
            (Some(prompt), _) => prompt.as_str(),
            (None, Some(_)) => "{skill} {request}",
            (None, None) => "{request}",
        };
        let call = self.skill.as_deref().map(|skill| match agent.adapter {
            Some(adapter) => adapter.skill_call(skill),
            None => skill.to_string(),
        });
        let Some(call) = call else {
            return template.to_string();
        };
        let prompt = fill(template, &[("skill", &call)]);
        if has_skill || !prompt.starts_with(&call) {
            return prompt;
        }
        self.fallback_prompt()
    }

    /// The first prompt when the agent lacks the kind's skill: names no skill.
    pub fn fallback_prompt(&self) -> String {
        match self.skill.as_deref() {
            Some("ship-feature") => FEATURE_FALLBACK,
            Some("ship-fix") => FIX_FALLBACK,
            _ => "{request}",
        }
        .to_string()
    }
}

const FEATURE_FALLBACK: &str = "Build this: plan it, implement it with tests, commit as you go, \
    then ask before you push and open a merge request. {request}";

const FIX_FALLBACK: &str = "Fix this bug: reproduce it, find its cause, fix it with a regression \
    test, commit, then ask before you push and open a merge request. {request}";

/// A kind's name, as a phase's: lowercase letters, digits and '-'.
pub fn check_name(name: &str) -> Result<(), String> {
    workflow::check_name(name, "kind")
}

/// A built-in workflow of (name, skill).
fn workflow(phases: &[(&str, Option<&str>)]) -> Workflow {
    let specs = phases
        .iter()
        .map(|(name, skill)| PhaseSpec {
            name: name.parse().expect("a built-in phase name is valid"),
            skills: skill.iter().copied().map(String::from).collect(),
        })
        .collect::<Vec<_>>();
    Workflow::try_from(specs).expect("a built-in workflow is valid")
}

fn names(parts: &[&str]) -> Vec<String> {
    parts.iter().copied().map(String::from).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::Adapter;
    use crate::workflow::Phase;

    fn agent(adapter: Option<Adapter>, prompt: Option<&str>) -> Agent {
        Agent {
            command: vec!["agent".to_string(), "{prompt}".to_string()],
            prompt: prompt.map(String::from),
            resume: None,
            adapter,
        }
    }

    #[test]
    fn the_builtins_are_feature_fix_and_research() {
        let kinds = Kind::builtins();

        assert_eq!(
            kinds.keys().collect::<Vec<_>>(),
            ["feature", "fix", "research"]
        );
        assert_eq!(kinds["feature"], Kind::feature());
        assert_eq!(
            kinds["fix"].phases.phases(),
            ["diagnose", "implement", "review", "merge-request"].map(Phase::of)
        );
        assert_eq!(
            kinds["fix"].phases.phase_of("diagnose-bug"),
            Some(Phase::of("diagnose"))
        );
        assert_eq!(kinds["fix"].branch_types, ["fix", "bugfix", "hotfix"]);
        assert_eq!(kinds["research"].phases.phases(), [Phase::of("research")]);
        assert!(!kinds["research"].branch);
        assert!(kinds["feature"].branch && kinds["fix"].branch);
    }

    #[test]
    fn calls_the_skill_as_the_agents_harness_does() {
        let fix = &Kind::builtins()["fix"];

        let claude = agent(Some(Adapter::ClaudeCode), None);
        let codex = agent(Some(Adapter::Codex), None);
        assert_eq!(fix.prompt_for(&claude, true), "/ship-fix {request}");
        assert_eq!(fix.prompt_for(&codex, true), "$ship-fix {request}");
        assert_eq!(
            fix.prompt_for(&agent(None, None), true),
            "ship-fix {request}"
        );
        assert_eq!(
            Kind::feature().prompt_for(&claude, true),
            "/ship-feature {request}"
        );
    }

    #[test]
    fn the_agents_prompt_wins_then_the_kinds() {
        let mut fix = Kind::builtins()["fix"].clone();
        fix.prompt = Some("{skill} now: {request} ({name})".to_string());
        let claude = agent(Some(Adapter::ClaudeCode), None);

        assert_eq!(
            fix.prompt_for(&claude, true),
            "/ship-fix now: {request} ({name})"
        );
        let own = agent(Some(Adapter::ClaudeCode), Some("do {request}"));
        assert_eq!(fix.prompt_for(&own, true), "do {request}");
    }

    #[test]
    fn a_kind_is_named_like_a_phase() {
        assert_eq!(check_name("hot-fix2"), Ok(()));
        for bad in ["", "Fix", "fix it", "a,b"] {
            assert!(check_name(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn a_kind_without_a_skill_asks_the_request_alone() {
        let research = &Kind::builtins()["research"];
        let claude = agent(Some(Adapter::ClaudeCode), None);

        assert_eq!(
            research.prompt_for(&claude, true),
            "Research this and answer here. Change no code and commit nothing: {request}"
        );
        let mut bare = research.clone();
        bare.prompt = None;
        assert_eq!(bare.prompt_for(&claude, true), "{request}");
    }

    #[test]
    fn falls_back_without_the_skill() {
        let claude = agent(Some(Adapter::ClaudeCode), None);

        assert_eq!(
            Kind::feature().prompt_for(&claude, false),
            "Build this: plan it, implement it with tests, commit as you go, then ask before you \
             push and open a merge request. {request}"
        );
        let fix = &Kind::builtins()["fix"];
        assert_eq!(
            fix.prompt_for(&claude, false),
            "Fix this bug: reproduce it, find its cause, fix it with a regression test, commit, \
             then ask before you push and open a merge request. {request}"
        );
        let mut ghost = fix.clone();
        ghost.skill = Some("no-such-skill".to_string());
        assert_eq!(ghost.prompt_for(&claude, false), "{request}");
        ghost.prompt = Some("{skill} now: {request}".to_string());
        assert_eq!(ghost.prompt_for(&claude, false), "{request}");
    }

    #[test]
    fn a_prompt_that_does_not_start_with_the_skill_is_sent_as_written() {
        let mut fix = Kind::builtins()["fix"].clone();
        fix.prompt = Some("Use {skill} for {request}".to_string());
        let claude = agent(Some(Adapter::ClaudeCode), None);

        assert_eq!(
            fix.prompt_for(&claude, false),
            "Use /ship-fix for {request}"
        );
        let own = agent(Some(Adapter::ClaudeCode), Some("/ship-fix {request}"));
        assert_eq!(fix.prompt_for(&own, false), "/ship-fix {request}");
    }
}
