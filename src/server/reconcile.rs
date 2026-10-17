use std::collections::HashSet;

use crate::config::Config;
use crate::launch::pane::{self, LaunchEnv, PaneSpec};
use crate::proto::PaneKey;
use crate::view::FlowView;

/// What the server should open and close to match the flows on the board.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Wanted {
    pub open: Vec<PaneSpec>,
    pub close: Vec<PaneKey>,
    pub failed: Vec<(PaneKey, String)>,
}

/// Why worker_spec found no command for an agent: it has no way to resume.
pub fn unopenable(name: &str) -> String {
    format!("agent {name} cannot resume a session")
}

/// Opens every unfinished flow with a workspace not in known; closes worker panes whose flow is gone;
/// failed: flows it cannot build a spec for (unknown agent, no resume template), with the reason.
pub fn reconcile(
    flows: &[FlowView],
    open: &[PaneKey],
    known: &HashSet<String>,
    config: &Config,
    env: &LaunchEnv,
) -> Wanted {
    let mut wanted = Wanted::default();
    for flow in flows {
        let Some(space) = &flow.workspace else {
            continue;
        };
        let key = PaneKey::Flow(flow.id.clone());
        if flow.is_finished() || known.contains(&flow.id) || open.contains(&key) {
            continue;
        }
        let Some((name, agent)) = config.agent(Some(&space.agent)) else {
            wanted
                .failed
                .push((key, format!("no agent named {}", space.agent)));
            continue;
        };
        match pane::worker_spec(flow, agent, env) {
            Some(spec) => wanted.open.push(spec),
            None => wanted.failed.push((key, unopenable(name))),
        }
    }
    for key in open {
        if let PaneKey::Flow(id) = key
            && !flows.iter().any(|flow| &flow.id == id)
        {
            wanted.close.push(key.clone());
        }
    }
    wanted
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Agent, builtin_agents};
    use crate::event::{Event, EventKind, Mode};
    use crate::fold;
    use crate::launch::pane::HOSTED;

    fn config() -> Config {
        let mut agents = builtin_agents();
        agents.insert(
            "aider".into(),
            Agent {
                command: vec!["aider".into()],
                prompt: String::new(),
                adapter: None,
                resume: None,
            },
        );
        Config {
            agents,
            workflow: crate::workflow::Workflow::builtin(),
            ..Config::default()
        }
    }

    fn flow(id: &str, agent: &str, window: Option<&str>, ended: bool) -> FlowView {
        let mut events = vec![
            Event::new(
                0,
                EventKind::Started {
                    name: id.into(),
                    request: "x".into(),
                    mode: Mode::Auto,
                    phases: None,
                },
            ),
            Event::new(
                1,
                EventKind::Workspace {
                    path: format!("/repo/.flow/worktrees/{id}"),
                    branch: Some(format!("flow/{id}")),
                    slot: Some(1),
                    window: window.map(String::from),
                    agent: agent.into(),
                },
            ),
        ];
        if ended {
            events.push(Event::new(2, EventKind::Ended));
        }
        fold::fold(id, &events, None, 3).unwrap()
    }

    fn unstarted(id: &str) -> FlowView {
        let started = Event::new(
            0,
            EventKind::Started {
                name: id.into(),
                request: "x".into(),
                mode: Mode::Guided,
                phases: None,
            },
        );
        fold::fold(id, &[started], None, 1).unwrap()
    }

    fn keys(wanted: &Wanted) -> Vec<PaneKey> {
        wanted.open.iter().map(|spec| spec.key.clone()).collect()
    }

    fn worker(id: &str) -> PaneKey {
        PaneKey::Flow(id.into())
    }

    #[test]
    fn opens_unfinished_flows_with_a_workspace_it_has_not_opened() {
        let flows = [
            flow("new", "claude", None, false),
            flow("hosted", "claude", Some(HOSTED), false),
            flow("ended", "claude", None, true),
            unstarted("plain"),
        ];

        let wanted = reconcile(
            &flows,
            &[PaneKey::Master],
            &HashSet::new(),
            &config(),
            &LaunchEnv::default(),
        );

        assert_eq!(keys(&wanted), vec![worker("new"), worker("hosted")]);
        assert!(wanted.open[1].argv.contains(&"--resume".to_string()));
        assert!(wanted.close.is_empty());
        assert!(wanted.failed.is_empty());
    }

    #[test]
    fn leaves_alone_what_is_known_or_already_open() {
        let flows = [
            flow("seen", "claude", Some(HOSTED), false),
            flow("running", "claude", Some(HOSTED), false),
        ];
        let known = HashSet::from(["seen".to_string()]);

        let wanted = reconcile(
            &flows,
            &[worker("running")],
            &known,
            &config(),
            &LaunchEnv::default(),
        );

        assert_eq!(wanted, Wanted::default());
    }

    #[test]
    fn closes_worker_panes_whose_flow_is_gone_but_never_the_master() {
        let flows = [flow("kept", "claude", Some(HOSTED), true)];

        let wanted = reconcile(
            &flows,
            &[PaneKey::Master, worker("kept"), worker("removed")],
            &HashSet::from(["kept".to_string(), "removed".to_string()]),
            &config(),
            &LaunchEnv::default(),
        );

        assert_eq!(wanted.close, vec![worker("removed")]);
        assert!(wanted.open.is_empty());
    }

    #[test]
    fn fails_flows_with_an_unknown_agent_or_no_way_to_resume() {
        let flows = [
            flow("gone-agent", "pi", None, false),
            flow("no-resume", "aider", Some(HOSTED), false),
            flow("fresh-aider", "aider", None, false),
        ];

        let wanted = reconcile(
            &flows,
            &[],
            &HashSet::new(),
            &config(),
            &LaunchEnv::default(),
        );

        assert_eq!(keys(&wanted), vec![worker("fresh-aider")]);
        assert_eq!(
            wanted.failed,
            vec![
                (worker("gone-agent"), "no agent named pi".to_string()),
                (
                    worker("no-resume"),
                    "agent aider cannot resume a session".to_string()
                ),
            ]
        );
    }
}
