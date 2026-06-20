use std::collections::{BTreeSet, HashMap, VecDeque};

use crate::lang::FnDecl;

pub type FnId = usize;

#[derive(Clone, Debug)]
pub struct FnNode {
    pub module: String,
    pub name: String,
    pub is_pub: bool,
    pub is_test: bool,
    pub is_main: bool,
}

impl FnNode {
    pub fn qualified(&self) -> String {
        format!("{}::{}", self.module, self.name)
    }
}

#[derive(Default)]
pub struct CallGraph {
    nodes: Vec<FnNode>,
    by_name: HashMap<String, Vec<FnId>>,
    edges: BTreeSet<(FnId, FnId)>,
}

impl CallGraph {
    /// Build from (module, FnDecl) pairs. Calls resolve by name only: receiver
    /// type is ignored, so an ambiguous callee links to every match (approximate
    /// but deterministic — the skeleton later refined by clustering/Tau).
    pub fn build(decls: &[(String, FnDecl)]) -> Self {
        let mut g = CallGraph::default();
        for (module, d) in decls {
            let id = g.nodes.len();
            g.nodes.push(FnNode {
                module: module.clone(),
                name: d.name.clone(),
                is_pub: d.is_pub,
                is_test: d.is_test,
                is_main: d.is_main,
            });
            g.by_name.entry(d.name.clone()).or_default().push(id);
        }
        for (from, (_, d)) in decls.iter().enumerate() {
            for callee in &d.calls {
                if let Some(targets) = g.by_name.get(callee) {
                    for &to in targets {
                        if to != from {
                            g.edges.insert((from, to));
                        }
                    }
                }
            }
        }
        g
    }

    pub fn nodes(&self) -> &[FnNode] {
        &self.nodes
    }

    pub fn node(&self, id: FnId) -> &FnNode {
        &self.nodes[id]
    }

    pub fn edges(&self) -> Vec<(FnId, FnId)> {
        self.edges.iter().copied().collect()
    }

    /// All functions reachable from `start` (inclusive), in ascending id order.
    pub fn reachable(&self, start: FnId) -> Vec<FnId> {
        let mut adj: HashMap<FnId, Vec<FnId>> = HashMap::new();
        for &(f, t) in &self.edges {
            adj.entry(f).or_default().push(t);
        }
        let mut seen = BTreeSet::new();
        let mut q = VecDeque::new();
        seen.insert(start);
        q.push_back(start);
        while let Some(n) = q.pop_front() {
            if let Some(next) = adj.get(&n) {
                for &t in next {
                    if seen.insert(t) {
                        q.push_back(t);
                    }
                }
            }
        }
        seen.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::FnDecl;

    fn decl(name: &str, is_pub: bool, is_main: bool, calls: &[&str]) -> FnDecl {
        FnDecl {
            name: name.into(),
            is_pub,
            is_test: false,
            is_main,
            calls: calls.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn builds_edges_and_reachability_by_name() {
        let decls = vec![
            ("root".to_string(), decl("main", false, true, &["greet"])),
            ("domain".to_string(), decl("greet", true, false, &[])),
        ];
        let g = CallGraph::build(&decls);

        assert_eq!(g.nodes().len(), 2);
        assert_eq!(g.node(0).qualified(), "root::main");
        assert_eq!(g.edges(), vec![(0, 1)]);
        assert_eq!(g.reachable(0), vec![0, 1]);
        assert_eq!(g.reachable(1), vec![1]);
    }
}
