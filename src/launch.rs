use std::path::{Path, PathBuf};

use crate::config::Agent;
use crate::proto::PaneKey;
use crate::slots::MAX_SLOTS;
use crate::view::{FlowView, Workspace};

pub const WORKTREES_DIR: &str = ".flow/worktrees";
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

/// The flow's name: an explicit name slugified whole, else the request's ticket key lowercased
/// (SHOP-217 -> shop-217), else its first four words; -2, -3 ... when taken.
pub fn flow_name(name: Option<&str>, request: &str, taken: &[String]) -> String {
    let base = match name {
        Some(name) => slugify(name, usize::MAX),
        None => ticket_key(request).unwrap_or_else(|| slugify(request, 4)),
    };
    if !taken.contains(&base) {
        return base;
    }
    (2..)
        .map(|number| format!("{base}-{number}"))
        .find(|candidate| !taken.contains(candidate))
        .expect("an unused suffix always exists")
}

/// The slots unfinished flows hold.
pub fn taken_slots(flows: &[FlowView]) -> Vec<u8> {
    flows
        .iter()
        .filter(|flow| !flow.is_finished())
        .filter_map(|flow| flow.workspace.as_ref().map(|workspace| workspace.slot))
        .collect()
}

/// The lowest slot 1..=MAX_SLOTS no unfinished flow of this project holds; flow start decides across
/// projects.
pub fn free_slot(flows: &[FlowView]) -> Option<u8> {
    let held = taken_slots(flows);
    (1..=MAX_SLOTS).find(|slot| !held.contains(slot))
}

/// <repo>/.flow/worktrees/<name> on `branch`.
pub fn workspace_for(repo: &Path, name: &str, branch: String, slot: u8, agent: &str) -> Workspace {
    Workspace {
        path: repo.join(WORKTREES_DIR).join(name).display().to_string(),
        branch,
        slot,
        window: None,
        agent: agent.to_string(),
    }
}

/// git -C <repo> worktree add -b <branch> <path>
pub fn worktree_add_argv(repo: &Path, workspace: &Workspace) -> Vec<String> {
    let repo = repo.display().to_string();
    [
        "git",
        "-C",
        &repo,
        "worktree",
        "add",
        "-b",
        &workspace.branch,
        &workspace.path,
    ]
    .map(String::from)
    .to_vec()
}

/// git -C <repo> worktree remove [--force] <path>; without force a dirty worktree is kept.
pub fn worktree_remove_argv(repo: &Path, workspace: &Workspace, force: bool) -> Vec<String> {
    let repo = repo.display().to_string();
    let mut argv: Vec<String> = ["git", "-C", &repo, "worktree", "remove"]
        .map(String::from)
        .to_vec();
    if force {
        argv.push("--force".to_string());
    }
    argv.push(workspace.path.clone());
    argv
}

/// The agent's command with {id}, {name}, {prompt}, {request} filled in ({prompt} itself filled first);
/// an argument that fills to nothing is dropped, so an agent without a prompt gets no empty one.
pub fn agent_argv(agent: &Agent, id: &str, name: &str, request: &str) -> Vec<String> {
    let prompt = fill(&agent.prompt, id, name, request, "");
    agent
        .command
        .iter()
        .map(|part| fill(part, id, name, request, &prompt))
        .filter(|part| !part.is_empty())
        .collect()
}

/// The agent's resume command with {id} and {name} filled in; None when it has none.
pub fn resume_argv(agent: &Agent, id: &str, name: &str) -> Option<Vec<String>> {
    let resume = agent.resume.as_ref()?;
    Some(
        resume
            .iter()
            .map(|part| fill(part, id, name, "", ""))
            .filter(|part| !part.is_empty())
            .collect(),
    )
}

/// The agent's command the first time, its resume command once it ran somewhere (window set:
/// HOSTED, or a tmux window from before flow ui), since its session id is taken from then on;
/// either way with the agent's adapter wired in right after the program. None when the flow has
/// no workspace, or must resume and the agent has no resume template.
pub fn worker_spec(flow: &FlowView, agent: &Agent, env: &LaunchEnv) -> Option<PaneSpec> {
    let space = flow.workspace.as_ref()?;
    let mut argv = if space.window.is_some() {
        resume_argv(agent, &flow.id, &flow.name)?
    } else {
        agent_argv(agent, &flow.id, &flow.name, &flow.request)
    };
    if let Some(adapter) = agent.adapter {
        let at = 1.min(argv.len());
        argv.splice(at..at, adapter.wiring(&env.flow_exe));
    }
    Some(PaneSpec {
        key: PaneKey::Flow(flow.id.clone()),
        argv,
        cwd: PathBuf::from(&space.path),
        env: worker_env(flow, space, env),
    })
}

/// What a worker's session knows of its flow: the hooks' variables, so it can find its own ports
/// from FLOW_SLOT, then pane_env's.
fn worker_env(flow: &FlowView, space: &Workspace, env: &LaunchEnv) -> Vec<(String, String)> {
    let mut vars = match repo_of(space) {
        Some(repo) => hook_env(&flow.id, &flow.name, &repo, space),
        None => vec![("FLOW_ID".to_string(), flow.id.clone())],
    };
    vars.extend(pane_env(None, env));
    vars
}

/// The repository a flow worktree belongs to: the path above .flow/worktrees/<name>.
pub fn repo_of(space: &Workspace) -> Option<PathBuf> {
    Path::new(&space.path)
        .parent()
        .filter(|worktrees| worktrees.ends_with(WORKTREES_DIR))?
        .parent()?
        .parent()
        .map(Path::to_path_buf)
}

/// FLOW_ID (workers only), FLOW_HOME when set, TERM=xterm-256color, COLORTERM=truecolor.
pub fn pane_env(id: Option<&str>, env: &LaunchEnv) -> Vec<(String, String)> {
    let mut vars = Vec::new();
    if let Some(id) = id {
        vars.push(("FLOW_ID".to_string(), id.to_string()));
    }
    if let Some(home) = &env.flow_home {
        vars.push(("FLOW_HOME".to_string(), home.display().to_string()));
    }
    vars.push(("TERM".to_string(), "xterm-256color".to_string()));
    vars.push(("COLORTERM".to_string(), "truecolor".to_string()));
    vars
}

/// FLOW_ID, FLOW_NAME, FLOW_SLOT, FLOW_WORKTREE, FLOW_REPO for the hooks.
pub fn hook_env(id: &str, name: &str, repo: &Path, workspace: &Workspace) -> Vec<(String, String)> {
    [
        ("FLOW_ID", id.to_string()),
        ("FLOW_NAME", name.to_string()),
        ("FLOW_SLOT", workspace.slot.to_string()),
        ("FLOW_WORKTREE", workspace.path.clone()),
        ("FLOW_REPO", repo.display().to_string()),
    ]
    .into_iter()
    .map(|(key, value)| (key.to_string(), value))
    .collect()
}

fn fill(template: &str, id: &str, name: &str, request: &str, prompt: &str) -> String {
    template
        .replace("{prompt}", prompt)
        .replace("{request}", request)
        .replace("{name}", name)
        .replace("{id}", id)
}

/// A ticket key like SHOP-217 at the start of the request, as written.
pub fn ticket(request: &str) -> Option<&str> {
    let first = request.split_whitespace().next()?;
    let (letters, digits) = first.split_once('-')?;
    let is_key = !letters.is_empty()
        && letters.chars().all(|c| c.is_ascii_uppercase())
        && !digits.is_empty()
        && digits.chars().all(|c| c.is_ascii_digit());
    is_key.then_some(first)
}

/// Up to five lowercase words of the request after its ticket key, joined by dashes; empty when none.
pub fn slug(request: &str) -> String {
    let rest = ticket(request)
        .and_then(|key| request.trim_start().strip_prefix(key))
        .unwrap_or(request);
    words(rest, 5).join("-")
}

/// The request's ticket key lowercased, for a flow's name.
fn ticket_key(request: &str) -> Option<String> {
    ticket(request).map(str::to_ascii_lowercase)
}

/// Lowercase words of letters and digits, at most `max_words`.
fn words(text: &str, max_words: usize) -> Vec<String> {
    text.split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
        .take(max_words)
        .map(str::to_ascii_lowercase)
        .collect()
}

/// Lowercase words of letters and digits, at most `max_words`, joined by single dashes;
/// any other character separates words. "flow" when nothing is left.
fn slugify(text: &str, max_words: usize) -> String {
    let words = words(text, max_words);
    if words.is_empty() {
        "flow".to_string()
    } else {
        words.join("-")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{Event, EventKind, Mode};
    use crate::fold;

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().copied().map(String::from).collect()
    }

    fn flow_in_slot(slot: u8, finished: bool) -> FlowView {
        let mut events = vec![Event::new(
            0,
            EventKind::Started {
                name: format!("f{slot}"),
                request: "x".into(),
                mode: Mode::Guided,
            },
        )];
        events.push(Event::new(
            1,
            EventKind::Workspace {
                path: "/p".into(),
                branch: "b".into(),
                slot,
                window: None,
                agent: "claude".into(),
            },
        ));
        if finished {
            events.push(Event::new(2, EventKind::Ended));
        }
        fold::fold("id", &events, None, 3).unwrap()
    }

    fn shop() -> Workspace {
        workspace_for(
            Path::new("/repo"),
            "shop-217",
            "flow/shop-217".into(),
            2,
            "claude",
        )
    }

    #[test]
    fn slugifies_an_explicit_name_and_the_words_of_a_request() {
        assert_eq!(
            flow_name(Some("skills-reporting"), "x", &[]),
            "skills-reporting"
        );
        assert_eq!(
            flow_name(Some("Skills Reporting"), "x", &[]),
            "skills-reporting"
        );
        assert_eq!(flow_name(Some("  a / b__c.d  "), "x", &[]), "a-b-c-d");
        assert_eq!(
            flow_name(Some("one two three four five six"), "x", &[]),
            "one-two-three-four-five-six"
        );
        assert_eq!(
            flow_name(None, "look at docs/flow-handover.md and build it", &[]),
            "look-at-docs-flow"
        );
        assert_eq!(flow_name(Some("SHOP-217"), "x", &[]), "shop-217");
        assert_eq!(flow_name(Some("!!!"), "x", &[]), "flow");
    }

    #[test]
    fn names_a_flow_after_its_ticket_or_its_first_words() {
        assert_eq!(flow_name(None, "SHOP-217 add CSV export", &[]), "shop-217");
        assert_eq!(
            flow_name(None, "fix the rounding on all invoices", &[]),
            "fix-the-rounding-on"
        );
        assert_eq!(flow_name(None, "Add: CSV export!", &[]), "add-csv-export");
        assert_eq!(
            flow_name(None, "SHOP-217 x", &strings(&["shop-217"])),
            "shop-217-2"
        );
        assert_eq!(
            flow_name(None, "SHOP-217 x", &strings(&["shop-217", "shop-217-2"])),
            "shop-217-3"
        );
        assert_eq!(flow_name(None, "!!!", &[]), "flow");
    }

    #[test]
    fn reads_the_ticket_as_written_and_the_words_after_it() {
        assert_eq!(ticket("SHOP-230 fix totals"), Some("SHOP-230"));
        assert_eq!(ticket("fix SHOP-230"), None);
        assert_eq!(ticket("shop-230 fix"), None);
        assert_eq!(
            slug("SHOP-230 Fix totals, on refunds!"),
            "fix-totals-on-refunds"
        );
        assert_eq!(
            slug("rate-limit the export endpoint now please"),
            "rate-limit-the-export-endpoint"
        );
        assert_eq!(slug("SHOP-230"), "");
    }

    #[test]
    fn gives_the_lowest_slot_no_running_flow_holds() {
        assert_eq!(free_slot(&[]), Some(1));
        assert_eq!(
            free_slot(&[flow_in_slot(1, false), flow_in_slot(3, false)]),
            Some(2)
        );
        assert_eq!(free_slot(&[flow_in_slot(1, true)]), Some(1));
        let full: Vec<FlowView> = (1..=MAX_SLOTS)
            .map(|slot| flow_in_slot(slot, false))
            .collect();
        assert_eq!(free_slot(&full), None);
    }

    #[test]
    fn lists_the_slots_of_unfinished_flows() {
        let flows = [flow_in_slot(4, false), flow_in_slot(2, true)];
        assert_eq!(taken_slots(&flows), vec![4]);
    }

    #[test]
    fn places_the_worktree_inside_the_repo_on_its_own_branch() {
        let workspace = shop();

        assert_eq!(workspace.path, "/repo/.flow/worktrees/shop-217");
        assert_eq!(workspace.branch, "flow/shop-217");
        assert_eq!(workspace.slot, 2);
        assert_eq!(workspace.window, None);
        assert_eq!(workspace.agent, "claude");
        assert_eq!(
            worktree_add_argv(Path::new("/repo"), &workspace),
            strings(&[
                "git",
                "-C",
                "/repo",
                "worktree",
                "add",
                "-b",
                "flow/shop-217",
                "/repo/.flow/worktrees/shop-217"
            ])
        );
        assert_eq!(
            worktree_remove_argv(Path::new("/repo"), &workspace, false),
            strings(&[
                "git",
                "-C",
                "/repo",
                "worktree",
                "remove",
                "/repo/.flow/worktrees/shop-217"
            ])
        );
        assert_eq!(
            worktree_remove_argv(Path::new("/repo"), &workspace, true),
            strings(&[
                "git",
                "-C",
                "/repo",
                "worktree",
                "remove",
                "--force",
                "/repo/.flow/worktrees/shop-217"
            ])
        );
    }

    #[test]
    fn fills_the_agent_template() {
        let agent = Agent {
            command: strings(&["claude", "-n", "{name}", "--session-id", "{id}", "{prompt}"]),
            prompt: "/ship-feature {request}".into(),
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
            prompt: String::new(),
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
            prompt: String::new(),
            adapter: None,
            resume: Some(strings(&["claude", "-n", "{name}", "--resume", "{id}"])),
        };

        assert_eq!(
            resume_argv(&agent, "7f3c", "shop-217"),
            Some(strings(&["claude", "-n", "shop-217", "--resume", "7f3c"]))
        );
        agent.resume = None;
        assert_eq!(resume_argv(&agent, "7f3c", "shop-217"), None);
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
            pane_env(Some("7f3c"), &env),
            pairs(&[
                ("FLOW_ID", "7f3c"),
                ("FLOW_HOME", "/state/flow"),
                ("TERM", "xterm-256color"),
                ("COLORTERM", "truecolor"),
            ])
        );
        assert_eq!(
            pane_env(None, &LaunchEnv::default()),
            pairs(&[("TERM", "xterm-256color"), ("COLORTERM", "truecolor")])
        );
    }

    #[test]
    fn hands_the_hooks_what_they_need() {
        assert_eq!(
            hook_env("7f3c", "shop-217", Path::new("/repo"), &shop()),
            vec![
                ("FLOW_ID".to_string(), "7f3c".to_string()),
                ("FLOW_NAME".to_string(), "shop-217".to_string()),
                ("FLOW_SLOT".to_string(), "2".to_string()),
                (
                    "FLOW_WORKTREE".to_string(),
                    "/repo/.flow/worktrees/shop-217".to_string()
                ),
                ("FLOW_REPO".to_string(), "/repo".to_string()),
            ]
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
                },
            ),
            Event::new(
                1,
                EventKind::Workspace {
                    path: "/repo/.flow/worktrees/shop-217".into(),
                    branch: "flow/shop-217".into(),
                    slot: 2,
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
        expected.extend(pane_env(None, &env));
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
    fn finds_the_repo_above_the_worktrees_folder_only() {
        assert_eq!(repo_of(&shop()), Some(PathBuf::from("/repo")));
        let elsewhere = Workspace {
            path: "/somewhere/shop-217".into(),
            ..shop()
        };
        assert_eq!(repo_of(&elsewhere), None);
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

    #[test]
    fn cannot_resume_a_worker_whose_agent_has_no_resume_template() {
        let agent = Agent {
            command: strings(&["aider"]),
            prompt: String::new(),
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
