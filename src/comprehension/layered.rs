use std::collections::BTreeSet;
use std::fmt::Write;

use crate::comprehension::callgraph::CallGraph;
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

#[derive(Clone, Debug, Default)]
pub struct FeatureOverlay {
    /// Raw selector the user passed.
    pub selector: String,
    /// Modules the feature's call-reachable functions live in (sorted by id, deduped).
    pub modules: Vec<ModuleId>,
    /// Indices into `LayeredGraph.edges` whose endpoints are both in `modules`.
    pub edges: Vec<usize>,
}

/// Resolve `target` like `impact` (bare name OR `module::name`, union all
/// matches), collect the modules of every call-reachable function, and induce
/// the subgraph edges among them. Empty `modules` means nothing matched.
pub fn overlay(
    g: &ArchGraph,
    calls: &CallGraph,
    target: &str,
    lg: &LayeredGraph,
) -> FeatureOverlay {
    let mut starts: Vec<usize> = Vec::new();
    for (id, node) in calls.nodes().iter().enumerate() {
        if node.name == target || node.qualified() == target {
            starts.push(id);
        }
    }

    let mut modset: BTreeSet<ModuleId> = BTreeSet::new();
    for &s in &starts {
        for fid in calls.reachable(s) {
            if let Some(mid) = g.module_id(&calls.node(fid).module) {
                modset.insert(mid);
            }
        }
    }

    let edges: Vec<usize> = lg
        .edges
        .iter()
        .enumerate()
        .filter(|(_, e)| modset.contains(&e.from) && modset.contains(&e.to))
        .map(|(i, _)| i)
        .collect();

    FeatureOverlay {
        selector: target.to_string(),
        modules: modset.into_iter().collect(),
        edges,
    }
}

/// Deterministic plain-text render: layer columns + edge summary + optional
/// feature overlay trailer.
pub fn render_text(g: &ArchGraph, lg: &LayeredGraph, overlay: Option<&FeatureOverlay>) -> String {
    if let Some(ov) = overlay {
        if ov.modules.is_empty() {
            return format!("no function matches '{}'\n", ov.selector);
        }
    }
    let members: BTreeSet<ModuleId> = overlay
        .map(|o| o.modules.iter().copied().collect())
        .unwrap_or_default();

    let mut out = String::new();
    let _ = writeln!(out, "layers (inward →)");
    for col in &lg.columns {
        let names: Vec<String> = col
            .modules
            .iter()
            .map(|&id| {
                let star = if members.contains(&id) { "*" } else { "" };
                format!("{}{}", g.name(id), star)
            })
            .collect();
        let body = if names.is_empty() {
            "(none)".to_string()
        } else {
            names.join("  ")
        };
        let _ = writeln!(out, "  [{:<11}] {}", format!("{:?}", col.layer), body);
    }

    let (mut inward, mut outward, mut lateral, mut unranked) = (0u32, 0u32, 0u32, 0u32);
    for e in &lg.edges {
        match e.dir {
            EdgeDir::Inward => inward += 1,
            EdgeDir::Outward => outward += 1,
            EdgeDir::Lateral => lateral += 1,
            EdgeDir::Unranked => unranked += 1,
        }
    }
    let _ = writeln!(
        out,
        "edges: {}  (inward {} · lateral {} · outward/violation {} · unranked {})",
        lg.edges.len(),
        inward,
        lateral,
        outward,
        unranked
    );
    for e in &lg.edges {
        if matches!(e.dir, EdgeDir::Outward) {
            let _ = writeln!(
                out,
                "  ⚠ {} → {}  (outward — dependency-rule violation)",
                g.name(e.from),
                g.name(e.to)
            );
        }
    }

    if let Some(ov) = overlay {
        let crossed: Vec<String> = lg
            .columns
            .iter()
            .filter(|c| c.modules.iter().any(|id| members.contains(id)))
            .map(|c| format!("{:?}", c.layer))
            .collect();
        let _ = writeln!(
            out,
            "feature · {} — spans {} modules, {} induced edges; crosses {}",
            ov.selector,
            ov.modules.len(),
            ov.edges.len(),
            crossed.join(" → ")
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::comprehension::callgraph::CallGraph;
    use crate::lang::FnDecl;

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

    fn fn_decl(name: &str, calls: &[&str]) -> FnDecl {
        FnDecl {
            name: name.into(),
            is_pub: false,
            is_test: false,
            is_main: name == "main",
            calls: calls.iter().map(|s| s.to_string()).collect(),
        }
    }

    /// Graph + matching call data: app::run -> domain::work; adapters::main -> app::run.
    fn overlay_fixture() -> (ArchGraph, CallGraph) {
        let mut g = ArchGraph::new();
        let adapters = g.ensure_module("adapters");
        let app = g.ensure_module("app");
        let domain = g.ensure_module("domain");
        g.add_edge(adapters, app);
        g.add_edge(app, domain);

        let decls = vec![
            ("adapters".to_string(), fn_decl("main", &["run"])),
            ("app".to_string(), fn_decl("run", &["work"])),
            ("domain".to_string(), fn_decl("work", &[])),
        ];
        (g, CallGraph::build(&decls))
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

    #[test]
    fn overlay_collects_reachable_modules_and_induced_edges() {
        let (g, calls) = overlay_fixture();
        let lg = layered(&g);
        let ov = overlay(&g, &calls, "main", &lg);

        let mut names: Vec<&str> = ov.modules.iter().map(|&id| g.name(id)).collect();
        names.sort();
        assert_eq!(names, vec!["adapters", "app", "domain"]);
        // Both edges (adapters->app, app->domain) are induced.
        assert_eq!(ov.edges.len(), 2);
        assert_eq!(ov.selector, "main");
    }

    #[test]
    fn overlay_no_match_is_empty() {
        let (g, calls) = overlay_fixture();
        let lg = layered(&g);
        let ov = overlay(&g, &calls, "nope", &lg);
        assert!(ov.modules.is_empty());
        assert!(ov.edges.is_empty());
    }

    #[test]
    fn overlay_unions_multiple_matches() {
        let mut g = ArchGraph::new();
        g.ensure_module("x");
        g.ensure_module("y");
        let decls = vec![
            ("x".to_string(), fn_decl("build", &[])),
            ("y".to_string(), fn_decl("build", &[])),
        ];
        let calls = CallGraph::build(&decls);
        let lg = layered(&g);
        let ov = overlay(&g, &calls, "build", &lg);
        let mut names: Vec<&str> = ov.modules.iter().map(|&id| g.name(id)).collect();
        names.sort();
        assert_eq!(names, vec!["x", "y"]);
    }

    #[test]
    fn render_lists_columns_and_edge_summary() {
        let lg = layered(&fixture());
        let g = fixture();
        let out = render_text(&g, &lg, None);
        assert!(out.contains("layers (inward →)"));
        assert!(out.contains("[Adapter"));
        assert!(out.contains("adapters"));
        assert!(out.contains("edges:"));
        // domain -> adapters is an outward violation in the fixture.
        assert!(out.contains("⚠ domain → adapters"));
    }

    #[test]
    fn render_marks_feature_members_and_trailer() {
        let (g, calls) = overlay_fixture();
        let lg = layered(&g);
        let ov = overlay(&g, &calls, "main", &lg);
        let out = render_text(&g, &lg, Some(&ov));
        assert!(out.contains("app*"));
        assert!(out.contains("feature · main"));
        assert!(out.contains("spans 3 modules"));
    }

    #[test]
    fn render_no_match_notice() {
        let (g, calls) = overlay_fixture();
        let lg = layered(&g);
        let ov = overlay(&g, &calls, "nope", &lg);
        let out = render_text(&g, &lg, Some(&ov));
        assert_eq!(out, "no function matches 'nope'\n");
    }
}
