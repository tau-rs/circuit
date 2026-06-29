use std::collections::HashSet;

use crate::comprehension::layered::{EdgeDir, FeatureOverlay, LayeredGraph};
use crate::graph::{ArchGraph, ModuleId};
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
    let viol: HashSet<(&str, &str)> = violations
        .iter()
        .map(|v| (v.from.as_str(), v.to.as_str()))
        .collect();

    let mut out = String::from("graph TD\n");

    let mut mods: Vec<&crate::graph::Module> = graph.modules().iter().collect();
    mods.sort_by(|a, b| a.name.cmp(&b.name));
    for m in &mods {
        let mark = if cyclic.contains(m.name.as_str()) {
            " ⟲"
        } else {
            ""
        };
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
            out.push_str(&format!(
                "  {} -->|VIOLATION| {}\n",
                node_id(&f),
                node_id(&t)
            ));
        } else {
            out.push_str(&format!("  {} --> {}\n", node_id(&f), node_id(&t)));
        }
    }

    out
}

/// Render the layered graph as a mermaid `flowchart LR`: one subgraph per
/// non-empty layer column, outward edges flagged `|VIOLATION|`, and (when an
/// overlay is given) its modules bolded via a `feat` class. Export only.
pub fn render_layered(
    g: &ArchGraph,
    lg: &LayeredGraph,
    overlay: Option<&FeatureOverlay>,
) -> String {
    let members: HashSet<ModuleId> = overlay
        .map(|o| o.modules.iter().copied().collect())
        .unwrap_or_default();

    let mut out = String::from("flowchart LR\n");
    for col in &lg.columns {
        if col.modules.is_empty() {
            continue;
        }
        out.push_str(&format!("  subgraph {:?}\n", col.layer));
        for &id in &col.modules {
            out.push_str(&format!("    {}[\"{}\"]\n", node_id(g.name(id)), g.name(id)));
        }
        out.push_str("  end\n");
    }
    for e in &lg.edges {
        let arrow = if matches!(e.dir, EdgeDir::Outward) {
            "-->|VIOLATION|"
        } else {
            "-->"
        };
        out.push_str(&format!(
            "  {} {} {}\n",
            node_id(g.name(e.from)),
            arrow,
            node_id(g.name(e.to))
        ));
    }
    if !members.is_empty() {
        let mut ids: Vec<String> = members.iter().map(|&id| node_id(g.name(id))).collect();
        ids.sort();
        out.push_str("  classDef feat stroke-width:3px,font-weight:bold;\n");
        out.push_str(&format!("  class {} feat;\n", ids.join(",")));
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

    #[test]
    fn layered_render_has_subgraph_per_nonempty_layer() {
        use crate::comprehension::layered::layered;
        let mut g = ArchGraph::new();
        let a = g.ensure_module("adapters");
        let d = g.ensure_module("domain");
        g.add_edge(a, d);
        let lg = layered(&g);

        let out = render_layered(&g, &lg, None);
        assert!(out.starts_with("flowchart LR\n"));
        assert!(out.contains("subgraph Adapter"));
        assert!(out.contains("subgraph Domain"));
        // Empty Application column is omitted.
        assert!(!out.contains("subgraph Application"));
        assert!(out.contains("adapters --> domain"));
    }

    #[test]
    fn layered_render_bolds_overlay_members() {
        use crate::comprehension::callgraph::CallGraph;
        use crate::comprehension::layered::{layered, overlay};
        use crate::lang::FnDecl;

        let mut g = ArchGraph::new();
        g.ensure_module("app");
        let lg = layered(&g);
        let decls = vec![(
            "app".to_string(),
            FnDecl {
                name: "run".into(),
                is_pub: true,
                is_test: false,
                is_main: false,
                calls: vec![],
            },
        )];
        let calls = CallGraph::build(&decls);
        let ov = overlay(&g, &calls, "run", &lg);

        let out = render_layered(&g, &lg, Some(&ov));
        assert!(out.contains("classDef feat"));
        assert!(out.contains("class app feat;"));
    }
}
