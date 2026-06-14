use std::collections::{BTreeMap, HashSet};

use crate::graph::ArchGraph;
use crate::indicators::cycles::find_cycles;
use crate::model::node::DagNode;

/// A reason a DAG is not yet valid. Advisory and reportable, never thrown.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DagError {
    /// A dependency cycle (the same SCC the architecture cycle indicator finds).
    Cycle(Vec<String>),
    /// `node` declares a dependency on `missing`, which is not a known node id.
    DanglingRef { node: String, missing: String },
    /// More than one node maps to the same git branch.
    DuplicateBranch { branch: String, nodes: Vec<String> },
}

/// Build an `ArchGraph` from DAG nodes (module = node id, edge = dependency),
/// adding only edges whose target is a known node so dangling refs do not create
/// phantom nodes. Reuses the M1 graph model so the M1 cycle detector applies.
fn build_graph(nodes: &[DagNode], known: &HashSet<&str>) -> ArchGraph {
    let mut g = ArchGraph::new();
    for n in nodes {
        g.ensure_module(&n.id);
    }
    for n in nodes {
        let from = g.ensure_module(&n.id);
        for dep in &n.depends_on {
            if known.contains(dep.as_str()) {
                let to = g.ensure_module(dep);
                g.add_edge(from, to);
            }
        }
    }
    g
}

/// Validate a set of DAG nodes. Returns all problems found (empty = sound).
/// Errors are sorted within each kind for deterministic output.
pub fn validate(nodes: &[DagNode]) -> Vec<DagError> {
    let mut errors = Vec::new();
    let known: HashSet<&str> = nodes.iter().map(|n| n.id.as_str()).collect();

    // Dangling references.
    for n in nodes {
        for dep in &n.depends_on {
            if !known.contains(dep.as_str()) {
                errors.push(DagError::DanglingRef {
                    node: n.id.clone(),
                    missing: dep.clone(),
                });
            }
        }
    }

    // Duplicate branches.
    let mut by_branch: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for n in nodes {
        by_branch.entry(n.branch.as_str()).or_default().push(n.id.as_str());
    }
    for (branch, ns) in by_branch {
        if ns.len() > 1 {
            let mut nodes_for_branch: Vec<String> = ns.iter().map(|s| s.to_string()).collect();
            nodes_for_branch.sort();
            errors.push(DagError::DuplicateBranch {
                branch: branch.to_string(),
                nodes: nodes_for_branch,
            });
        }
    }

    // Cycles — reuse the M1 Tarjan SCC detector.
    let g = build_graph(nodes, &known);
    for cycle in find_cycles(&g) {
        errors.push(DagError::Cycle(cycle));
    }

    errors
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: &str, branch: &str, deps: &[&str]) -> DagNode {
        let mut n = DagNode::new(id, "checkout", id, branch);
        n.depends_on = deps.iter().map(|d| d.to_string()).collect();
        n
    }

    #[test]
    fn acyclic_resolved_unique_dag_is_sound() {
        let nodes = vec![
            node("cart", "impl/cart", &[]),
            node("auth", "impl/auth", &["cart"]),
        ];
        assert!(validate(&nodes).is_empty());
    }

    #[test]
    fn detects_a_dependency_cycle() {
        let nodes = vec![
            node("a", "impl/a", &["b"]),
            node("b", "impl/b", &["a"]),
        ];
        let errors = validate(&nodes);
        assert!(errors.contains(&DagError::Cycle(vec!["a".to_string(), "b".to_string()])));
    }

    #[test]
    fn detects_a_dangling_reference() {
        let nodes = vec![node("auth", "impl/auth", &["ghost"])];
        assert_eq!(
            validate(&nodes),
            vec![DagError::DanglingRef {
                node: "auth".to_string(),
                missing: "ghost".to_string(),
            }]
        );
    }

    #[test]
    fn detects_duplicate_branches() {
        let nodes = vec![
            node("a", "impl/shared", &[]),
            node("b", "impl/shared", &[]),
        ];
        assert_eq!(
            validate(&nodes),
            vec![DagError::DuplicateBranch {
                branch: "impl/shared".to_string(),
                nodes: vec!["a".to_string(), "b".to_string()],
            }]
        );
    }
}
