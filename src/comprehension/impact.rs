use std::fmt::Write;

use crate::lang::FnDecl;

use super::callgraph::{CallGraph, FnId};

#[derive(Clone, Debug, Default)]
pub struct ImpactReport {
    /// The raw target selector the user passed.
    pub selector: String,
    /// Matched qualified names (sorted, deduped; empty when nothing matched).
    pub targets: Vec<String>,
    /// (hop, qualified) upstream cone — callers-of-callers.
    pub dependents: Vec<(u32, String)>,
    /// (hop, qualified) downstream cone — resolved internal callees.
    pub dependencies: Vec<(u32, String)>,
}

/// Pure core. Resolve `target` by bare name or `module::name`, union all
/// matches, then walk both directions with min-hop BFS. Deterministic:
/// cones sorted by (hop, qualified); targets (hop 0) excluded from cones.
pub fn impact(decls: &[(String, FnDecl)], target: &str, max_depth: Option<u32>) -> ImpactReport {
    let g = CallGraph::build(decls);

    let mut start: Vec<FnId> = Vec::new();
    for (id, node) in g.nodes().iter().enumerate() {
        if node.name == target || node.qualified() == target {
            start.push(id);
        }
    }

    let mut targets: Vec<String> = start.iter().map(|&id| g.node(id).qualified()).collect();
    targets.sort();
    targets.dedup();

    let cone = |raw: Vec<(FnId, u32)>| -> Vec<(u32, String)> {
        let mut out: Vec<(u32, String)> = raw
            .into_iter()
            .filter(|&(_, hop)| hop > 0 && max_depth.is_none_or(|m| hop <= m))
            .map(|(id, hop)| (hop, g.node(id).qualified()))
            .collect();
        out.sort();
        out.dedup();
        out
    };

    let (dependents, dependencies) = if start.is_empty() {
        (Vec::new(), Vec::new())
    } else {
        (
            cone(g.reverse_reachable_with_depth(&start)),
            cone(g.reachable_with_depth(&start)),
        )
    };

    ImpactReport {
        selector: target.to_string(),
        targets,
        dependents,
        dependencies,
    }
}

/// Deterministic plain-text render: header + two hop-grouped cones.
pub fn render_text(r: &ImpactReport) -> String {
    let mut out = String::new();
    if r.targets.is_empty() {
        let _ = writeln!(out, "no function matches '{}'", r.selector);
        return out;
    }
    if r.targets.len() > 1 {
        let _ = writeln!(
            out,
            "note: '{}' matches {} functions; reporting union blast radius:",
            r.selector,
            r.targets.len()
        );
        for t in &r.targets {
            let _ = writeln!(out, "        {t}");
        }
    }
    let _ = writeln!(out, "impact: {}  ({} target(s))", r.selector, r.targets.len());
    write_cone(&mut out, "▲ dependents (affected if changed)", &r.dependents);
    write_cone(&mut out, "▼ dependencies (what it relies on)", &r.dependencies);
    out
}

fn write_cone(out: &mut String, title: &str, cone: &[(u32, String)]) {
    let _ = writeln!(out, "\n{} — {} unit(s)", title, cone.len());
    for (hop, name) in cone {
        let _ = writeln!(out, "  ·{hop}  {name}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::FnDecl;

    fn decl(name: &str, calls: &[&str]) -> FnDecl {
        FnDecl {
            name: name.into(),
            is_pub: false,
            is_test: false,
            is_main: name == "main",
            calls: calls.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn chain() -> Vec<(String, FnDecl)> {
        vec![
            ("a".to_string(), decl("run", &["mid"])),
            ("a".to_string(), decl("mid", &["leaf"])),
            ("a".to_string(), decl("leaf", &[])),
        ]
    }

    #[test]
    fn dependents_cone_with_hops() {
        let r = impact(&chain(), "leaf", None);
        assert_eq!(r.targets, vec!["a::leaf"]);
        assert_eq!(
            r.dependents,
            vec![(1, "a::mid".to_string()), (2, "a::run".to_string())]
        );
        assert!(r.dependencies.is_empty());
    }

    #[test]
    fn dependencies_cone_with_hops() {
        let r = impact(&chain(), "run", None);
        assert_eq!(
            r.dependencies,
            vec![(1, "a::mid".to_string()), (2, "a::leaf".to_string())]
        );
        assert!(r.dependents.is_empty());
    }

    #[test]
    fn max_depth_caps_both_cones() {
        let r = impact(&chain(), "run", Some(1));
        assert_eq!(r.dependencies, vec![(1, "a::mid".to_string())]);
    }

    #[test]
    fn bare_name_unions_all_matches() {
        let decls = vec![
            ("x".to_string(), decl("build", &[])),
            ("y".to_string(), decl("build", &[])),
            ("z".to_string(), decl("caller", &["build"])),
        ];
        let r = impact(&decls, "build", None);
        assert_eq!(r.targets, vec!["x::build", "y::build"]);
        assert_eq!(r.dependents, vec![(1, "z::caller".to_string())]);
    }

    #[test]
    fn no_match_renders_notice() {
        let r = impact(&chain(), "nope", None);
        assert!(r.targets.is_empty());
        assert!(render_text(&r).contains("no function matches 'nope'"));
    }

    #[test]
    fn render_shows_both_cones() {
        let out = render_text(&impact(&chain(), "mid", None));
        assert!(out.contains("impact: mid"));
        assert!(out.contains("dependents"));
        assert!(out.contains("·1  a::run"));
        assert!(out.contains("dependencies"));
        assert!(out.contains("·1  a::leaf"));
    }
}
