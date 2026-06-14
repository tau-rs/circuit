use std::collections::HashSet;

use crate::graph::ArchGraph;
use crate::indicators::dependency_rule::Violation;

fn node_id(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect()
}

/// Render the graph as a mermaid `graph TD`. Cyclic modules are marked `⟲`;
/// violating edges are labelled `|VIOLATION|`. Output is deterministic (sorted).
pub fn render(graph: &ArchGraph, violations: &[Violation], cycles: &[Vec<String>]) -> String {
    let cyclic: HashSet<&str> = cycles.iter().flatten().map(|s| s.as_str()).collect();
    let viol: HashSet<(&str, &str)> =
        violations.iter().map(|v| (v.from.as_str(), v.to.as_str())).collect();

    let mut out = String::from("graph TD\n");

    let mut mods: Vec<&crate::graph::Module> = graph.modules().iter().collect();
    mods.sort_by(|a, b| a.name.cmp(&b.name));
    for m in &mods {
        let mark = if cyclic.contains(m.name.as_str()) { " ⟲" } else { "" };
        out.push_str(&format!(
            "  {}[\"{}<br/>({:?}){}\"]\n",
            node_id(&m.name),
            m.name,
            m.layer,
            mark
        ));
    }

    let mut edges: Vec<(String, String)> = graph
        .edges()
        .into_iter()
        .map(|(f, t)| (graph.name(f).to_string(), graph.name(t).to_string()))
        .collect();
    edges.sort();
    for (f, t) in edges {
        if viol.contains(&(f.as_str(), t.as_str())) {
            out.push_str(&format!("  {} -->|VIOLATION| {}\n", node_id(&f), node_id(&t)));
        } else {
            out.push_str(&format!("  {} --> {}\n", node_id(&f), node_id(&t)));
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_nodes_and_edges_deterministically() {
        let mut g = ArchGraph::new();
        let a = g.ensure_module("adapters");
        let d = g.ensure_module("domain");
        g.add_edge(a, d);

        let out = render(&g, &[], &[]);
        assert!(out.starts_with("graph TD\n"));
        assert!(out.contains("adapters[\"adapters<br/>(Adapter)\"]"));
        assert!(out.contains("domain[\"domain<br/>(Domain)\"]"));
        assert!(out.contains("adapters --> domain"));
    }

    #[test]
    fn marks_violations_and_cycles() {
        let mut g = ArchGraph::new();
        let d = g.ensure_module("domain");
        let a = g.ensure_module("adapters");
        g.add_edge(d, a);

        let violations = vec![Violation {
            from: "domain".into(),
            to: "adapters".into(),
            from_layer: crate::layer::Layer::Domain,
            to_layer: crate::layer::Layer::Adapter,
        }];
        let cycles = vec![vec!["adapters".to_string(), "domain".to_string()]];

        let out = render(&g, &violations, &cycles);
        assert!(out.contains("domain -->|VIOLATION| adapters"));
        assert!(out.contains("(Domain) ⟲"));
    }
}
