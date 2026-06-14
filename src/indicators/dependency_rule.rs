use crate::graph::ArchGraph;
use crate::layer::{rank, Layer};

/// An inner layer depending on an outer one (violates the Dependency Rule).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Violation {
    pub from: String,
    pub to: String,
    pub from_layer: Layer,
    pub to_layer: Layer,
}

/// Report edges where a more-inner module depends on a more-outer one.
/// Edges touching an `Unknown` (unranked) layer are skipped — we never fake a verdict.
pub fn violations(graph: &ArchGraph) -> Vec<Violation> {
    let mut out = Vec::new();
    for (f, t) in graph.edges() {
        let from_layer = graph.modules()[f].layer;
        let to_layer = graph.modules()[t].layer;
        if let (Some(rf), Some(rt)) = (rank(from_layer), rank(to_layer)) {
            if rf < rt {
                out.push(Violation {
                    from: graph.name(f).to_string(),
                    to: graph.name(t).to_string(),
                    from_layer,
                    to_layer,
                });
            }
        }
    }
    out.sort_by(|a, b| (&a.from, &a.to).cmp(&(&b.from, &b.to)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outer_depending_on_inner_is_allowed() {
        let mut g = ArchGraph::new();
        let a = g.ensure_module("adapters");
        let d = g.ensure_module("domain");
        g.add_edge(a, d);
        assert!(violations(&g).is_empty());
    }

    #[test]
    fn inner_depending_on_outer_is_a_violation() {
        let mut g = ArchGraph::new();
        let d = g.ensure_module("domain");
        let a = g.ensure_module("adapters");
        g.add_edge(d, a);
        let v = violations(&g);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].from, "domain");
        assert_eq!(v[0].to, "adapters");
        assert_eq!(v[0].from_layer, Layer::Domain);
        assert_eq!(v[0].to_layer, Layer::Adapter);
    }

    #[test]
    fn unknown_layers_are_skipped() {
        let mut g = ArchGraph::new();
        let graph_mod = g.ensure_module("graph");
        let a = g.ensure_module("adapters");
        g.add_edge(graph_mod, a);
        assert!(violations(&g).is_empty());
    }
}
