//! Which flows stand on a flow's branch, and the event each gets once that branch merged (pure)

use crate::event::EventKind;
use crate::launch::worktree::repo_of;
use crate::view::FlowView;

/// The unfinished, unmerged worktree flows of `base`'s repository whose base is `base`'s branch;
/// never `base` itself.
pub fn standing_on<'a>(flows: &'a [FlowView], base: &FlowView) -> Vec<&'a FlowView> {
    let Some(space) = &base.workspace else {
        return Vec::new();
    };
    let (Some(branch), Some(repo)) = (&space.branch, repo_of(space)) else {
        return Vec::new();
    };
    flows
        .iter()
        .filter(|flow| flow.id != base.id && !flow.is_finished() && !flow.merged)
        .filter(|flow| {
            flow.workspace.as_ref().is_some_and(|other| {
                other.base.as_ref() == Some(branch) && repo_of(other).as_ref() == Some(&repo)
            })
        })
        .collect()
}

/// For each flow standing_on `merged`: its id and BaseMerged { base: merged's branch, onto }; none
/// when the forge named no branch it merged into.
pub fn on_merge(flows: &[FlowView], merged: &FlowView, onto: &str) -> Vec<(String, EventKind)> {
    let Some(base) = merged
        .workspace
        .as_ref()
        .and_then(|space| space.branch.clone())
    else {
        return Vec::new();
    };
    if onto.is_empty() {
        return Vec::new();
    }
    standing_on(flows, merged)
        .into_iter()
        .map(|flow| {
            let kind = EventKind::BaseMerged {
                base: base.clone(),
                onto: onto.to_string(),
            };
            (flow.id.clone(), kind)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::Event;
    use crate::fixtures::{flow, workspace};
    use crate::view::Workspace;

    fn at(space: Workspace, more: Vec<EventKind>) -> Vec<Event> {
        std::iter::once(space.into())
            .chain(more)
            .map(|kind| Event::new(1, kind))
            .collect()
    }

    fn on(name: &str, base: Option<&str>) -> Workspace {
        Workspace {
            base: base.map(String::from),
            ..workspace(name, "claude")
        }
    }

    #[test]
    fn finds_the_unfinished_flows_of_the_same_repo_on_a_branch() {
        let elsewhere = Workspace {
            path: "/other/.saaio/worktrees/c".into(),
            ..on("c", Some("flow/a"))
        };
        let flows = vec![
            flow("a", at(on("a", None), vec![]), 2),
            flow("b", at(on("b", Some("flow/a")), vec![]), 2),
            flow("c", at(elsewhere, vec![]), 2),
            flow("d", at(on("d", Some("flow/a")), vec![EventKind::Ended]), 2),
            flow(
                "e",
                at(
                    on("e", Some("flow/a")),
                    vec![EventKind::Merged { url: "u".into() }],
                ),
                2,
            ),
            flow("f", at(on("f", Some("main")), vec![]), 2),
            flow("g", vec![], 2),
        ];

        let names: Vec<&str> = standing_on(&flows, &flows[0])
            .iter()
            .map(|flow| flow.name.as_str())
            .collect();

        assert_eq!(names, ["b"]);
        assert!(standing_on(&flows, &flows[6]).is_empty(), "no workspace");
    }

    #[test]
    fn tells_each_flow_on_a_merged_branch_where_it_went() {
        let flows = vec![
            flow("a", at(on("a", Some("flow/base")), vec![]), 2),
            flow("b", at(on("b", Some("flow/a")), vec![]), 2),
            flow("c", at(on("c", Some("flow/a")), vec![]), 2),
        ];

        assert_eq!(
            on_merge(&flows, &flows[0], "flow/base"),
            [("b", "flow/base"), ("c", "flow/base")].map(|(id, onto)| (
                id.to_string(),
                EventKind::BaseMerged {
                    base: "flow/a".into(),
                    onto: onto.into()
                }
            ))
        );
        assert!(on_merge(&flows, &flows[0], "").is_empty());
        assert!(on_merge(&flows, &flows[1], "main").is_empty());
    }
}
