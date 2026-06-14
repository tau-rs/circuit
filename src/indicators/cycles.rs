use crate::graph::{ArchGraph, ModuleId};

/// Strongly-connected components of size > 1 are dependency cycles.
/// Returns each cycle as a sorted list of module names; the outer list is sorted.
pub fn find_cycles(graph: &ArchGraph) -> Vec<Vec<String>> {
    let n = graph.modules().len();
    let mut adj: Vec<Vec<ModuleId>> = vec![Vec::new(); n];
    for (f, t) in graph.edges() {
        adj[f].push(t);
    }

    let sccs = Tarjan::run(&adj, n);
    let mut cycles: Vec<Vec<String>> = sccs
        .into_iter()
        .filter(|c| c.len() > 1)
        .map(|mut c| {
            c.sort();
            c.into_iter().map(|i| graph.name(i).to_string()).collect()
        })
        .collect();
    cycles.sort();
    cycles
}

struct Tarjan<'a> {
    adj: &'a Vec<Vec<ModuleId>>,
    index: Vec<usize>,
    low: Vec<usize>,
    on_stack: Vec<bool>,
    stack: Vec<usize>,
    idx: usize,
    sccs: Vec<Vec<usize>>,
}

impl<'a> Tarjan<'a> {
    fn run(adj: &'a Vec<Vec<ModuleId>>, n: usize) -> Vec<Vec<usize>> {
        let mut t = Tarjan {
            adj,
            index: vec![usize::MAX; n],
            low: vec![0; n],
            on_stack: vec![false; n],
            stack: Vec::new(),
            idx: 0,
            sccs: Vec::new(),
        };
        for v in 0..n {
            if t.index[v] == usize::MAX {
                t.strongconnect(v);
            }
        }
        t.sccs
    }

    fn strongconnect(&mut self, v: usize) {
        self.index[v] = self.idx;
        self.low[v] = self.idx;
        self.idx += 1;
        self.stack.push(v);
        self.on_stack[v] = true;

        for &w in &self.adj[v].clone() {
            if self.index[w] == usize::MAX {
                self.strongconnect(w);
                self.low[v] = self.low[v].min(self.low[w]);
            } else if self.on_stack[w] {
                self.low[v] = self.low[v].min(self.index[w]);
            }
        }

        if self.low[v] == self.index[v] {
            let mut comp = Vec::new();
            loop {
                let w = self.stack.pop().unwrap();
                self.on_stack[w] = false;
                comp.push(w);
                if w == v {
                    break;
                }
            }
            self.sccs.push(comp);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acyclic_graph_has_no_cycles() {
        let mut g = ArchGraph::new();
        let a = g.ensure_module("adapters");
        let d = g.ensure_module("domain");
        g.add_edge(a, d);
        assert!(find_cycles(&g).is_empty());
    }

    #[test]
    fn detects_a_two_node_cycle() {
        let mut g = ArchGraph::new();
        let a = g.ensure_module("a");
        let b = g.ensure_module("b");
        g.add_edge(a, b);
        g.add_edge(b, a);
        assert_eq!(find_cycles(&g), vec![vec!["a".to_string(), "b".to_string()]]);
    }
}
