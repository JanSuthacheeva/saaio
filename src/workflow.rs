use serde::Deserialize;
#[cfg(test)]
use serde::Serialize;

use crate::event::Phase;

/// The most phases a workflow has: the steps row and the built-in colors have room for four.
pub const MAX_PHASES: usize = 4;

/// One [[phases]] entry: its name and the skills that start it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[cfg_attr(test, derive(Serialize))]
#[serde(deny_unknown_fields)]
pub struct PhaseSpec {
    pub name: Phase,
    #[serde(default)]
    pub skills: Vec<String>,
}

/// 1 to MAX_PHASES phases with distinct names, no skill in two.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[cfg_attr(test, derive(Serialize), serde(into = "Vec<PhaseSpec>"))]
#[serde(try_from = "Vec<PhaseSpec>")]
pub struct Workflow {
    phases: Vec<PhaseSpec>,
}

impl Default for Workflow {
    fn default() -> Workflow {
        Workflow::builtin()
    }
}

impl TryFrom<Vec<PhaseSpec>> for Workflow {
    type Error = String;

    fn try_from(phases: Vec<PhaseSpec>) -> Result<Workflow, String> {
        if phases.is_empty() || phases.len() > MAX_PHASES {
            return Err(format!("a workflow has 1 to {MAX_PHASES} phases"));
        }
        for (index, phase) in phases.iter().enumerate() {
            let earlier = &phases[..index];
            if earlier.iter().any(|other| other.name == phase.name) {
                return Err(format!("two phases are named {}", phase.name.name()));
            }
            let mut skills = earlier.iter().flat_map(|other| &other.skills);
            if let Some(skill) = skills.find(|skill| phase.skills.contains(skill)) {
                return Err(format!("{skill} starts two phases"));
            }
        }
        Ok(Workflow { phases })
    }
}

#[cfg(test)]
impl From<Workflow> for Vec<PhaseSpec> {
    fn from(workflow: Workflow) -> Vec<PhaseSpec> {
        workflow.phases
    }
}

impl Workflow {
    /// plan design-code, implement implement-plan, review review-changes, merge-request
    /// write-merge-request: the agent-skills chain.
    pub fn builtin() -> Workflow {
        let spec = |name: &str, skill: &str| PhaseSpec {
            name: name.parse().expect("a built-in phase name is valid"),
            skills: vec![skill.to_string()],
        };
        Workflow {
            phases: vec![
                spec("plan", "design-code"),
                spec("implement", "implement-plan"),
                spec("review", "review-changes"),
                spec("merge-request", "write-merge-request"),
            ],
        }
    }

    /// A workflow of (name, skills) a test knows is valid.
    #[cfg(test)]
    pub fn of(phases: &[(&str, &[&str])]) -> Workflow {
        let specs = phases
            .iter()
            .map(|(name, skills)| PhaseSpec {
                name: Phase::of(name),
                skills: skills.iter().copied().map(String::from).collect(),
            })
            .collect::<Vec<_>>();
        Workflow::try_from(specs).expect("a valid workflow")
    }

    /// The names in order: what flow start records.
    pub fn phases(&self) -> Vec<Phase> {
        self.phases.iter().map(|phase| phase.name.clone()).collect()
    }

    /// The phase a skill starts: "/design-code" and "plugin:design-code" both find design-code's.
    pub fn phase_of(&self, skill: &str) -> Option<Phase> {
        let name = skill.trim_start_matches('/');
        let name = name.rsplit(':').next().unwrap_or(name);
        self.phases
            .iter()
            .find(|phase| phase.skills.iter().any(|known| known == name))
            .map(|phase| phase.name.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(name: &str, skills: &[&str]) -> PhaseSpec {
        PhaseSpec {
            name: Phase::of(name),
            skills: skills.iter().copied().map(String::from).collect(),
        }
    }

    #[test]
    fn the_builtin_workflow_is_the_agent_skills_chain() {
        let workflow = Workflow::builtin();

        assert_eq!(
            workflow.phases(),
            ["plan", "implement", "review", "merge-request"].map(Phase::of)
        );
        assert_eq!(workflow.phase_of("design-code"), Some(Phase::of("plan")));
        assert_eq!(
            workflow.phase_of("write-merge-request"),
            Some(Phase::of("merge-request"))
        );
        assert_eq!(Workflow::default(), workflow);
    }

    #[test]
    fn finds_the_phase_of_a_skill_with_or_without_a_slash_or_plugin() {
        let workflow = Workflow::builtin();

        assert_eq!(
            workflow.phase_of("/implement-plan"),
            Some(Phase::of("implement"))
        );
        assert_eq!(
            workflow.phase_of("buzzwoo-standard:design-code"),
            Some(Phase::of("plan"))
        );
        assert_eq!(workflow.phase_of("commit"), None);
    }

    #[test]
    fn a_phase_can_have_several_skills_or_none() {
        let workflow = Workflow::try_from(vec![
            spec("spec", &[]),
            spec("build", &["executing-plans", "subagent-driven-development"]),
        ])
        .unwrap();

        assert_eq!(workflow.phases(), [Phase::of("spec"), Phase::of("build")]);
        assert_eq!(
            workflow.phase_of("subagent-driven-development"),
            Some(Phase::of("build"))
        );
    }

    #[test]
    fn has_one_to_four_phases() {
        let named = |count: usize| {
            (0..count)
                .map(|index| spec(&format!("p{index}"), &[]))
                .collect::<Vec<_>>()
        };

        assert_eq!(
            Workflow::try_from(named(0)).unwrap_err(),
            "a workflow has 1 to 4 phases"
        );
        assert_eq!(
            Workflow::try_from(named(5)).unwrap_err(),
            "a workflow has 1 to 4 phases"
        );
        assert!(Workflow::try_from(named(1)).is_ok());
        assert!(Workflow::try_from(named(4)).is_ok());
    }

    #[test]
    fn refuses_a_name_twice_or_a_skill_in_two_phases() {
        let twice = vec![spec("review", &[]), spec("review", &[])];
        let shared = vec![
            spec("plan", &["design-code"]),
            spec("build", &["design-code"]),
        ];

        assert_eq!(
            Workflow::try_from(twice).unwrap_err(),
            "two phases are named review"
        );
        assert_eq!(
            Workflow::try_from(shared).unwrap_err(),
            "design-code starts two phases"
        );
    }
}
