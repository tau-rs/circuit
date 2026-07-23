use std::collections::BTreeMap;

use serde::Serialize;

use crate::comprehension::layered::{EdgeDir, FeatureOverlay, LayeredGraph};
use crate::graph::ArchGraph;

const TEMPLATE: &str = include_str!("html/template.html");

#[derive(Serialize)]
struct ColView<'a> {
    layer: String,
    modules: Vec<&'a str>,
}

#[derive(Serialize)]
struct EdgeView<'a> {
    from: &'a str,
    to: &'a str,
    dir: &'static str,
}

#[derive(Serialize)]
struct OverlayView {
    nodes: Vec<String>,
    edges: Vec<usize>,
}

#[derive(Serialize)]
struct MapView<'a> {
    columns: Vec<ColView<'a>>,
    edges: Vec<EdgeView<'a>>,
    overlays: BTreeMap<String, OverlayView>,
    files: &'a BTreeMap<String, Vec<String>>,
    initial: Option<String>,
}

fn dir_str(d: EdgeDir) -> &'static str {
    match d {
        EdgeDir::Inward => "inward",
        EdgeDir::Outward => "outward",
        EdgeDir::Lateral => "lateral",
        EdgeDir::Unranked => "unranked",
    }
}

/// Emit a self-contained interactive HTML document that hydrates the layered
/// graph. Names are resolved (no `ModuleId` leaks); every map is a `BTreeMap`
/// and every list is pre-sorted, so the output is byte-stable. Presentation
/// only — the pure core carries no `serde`.
pub fn render(
    g: &ArchGraph,
    lg: &LayeredGraph,
    overlays: &[(String, FeatureOverlay)],
    files: &BTreeMap<String, Vec<String>>,
    initial: Option<&str>,
) -> String {
    let columns = lg
        .columns
        .iter()
        .map(|c| ColView {
            layer: format!("{:?}", c.layer),
            modules: c.modules.iter().map(|&id| g.name(id)).collect(),
        })
        .collect();

    let edges = lg
        .edges
        .iter()
        .map(|e| EdgeView {
            from: g.name(e.from),
            to: g.name(e.to),
            dir: dir_str(e.dir),
        })
        .collect();

    let overlays_map = overlays
        .iter()
        .map(|(sel, ov)| {
            (
                sel.clone(),
                OverlayView {
                    nodes: ov
                        .modules
                        .iter()
                        .map(|&id| g.name(id).to_string())
                        .collect(),
                    edges: ov.edges.clone(),
                },
            )
        })
        .collect();

    let view = MapView {
        columns,
        edges,
        overlays: overlays_map,
        files,
        initial: initial.map(|s| s.to_string()),
    };

    // Escape angle brackets so a module/file name containing "</script>" cannot
    // break out of the inline <script> block that embeds this JSON payload.
    let json = serde_json::to_string(&view)
        .expect("MapView is always serializable")
        .replace('<', "\\u003c")
        .replace('>', "\\u003e");
    TEMPLATE.replace("__CIRCUIT_DATA__", &json)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::comprehension::callgraph::CallGraph;
    use crate::comprehension::layered::{layered, overlay};
    use crate::lang::FnDecl;

    #[test]
    fn render_wraps_document_and_embeds_payload() {
        let mut g = ArchGraph::new();
        let a = g.ensure_module("adapters");
        let d = g.ensure_module("domain");
        g.add_edge(a, d);
        let lg = layered(&g);
        let files: BTreeMap<String, Vec<String>> =
            BTreeMap::from([("adapters".to_string(), vec!["adapters.rs".to_string()])]);

        let out = render(&g, &lg, &[], &files, None);

        assert!(out.starts_with("<!DOCTYPE html>"));
        assert!(out.contains("<script"));
        assert!(!out.contains("__CIRCUIT_DATA__"));
        assert!(out.contains("\"adapters\""));
        // adapters(Adapter, rank 3) -> domain(Domain, rank 1) is inward.
        assert!(out.contains("\"dir\":\"inward\""));
    }

    #[test]
    fn render_embeds_overlay_and_initial() {
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
        let ov = overlay(&g, &calls, "app::run", &lg);
        let files = BTreeMap::new();

        let out = render(
            &g,
            &lg,
            &[("app::run".to_string(), ov)],
            &files,
            Some("app::run"),
        );

        assert!(out.contains("\"overlays\":{\"app::run\""));
        assert!(out.contains("\"initial\":\"app::run\""));
    }

    #[test]
    fn render_escapes_angle_brackets_to_prevent_script_breakout() {
        let mut g = ArchGraph::new();
        g.ensure_module("</script><img>");
        let lg = layered(&g);
        let files = BTreeMap::new();

        let out = render(&g, &lg, &[], &files, None);

        // A hostile module/file name must not terminate the inline <script>.
        assert!(!out.contains("</script><img>"));
        assert!(out.contains("\\u003c/script\\u003e\\u003cimg\\u003e"));
    }
}
