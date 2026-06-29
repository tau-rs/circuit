use std::collections::{BTreeMap, BTreeSet};

use crate::cockpit::health::Health;
use crate::graph::ArchGraph;
use crate::model::projection::{Component, SystemProjection};

/// A derived edge between two declared components that the projection's `edge`
/// allowlist does not sanction — a broken planned boundary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BrokenEdge {
    /// Component names (design vocabulary).
    pub from: String,
    pub to: String,
    /// The derived modules they map to (for the message).
    pub from_module: String,
    pub to_module: String,
}

/// Result of diffing reality (graph) against intent (projection).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Conformance {
    pub violations: Vec<BrokenEdge>,
    /// Declared component names whose `effective_module` is not a node in the graph.
    pub uncovered: Vec<String>,
}

impl Conformance {
    /// Verdict on the existing ladder. NOT wired into SessionHealth in this slice.
    pub fn health(&self) -> Health {
        if !self.violations.is_empty() {
            Health::Critical
        } else if !self.uncovered.is_empty() {
            Health::Unknown
        } else {
            Health::Sound
        }
    }
}

/// Diff the derived graph against the system projection. See the design's
/// "Rules (precise)" section. Output vectors are sorted for determinism.
pub fn check(graph: &ArchGraph, proj: &SystemProjection) -> Conformance {
    // component name -> effective module
    let module_of: BTreeMap<&str, &str> = proj
        .component
        .iter()
        .map(|c| (c.name.as_str(), c.effective_module()))
        .collect();

    // the set of modules under design control
    let declared_modules: BTreeSet<&str> = module_of.values().copied().collect();

    // allowed module pairs, translated from projection edges (which name components).
    // An edge naming an unknown component is ignored (authoring slip, not a code violation).
    let mut allowed: BTreeSet<(&str, &str)> = BTreeSet::new();
    for e in &proj.edge {
        if let (Some(&fm), Some(&tm)) =
            (module_of.get(e.from.as_str()), module_of.get(e.to.as_str()))
        {
            allowed.insert((fm, tm));
        }
    }

    // module -> a single component name for messages. When two components map to
    // the same module, pick the first by sorted component name (deterministic).
    let mut comps_sorted: Vec<&Component> = proj.component.iter().collect();
    comps_sorted.sort_by(|a, b| a.name.cmp(&b.name));
    let mut component_of_module: BTreeMap<&str, &str> = BTreeMap::new();
    for c in &comps_sorted {
        component_of_module
            .entry(c.effective_module())
            .or_insert(c.name.as_str());
    }

    // violations: derived edges between two declared modules not in the allowlist.
    let mut violations = Vec::new();
    for (f, t) in graph.edges() {
        let fm = graph.name(f);
        let tm = graph.name(t);
        if declared_modules.contains(fm)
            && declared_modules.contains(tm)
            && !allowed.contains(&(fm, tm))
        {
            violations.push(BrokenEdge {
                from: component_of_module[fm].to_string(),
                to: component_of_module[tm].to_string(),
                from_module: fm.to_string(),
                to_module: tm.to_string(),
            });
        }
    }
    violations.sort_by(|a, b| (&a.from_module, &a.to_module).cmp(&(&b.from_module, &b.to_module)));

    // uncovered: declared components whose module is absent from the graph.
    let mut uncovered: Vec<String> = proj
        .component
        .iter()
        .filter(|c| graph.module_id(c.effective_module()).is_none())
        .map(|c| c.name.clone())
        .collect();
    uncovered.sort();
    uncovered.dedup();

    Conformance { violations, uncovered }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layer::Layer;
    use crate::model::projection::IntendedEdge;

    // Build a projection with the given (name, module) components and (from,to) component edges.
    fn proj(components: &[(&str, &str)], edges: &[(&str, &str)]) -> SystemProjection {
        let mut p = SystemProjection::new("checkout");
        p.component = components
            .iter()
            .map(|(n, m)| Component {
                name: (*n).into(),
                layer: Layer::Domain,
                module: Some((*m).into()),
            })
            .collect();
        p.edge = edges
            .iter()
            .map(|(f, t)| IntendedEdge { from: (*f).into(), to: (*t).into() })
            .collect();
        p
    }

    // Build a graph with the given module->module edges.
    fn graph(edges: &[(&str, &str)]) -> ArchGraph {
        let mut g = ArchGraph::new();
        for (f, t) in edges {
            let fi = g.ensure_module(f);
            let ti = g.ensure_module(t);
            g.add_edge(fi, ti);
        }
        g
    }

    #[test]
    fn allowed_edge_is_not_a_violation() {
        let p = proj(&[("billing", "model"), ("ghx", "adapters")], &[("ghx", "billing")]);
        let g = graph(&[("adapters", "model")]); // ghx->billing == adapters->model, allowed
        let c = check(&g, &p);
        assert!(c.violations.is_empty(), "got: {:?}", c.violations);
        assert!(c.uncovered.is_empty());
        assert_eq!(c.health(), Health::Sound);
    }

    #[test]
    fn forbidden_edge_between_declared_components_is_a_violation() {
        let p = proj(&[("billing", "model"), ("ghx", "adapters")], &[("ghx", "billing")]);
        let g = graph(&[("model", "adapters")]); // billing->ghx, NOT allowed
        let c = check(&g, &p);
        assert_eq!(c.violations.len(), 1, "got: {:?}", c.violations);
        let v = &c.violations[0];
        assert_eq!(v.from, "billing");
        assert_eq!(v.to, "ghx");
        assert_eq!(v.from_module, "model");
        assert_eq!(v.to_module, "adapters");
        assert_eq!(c.health(), Health::Critical);
    }

    #[test]
    fn edge_touching_an_undeclared_module_is_silent() {
        let p = proj(&[("billing", "model")], &[]);
        let g = graph(&[("model", "flow")]); // flow undeclared
        let c = check(&g, &p);
        assert!(c.violations.is_empty(), "got: {:?}", c.violations);
    }

    #[test]
    fn declared_component_with_no_module_is_uncovered() {
        let p = proj(&[("billing", "model"), ("cart", "cart")], &[]);
        let g = graph(&[]); // no modules at all -> both uncovered
        let c = check(&g, &p);
        assert_eq!(c.uncovered, vec!["billing".to_string(), "cart".to_string()]);
        assert_eq!(c.health(), Health::Unknown);
    }

    #[test]
    fn projected_edge_absent_from_code_is_silent() {
        let p = proj(&[("billing", "model"), ("ghx", "adapters")], &[("ghx", "billing")]);
        let g = graph(&[("adapters", "model")]); // only the allowed edge exists
        let c = check(&g, &p);
        assert!(c.violations.is_empty());
        // billing(model) and ghx(adapters) are both present as graph nodes -> covered
        assert!(c.uncovered.is_empty());
        assert_eq!(c.health(), Health::Sound);
    }
}
