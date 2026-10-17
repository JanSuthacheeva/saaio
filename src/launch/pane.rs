use std::path::PathBuf;

use crate::config::Agent;
use crate::launch::worktree::{hook_env, repo_of};
use crate::proto::PaneKey;
use crate::view::{FlowView, Workspace};

/// Set to 1 in the master's pane, so the hooks it shares with workers know whose they are.
pub const MASTER_ENV: &str = "FLOW_MASTER";

/// Workspace.window of a worker flow ui has opened once; its next open resumes it.
pub const HOSTED: &str = "flow-ui";

/// What the server passes on to every pane: FLOW_HOME, when it was set for the server, and the
/// flow binary a worker's hooks run.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LaunchEnv {
    pub flow_home: Option<PathBuf>,
    pub flow_exe: PathBuf,
}

/// Exactly what a pane runs: its command line, directory and environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneSpec {
    pub key: PaneKey,
    pub argv: Vec<String>,
    pub cwd: PathBuf,
    pub env: Vec<(String, String)>,
}

/// The agent's command with {id}, {name}, {prompt}, {request} filled in ({prompt} itself filled first);
/// an argument that fills to nothing is dropped, so an agent without a prompt gets no empty one.
pub fn agent_argv(agent: &Agent, id: &str, name: &str, request: &str) -> Vec<String> {
    let template = agent.prompt.as_deref().unwrap_or_default();
    let prompt = fill(template, id, name, request, "", "");
    agent
        .command
        .iter()
        .map(|part| fill(part, id, name, request, &prompt, ""))
        .filter(|part| !part.is_empty())
        .collect()
}

/// The agent's command with {id} and {name} filled in and {prompt} and {request} dropped: a plain
/// session opens at an empty prompt.
pub fn plain_argv(agent: &Agent, id: &str, name: &str) -> Vec<String> {
    agent
        .command
        .iter()
        .map(|part| fill(part, id, name, "", "", ""))
        .filter(|part| !part.is_empty())
        .collect()
}

/// The agent's resume command with {id}, {name} and {session}, the id the harness resumes by,
/// filled in; None when it has none.
pub fn resume_argv(agent: &Agent, id: &str, name: &str, session: &str) -> Option<Vec<String>> {
    let resume = agent.resume.as_ref()?;
    Some(
        resume
            .iter()
            .map(|part| fill(part, id, name, "", "", session))
            .filter(|part| !part.is_empty())
            .collect(),
    )
}

/// The agent's command the first time, its resume command once it ran somewhere (window set:
/// HOSTED, or a tmux window from before flow ui), since its session id is taken from then on;
/// either way with the agent's adapter wired in right after the program. A plain session starts
/// without a prompt and gets the adapter's plain wiring. A harness that picks its own session
/// resumes by the one its hooks reported, and starts again when it never reported one (it had no
/// turn). None when the flow has no workspace, or must resume and the agent has no resume template.
pub fn worker_spec(flow: &FlowView, agent: &Agent, env: &LaunchEnv) -> Option<PaneSpec> {
    let space = flow.workspace.as_ref()?;
    let session = flow.session.as_ref().map(|session| session.id.as_str());
    let resumes = space.window.is_some() && (session.is_some() || !names_session(agent));
    let mut argv = if resumes {
        resume_argv(agent, &flow.id, &flow.name, session.unwrap_or(&flow.id))?
    } else if flow.is_plain() {
        plain_argv(agent, &flow.id, &flow.name)
    } else {
        agent_argv(agent, &flow.id, &flow.name, &flow.request)
    };
    if let Some(adapter) = agent.adapter {
        let wiring = if flow.is_plain() {
            adapter.plain_wiring(&env.flow_exe)
        } else {
            adapter.wiring(&env.flow_exe)
        };
        let at = 1.min(argv.len());
        argv.splice(at..at, wiring);
    }
    Some(PaneSpec {
        key: PaneKey::Flow(flow.id.clone()),
        argv,
        cwd: PathBuf::from(&space.path),
        env: worker_env(flow, space, env),
    })
}

/// True when the agent resumes by the session its harness picked, not by flow's id.
pub fn names_session(agent: &Agent) -> bool {
    agent
        .resume
        .iter()
        .flatten()
        .any(|part| part.contains("{session}"))
}

/// What a worker's session knows of its flow: the hooks' variables, so it can find its own ports
/// from FLOW_SLOT, then pane_env's.
fn worker_env(flow: &FlowView, space: &Workspace, env: &LaunchEnv) -> Vec<(String, String)> {
    let mut vars = match repo_of(space) {
        Some(repo) => hook_env(&flow.id, &flow.name, &repo, space),
        None => vec![("FLOW_ID".to_string(), flow.id.clone())],
    };
    vars.extend(pane_env(env));
    vars
}

/// FLOW_HOME when set, TERM=xterm-256color, COLORTERM=truecolor.
pub fn pane_env(env: &LaunchEnv) -> Vec<(String, String)> {
    let mut vars = Vec::new();
    if let Some(home) = &env.flow_home {
        vars.push(("FLOW_HOME".to_string(), home.display().to_string()));
    }
    vars.push(("TERM".to_string(), "xterm-256color".to_string()));
    vars.push(("COLORTERM".to_string(), "truecolor".to_string()));
    vars
}

fn fill(
    template: &str,
    id: &str,
    name: &str,
    request: &str,
    prompt: &str,
    session: &str,
) -> String {
    template
        .replace("{prompt}", prompt)
        .replace("{request}", request)
        .replace("{name}", name)
        .replace("{session}", session)
        .replace("{id}", id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    use crate::event::{Event, EventKind, Mode};
    use crate::fold;
    use crate::launch::fixtures::{shop, strings};

    #[test]
    fn fills_the_agent_template() {
        let agent = Agent {
            command: strings(&["claude", "-n", "{name}", "--session-id", "{id}", "{prompt}"]),
            prompt: Some("/ship-feature {request}".into()),
            adapter: None,
            resume: None,
        };

        assert_eq!(
            agent_argv(&agent, "7f3c", "shop-217", "SHOP-217 add CSV export"),
            strings(&[
                "claude",
                "-n",
                "shop-217",
                "--session-id",
                "7f3c",
                "/ship-feature SHOP-217 add CSV export"
            ])
        );
    }

    #[test]
    fn drops_an_argument_that_fills_to_nothing() {
        let agent = Agent {
            command: strings(&["claude", "-n", "{name}", "{prompt}"]),
            prompt: None,
            adapter: None,
            resume: None,
        };

        assert_eq!(
            agent_argv(&agent, "7f3c", "master", ""),
            strings(&["claude", "-n", "master"])
        );
    }

    #[test]
    fn fills_the_resume_command_when_the_agent_has_one() {
        let mut agent = Agent {
            command: strings(&["claude"]),
            prompt: None,
            adapter: None,
            resume: Some(strings(&["claude", "-n", "{name}", "--resume", "{id}"])),
        };

        assert_eq!(
            resume_argv(&agent, "7f3c", "shop-217", "7f3c"),
            Some(strings(&["claude", "-n", "shop-217", "--resume", "7f3c"]))
        );
        agent.resume = Some(strings(&["codex", "resume", "{session}"]));
        assert_eq!(
            resume_argv(&agent, "7f3c", "shop-217", "01a1"),
            Some(strings(&["codex", "resume", "01a1"]))
        );
        agent.resume = None;
        assert_eq!(resume_argv(&agent, "7f3c", "shop-217", "7f3c"), None);
    }

    #[test]
    fn gives_every_pane_a_terminal_type_and_workers_their_flow() {
        let env = LaunchEnv {
            flow_home: Some(PathBuf::from("/state/flow")),
            ..LaunchEnv::default()
        };
        let pairs = |items: &[(&str, &str)]| -> Vec<(String, String)> {
            items
                .iter()
                .map(|(key, value)| (key.to_string(), value.to_string()))
                .collect()
        };

        assert_eq!(
            pane_env(&env),
            pairs(&[
                ("FLOW_HOME", "/state/flow"),
                ("TERM", "xterm-256color"),
                ("COLORTERM", "truecolor"),
            ])
        );
        assert_eq!(
            pane_env(&LaunchEnv::default()),
            pairs(&[("TERM", "xterm-256color"), ("COLORTERM", "truecolor")])
        );
    }

    fn worker(window: Option<&str>) -> FlowView {
        let events = [
            Event::new(
                0,
                EventKind::Started {
                    name: "shop-217".into(),
                    request: "SHOP-217 add CSV export".into(),
                    mode: Mode::Auto,
                    phases: None,
                },
            ),
            Event::new(
                1,
                EventKind::Workspace {
                    path: "/repo/.flow/worktrees/shop-217".into(),
                    branch: Some("flow/shop-217".into()),
                    slot: Some(2),
                    window: window.map(String::from),
                    agent: "claude".into(),
                },
            ),
        ];
        fold::fold("7f3c", &events, None, 2).unwrap()
    }

    #[test]
    fn starts_a_worker_fresh_and_resumes_it_once_hosted() {
        let agents = crate::config::builtin_agents();
        let unwired = Agent {
            adapter: None,
            ..agents["claude"].clone()
        };
        let claude = &unwired;
        let env = LaunchEnv::default();

        let fresh = worker_spec(&worker(None), claude, &env).unwrap();
        assert_eq!(fresh.key, PaneKey::Flow("7f3c".into()));
        assert_eq!(fresh.cwd, PathBuf::from("/repo/.flow/worktrees/shop-217"));
        assert_eq!(
            fresh.argv,
            agent_argv(claude, "7f3c", "shop-217", "SHOP-217 add CSV export")
        );
        let mut expected = hook_env("7f3c", "shop-217", Path::new("/repo"), &shop());
        expected.extend(pane_env(&env));
        assert_eq!(
            fresh.env, expected,
            "FLOW_SLOT and the rest, as the hooks get them"
        );

        let resumed = worker_spec(&worker(Some(HOSTED)), claude, &env).unwrap();
        assert_eq!(
            resumed.argv,
            strings(&["claude", "-n", "shop-217", "--resume", "7f3c"])
        );
        let old_tmux = worker_spec(&worker(Some("@12")), claude, &env).unwrap();
        assert_eq!(
            old_tmux.argv, resumed.argv,
            "a tmux worker already took its id"
        );
    }

    #[test]
    fn wires_the_agents_adapter_in_after_the_program_fresh_and_resumed() {
        let agents = crate::config::builtin_agents();
        let mut claude = agents["claude"].clone();
        claude.adapter = Some(crate::adapter::Adapter::ClaudeCode);
        let env = LaunchEnv {
            flow_exe: PathBuf::from("/bin/flow"),
            ..LaunchEnv::default()
        };

        let fresh = worker_spec(&worker(None), &claude, &env).unwrap();
        let resumed = worker_spec(&worker(Some(HOSTED)), &claude, &env).unwrap();

        for spec in [&fresh, &resumed] {
            assert_eq!(spec.argv[0], "claude");
            assert_eq!(spec.argv[1], "--settings");
            assert!(spec.argv[2].contains("\"/bin/flow\""), "{}", spec.argv[2]);
        }
        assert_eq!(fresh.argv[3], "--append-system-prompt");
        assert_eq!(&fresh.argv[5..7], ["-n", "shop-217"]);
        assert_eq!(&resumed.argv[7..], ["--resume", "7f3c"]);
    }

    fn codex_worker(window: Option<&str>, session: Option<&str>) -> FlowView {
        let mut flow = worker(window);
        flow.workspace.as_mut().unwrap().agent = "codex".into();
        flow.session = session.map(|id| crate::view::Session {
            id: id.into(),
            transcript: None,
        });
        flow
    }

    #[test]
    fn resumes_a_codex_worker_by_the_session_it_picked_or_starts_it_again() {
        let agents = crate::config::builtin_agents();
        let codex = Agent {
            adapter: None,
            ..agents["codex"].clone()
        };
        let env = LaunchEnv::default();

        let resumed = worker_spec(&codex_worker(Some(HOSTED), Some("01a1")), &codex, &env);
        assert_eq!(resumed.unwrap().argv, strings(&["codex", "resume", "01a1"]));
        let never_ran = worker_spec(&codex_worker(Some(HOSTED), None), &codex, &env).unwrap();
        assert_eq!(
            never_ran.argv,
            strings(&["codex", "$ship-feature SHOP-217 add CSV export"]),
            "a session that never had a turn starts again"
        );
        let mut plain = codex_worker(Some(HOSTED), None);
        plain.mode = Mode::Plain;
        assert_eq!(
            worker_spec(&plain, &codex, &env).unwrap().argv,
            strings(&["codex"])
        );
    }

    #[test]
    fn wires_codex_hooks_in_before_its_resume_subcommand() {
        let agents = crate::config::builtin_agents();
        let codex = &agents["codex"];
        let env = LaunchEnv {
            flow_exe: PathBuf::from("/bin/flow"),
            ..LaunchEnv::default()
        };

        let resumed = worker_spec(&codex_worker(Some(HOSTED), Some("01a1")), codex, &env).unwrap();

        let wiring = crate::adapter::Adapter::Codex.wiring(Path::new("/bin/flow"));
        assert_eq!(resumed.argv[0], "codex");
        assert_eq!(resumed.argv[1..=wiring.len()], wiring[..]);
        assert_eq!(resumed.argv[wiring.len() + 1..], ["resume", "01a1"]);
    }

    fn scratch(window: Option<&str>) -> FlowView {
        let events = [
            Event::new(
                0,
                EventKind::Started {
                    name: "scratch".into(),
                    request: String::new(),
                    mode: Mode::Plain,
                    phases: None,
                },
            ),
            Event::new(
                1,
                EventKind::Workspace {
                    path: "/Users/jan/scratch".into(),
                    branch: None,
                    slot: None,
                    window: window.map(String::from),
                    agent: "claude".into(),
                },
            ),
        ];
        fold::fold("9b1d", &events, None, 2).unwrap()
    }

    #[test]
    fn opens_a_plain_session_in_its_folder_without_a_prompt_or_the_master() {
        let agents = crate::config::builtin_agents();
        let claude = &agents["claude"];
        let env = LaunchEnv {
            flow_exe: PathBuf::from("/bin/flow"),
            ..LaunchEnv::default()
        };
        let wiring = crate::adapter::Adapter::ClaudeCode.plain_wiring(Path::new("/bin/flow"));

        let fresh = worker_spec(&scratch(None), claude, &env).unwrap();
        let mut expected = vec![claude.command[0].clone()];
        expected.extend(wiring.clone());
        expected.extend(plain_argv(claude, "9b1d", "scratch").into_iter().skip(1));
        assert_eq!(fresh.argv, expected);
        assert!(
            !fresh.argv.iter().any(|arg| arg.contains("/ship-feature")),
            "{:?}",
            fresh.argv
        );
        assert_eq!(fresh.cwd, PathBuf::from("/Users/jan/scratch"));
        let mut env_expected = vec![("FLOW_ID".to_string(), "9b1d".to_string())];
        env_expected.extend(pane_env(&env));
        assert_eq!(
            fresh.env, env_expected,
            "no FLOW_SLOT, FLOW_WORKTREE or FLOW_REPO"
        );

        let resumed = worker_spec(&scratch(Some(HOSTED)), claude, &env).unwrap();
        let mut expected = vec![claude.command[0].clone()];
        expected.extend(wiring);
        expected.extend(strings(&["-n", "scratch", "--resume", "9b1d"]));
        assert_eq!(resumed.argv, expected);
    }

    #[test]
    fn fills_a_plain_sessions_command_without_a_prompt() {
        let agent = Agent {
            command: strings(&["claude", "-n", "{name}", "--session-id", "{id}", "{prompt}"]),
            prompt: Some("/ship-feature {request}".into()),
            adapter: None,
            resume: None,
        };

        assert_eq!(
            plain_argv(&agent, "9b1d", "scratch"),
            strings(&["claude", "-n", "scratch", "--session-id", "9b1d"])
        );
    }

    #[test]
    fn cannot_resume_a_worker_whose_agent_has_no_resume_template() {
        let agent = Agent {
            command: strings(&["aider"]),
            prompt: None,
            adapter: None,
            resume: None,
        };
        let env = LaunchEnv::default();

        assert!(worker_spec(&worker(None), &agent, &env).is_some());
        assert_eq!(worker_spec(&worker(Some(HOSTED)), &agent, &env), None);
        let mut bare = worker(None);
        bare.workspace = None;
        assert_eq!(worker_spec(&bare, &agent, &env), None);
    }
}
