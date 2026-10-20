use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

type Graph = BTreeMap<String, BTreeSet<String>>;

/// Each top-level module's production `crate::` uses: test modules, tests.rs and fixtures left out.
fn uses(src: &Path) -> Graph {
    let mut graph = Graph::new();
    let mut dirs = vec![src.to_path_buf()];
    while let Some(dir) = dirs.pop() {
        for entry in fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            let name = path.file_name().unwrap().to_str().unwrap();
            if path.is_dir() {
                if name != "tests" {
                    dirs.push(path);
                }
                continue;
            }
            let skipped = [
                "lib.rs",
                "main.rs",
                "tests.rs",
                "fixtures.rs",
                "layering.rs",
            ];
            if path.extension().is_none_or(|extension| extension != "rs") || skipped.contains(&name)
            {
                continue;
            }
            let relative = path.strip_prefix(src).unwrap().to_str().unwrap();
            let module = relative.split('/').next().unwrap().trim_end_matches(".rs");
            let text = fs::read_to_string(&path).unwrap();
            let production = text.split("#[cfg(test)]\nmod tests {").next().unwrap();
            let used = graph.entry(module.to_string()).or_default();
            for line in production.lines() {
                if line.trim_start().starts_with("//") {
                    continue;
                }
                for (at, path) in line.match_indices("crate::") {
                    let target: String = line[at + path.len()..]
                        .chars()
                        .take_while(|c| c.is_alphanumeric() || *c == '_')
                        .collect();
                    if !target.is_empty() && target != module {
                        used.insert(target);
                    }
                }
            }
        }
    }
    graph
}

/// The first cycle in the graph, as the modules on it.
fn cycle(graph: &Graph) -> Option<Vec<String>> {
    fn visit(
        module: &str,
        graph: &Graph,
        path: &mut Vec<String>,
        done: &mut BTreeSet<String>,
    ) -> Option<Vec<String>> {
        if let Some(at) = path.iter().position(|seen| seen == module) {
            return Some(path[at..].to_vec());
        }
        if done.contains(module) {
            return None;
        }
        path.push(module.to_string());
        for used in graph.get(module).into_iter().flatten() {
            if let Some(found) = visit(used, graph, path, done) {
                return Some(found);
            }
        }
        path.pop();
        done.insert(module.to_string());
        None
    }
    let mut done = BTreeSet::new();
    graph
        .keys()
        .find_map(|module| visit(module, graph, &mut Vec::new(), &mut done))
}

#[test]
fn finds_a_cycle_and_names_its_modules() {
    let graph = |edges: &[(&str, &str)]| {
        let mut graph = Graph::new();
        for (from, to) in edges {
            graph
                .entry((*from).to_string())
                .or_default()
                .insert((*to).to_string());
        }
        graph
    };

    assert_eq!(cycle(&graph(&[("cli", "ui"), ("ui", "config")])), None);
    assert_eq!(
        cycle(&graph(&[("cli", "ui"), ("ui", "config"), ("config", "ui")])),
        Some(vec!["ui".to_string(), "config".to_string()])
    );
}

#[test]
fn the_top_level_modules_form_no_cycle() {
    let graph = uses(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"));

    assert!(graph["cli"].contains("ui"), "{graph:?}");
    assert_eq!(cycle(&graph), None);
}
