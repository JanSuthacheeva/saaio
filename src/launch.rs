use std::path::Path;

use crate::config::Agent;
use crate::view::{FlowView, Workspace};

pub const MAX_SLOTS: u8 = 9;
pub const WORKTREES_DIR: &str = ".flow/worktrees";

/// Ticket key lowercased (SHOP-217 -> shop-217), else the first four words as a slug;
/// -2, -3 ... when taken.
pub fn flow_name(request: &str, taken: &[String]) -> String {
    let base = ticket_key(request).unwrap_or_else(|| word_slug(request));
    if !taken.contains(&base) {
        return base;
    }
    (2..)
        .map(|number| format!("{base}-{number}"))
        .find(|candidate| !taken.contains(candidate))
        .expect("an unused suffix always exists")
}

/// The lowest slot 1..=MAX_SLOTS no unfinished flow holds; None when all are taken.
pub fn free_slot(flows: &[FlowView]) -> Option<u8> {
    let held: Vec<u8> = flows
        .iter()
        .filter(|flow| !flow.is_finished())
        .filter_map(|flow| flow.workspace.as_ref().map(|workspace| workspace.slot))
        .collect();
    (1..=MAX_SLOTS).find(|slot| !held.contains(slot))
}

/// <repo>/.flow/worktrees/<name> and branch flow/<name>.
pub fn workspace_for(repo: &Path, name: &str, slot: u8, agent: &str) -> Workspace {
    Workspace {
        path: repo.join(WORKTREES_DIR).join(name).display().to_string(),
        branch: format!("flow/{name}"),
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

/// git -C <repo> worktree remove <path> (no --force: a dirty worktree is kept)
pub fn worktree_remove_argv(repo: &Path, workspace: &Workspace) -> Vec<String> {
    let repo = repo.display().to_string();
    ["git", "-C", &repo, "worktree", "remove", &workspace.path]
        .map(String::from)
        .to_vec()
}

/// The agent's command with {id}, {name}, {prompt}, {request} filled in ({prompt} itself filled first).
pub fn agent_argv(agent: &Agent, id: &str, name: &str, request: &str) -> Vec<String> {
    let prompt = fill(&agent.prompt, id, name, request, "");
    agent
        .command
        .iter()
        .map(|part| fill(part, id, name, request, &prompt))
        .collect()
}

/// tmux new-window -d -P -F '#{window_id}' -n <name> -c <path> -e K=V ... -- <agent argv>
pub fn window_argv(
    workspace: &Workspace,
    name: &str,
    env: &[(String, String)],
    agent: &[String],
) -> Vec<String> {
    let mut argv: Vec<String> = [
        "tmux",
        "new-window",
        "-d",
        "-P",
        "-F",
        "#{window_id}",
        "-n",
        name,
        "-c",
        &workspace.path,
    ]
    .map(String::from)
    .to_vec();
    for (key, value) in env {
        argv.push("-e".to_string());
        argv.push(format!("{key}={value}"));
    }
    argv.push("--".to_string());
    argv.extend(agent.iter().cloned());
    argv
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

/// A ticket key like SHOP-217 at the start of the request, lowercased.
fn ticket_key(request: &str) -> Option<String> {
    let first = request.split_whitespace().next()?;
    let (letters, digits) = first.split_once('-')?;
    let is_key = !letters.is_empty()
        && letters.chars().all(|c| c.is_ascii_uppercase())
        && !digits.is_empty()
        && digits.chars().all(|c| c.is_ascii_digit());
    is_key.then(|| first.to_ascii_lowercase())
}

/// The first four words, lowercased, letters and digits only, joined by dashes.
fn word_slug(request: &str) -> String {
    let words: Vec<String> = request
        .split_whitespace()
        .map(|word| {
            word.chars()
                .filter(|c| c.is_ascii_alphanumeric())
                .collect::<String>()
                .to_ascii_lowercase()
        })
        .filter(|word| !word.is_empty())
        .take(4)
        .collect();
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
        items.iter().map(|item| item.to_string()).collect()
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
        workspace_for(Path::new("/repo"), "shop-217", 2, "claude")
    }

    #[test]
    fn names_a_flow_after_its_ticket_or_its_first_words() {
        assert_eq!(flow_name("SHOP-217 add CSV export", &[]), "shop-217");
        assert_eq!(
            flow_name("fix the rounding on all invoices", &[]),
            "fix-the-rounding-on"
        );
        assert_eq!(flow_name("Add: CSV export!", &[]), "add-csv-export");
        assert_eq!(
            flow_name("SHOP-217 x", &strings(&["shop-217"])),
            "shop-217-2"
        );
        assert_eq!(
            flow_name("SHOP-217 x", &strings(&["shop-217", "shop-217-2"])),
            "shop-217-3"
        );
        assert_eq!(flow_name("!!!", &[]), "flow");
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
            worktree_remove_argv(Path::new("/repo"), &workspace),
            strings(&[
                "git",
                "-C",
                "/repo",
                "worktree",
                "remove",
                "/repo/.flow/worktrees/shop-217"
            ])
        );
    }

    #[test]
    fn fills_the_agent_template() {
        let agent = Agent {
            command: strings(&["claude", "-n", "{name}", "--session-id", "{id}", "{prompt}"]),
            prompt: "/ship-feature {request}".into(),
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
    fn opens_the_worker_in_a_background_tmux_window_with_its_environment() {
        let env = vec![("FLOW_ID".to_string(), "7f3c".to_string())];

        assert_eq!(
            window_argv(&shop(), "shop-217", &env, &strings(&["claude", "go"])),
            strings(&[
                "tmux",
                "new-window",
                "-d",
                "-P",
                "-F",
                "#{window_id}",
                "-n",
                "shop-217",
                "-c",
                "/repo/.flow/worktrees/shop-217",
                "-e",
                "FLOW_ID=7f3c",
                "--",
                "claude",
                "go",
            ])
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
}
