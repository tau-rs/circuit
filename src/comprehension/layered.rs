use std::fmt::Write;

use crate::graph::{ArchGraph, ModuleId};
use crate::layer::{rank, Layer};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EdgeDir {
    Inward,
    Outward,
    Lateral,
    Unranked,
}

#[derive(Clone, Debug)]
pub struct LgEdge {
    pub from: ModuleId,
    pub to: ModuleId,
    pub dir: EdgeDir,
}

#[derive(Clone, Debug)]
pub struct LayerColumn {
    pub layer: Layer,
    pub modules: Vec<ModuleId>,
}

#[derive(Clone, Debug, Default)]
pub struct LayeredGraph {
    pub columns: Vec<LayerColumn>,
    pub edges: Vec<LgEdge>,
}

/// Fixed outside-in column order: outermost adapters first, domain core last,
/// unranked modules trailing. Arrows point inward toward the core.
const COLUMN_ORDER: [Layer; 4] = [
    Layer::Adapter,
    Layer::Application,
    Layer::Domain,
    Layer::Unknown,
];

fn edge_dir(from: Layer, to: Layer) -> EdgeDir {
    match (rank(from), rank(to)) {
        (Some(f), Some(t)) if t < f => EdgeDir::Inward,
        (Some(f), Some(t)) if t > f => EdgeDir::Outward,
        (Some(_), Some(_)) => EdgeDir::Lateral,
        _ => EdgeDir::Unranked,
    }
}

/// Pure core: bucket modules into fixed-order layer columns (name-sorted within
/// each) and classify every dependency edge by inward-ness. Deterministic.
pub fn layered(g: &ArchGraph) -> LayeredGraph {
    let columns = COLUMN_ORDER
        .iter()
        .map(|&layer| {
            let mut modules: Vec<ModuleId> = g
                .modules()
                .iter()
                .enumerate()
                .filter(|(_, m)| m.layer == layer)
                .map(|(id, _)| id)
                .collect();
            modules.sort_by(|&a, &b| g.name(a).cmp(g.name(b)));
            LayerColumn { layer, modules }
        })
        .collect();

    let edges = g
        .edges()
        .into_iter()
        .map(|(from, to)| LgEdge {
            from,
            to,
            dir: edge_dir(g.modules()[from].layer, g.modules()[to].layer),
        })
        .collect();

    LayeredGraph { columns, edges }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// adapters → app → domain (all inward), plus a domain → adapters violation.
    fn fixture() -> ArchGraph {
        let mut g = ArchGraph::new();
        let adapters = g.ensure_module("adapters");
        let app = g.ensure_module("app");
        let domain = g.ensure_module("domain");
        let widgets = g.ensure_module("widgets"); // Unknown layer
        g.add_edge(adapters, app); // inward (3 -> 2)
        g.add_edge(app, domain); // inward (2 -> 1)
        g.add_edge(domain, adapters); // outward (1 -> 3) = violation
        g.add_edge(adapters, widgets); // unranked (Adapter -> Unknown)
        g
    }

    #[test]
    fn columns_are_outside_in_and_name_sorted() {
        let lg = layered(&fixture());
        let order: Vec<Layer> = lg.columns.iter().map(|c| c.layer).collect();
        assert_eq!(
            order,
            vec![Layer::Adapter, Layer::Application, Layer::Domain, Layer::Unknown]
        );
        let g = fixture();
        let adapter_names: Vec<&str> = lg.columns[0]
            .modules
            .iter()
            .map(|&id| g.name(id))
            .collect();
        assert_eq!(adapter_names, vec!["adapters"]);
        let unknown_names: Vec<&str> = lg.columns[3]
            .modules
            .iter()
            .map(|&id| g.name(id))
            .collect();
        assert_eq!(unknown_names, vec!["widgets"]);
    }

    #[test]
    fn edge_directions_are_classified() {
        let g = fixture();
        let lg = layered(&g);
        let dir = |from: &str, to: &str| {
            let f = g.module_id(from).unwrap();
            let t = g.module_id(to).unwrap();
            lg.edges
                .iter()
                .find(|e| e.from == f && e.to == t)
                .map(|e| e.dir)
                .unwrap()
        };
        assert_eq!(dir("adapters", "app"), EdgeDir::Inward);
        assert_eq!(dir("domain", "adapters"), EdgeDir::Outward);
        assert_eq!(dir("adapters", "widgets"), EdgeDir::Unranked);
    }

    #[test]
    fn lateral_edge_is_classified() {
        let mut g = ArchGraph::new();
        let a = g.ensure_module("adapters");
        let r = g.ensure_module("render"); // also Adapter
        g.add_edge(a, r);
        let lg = layered(&g);
        assert_eq!(lg.edges[0].dir, EdgeDir::Lateral);
    }
}
