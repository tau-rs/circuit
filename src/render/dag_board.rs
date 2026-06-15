//! The spec-level DAG board: a deterministic mermaid renderer whose nodes are
//! sessions/slices and whose edges are task dependencies. Flow never wears a
//! health color (design §8.2) — health appears only as a glyph in the label,
//! never in styling. A separate renderer from `render::mermaid` by design.

use crate::cockpit::health::Health;
use crate::flow::stage::{Stage, StageView};

/// One board node: a DAG node decorated with its derived stage and health.
/// `stage` is `Option` so a git-undeterminable stage (`None`) is distinct from
/// `Draft` — honesty the foundation `Stage` (no Unknown variant) cannot express.
pub struct BoardNode {
    pub id: String,
    pub depends_on: Vec<String>,
    pub stage: Option<StageView>,
    pub health: Health,
}

/// A whole board for one spec.
pub struct Board {
    pub nodes: Vec<BoardNode>,
}

/// Sanitize an id into a mermaid-safe node id (alphanumerics kept, rest `_`).
/// Reimplemented locally — the board is a separate renderer from `render::mermaid`.
fn node_id(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect()
}

/// Health glyph (design §9). Total over `Health`; `Warn` is reserved (no producer
/// yet) so `◐` never actually appears, but the mapping stays exhaustive.
pub fn glyph(h: Health) -> char {
    match h {
        Health::Sound => '●',
        Health::Warn => '◐',
        Health::Critical => '◍',
        Health::Unknown => '?',
    }
}

fn stage_name(s: Stage) -> &'static str {
    match s {
        Stage::Draft => "Draft",
        Stage::Project => "Project",
        Stage::Implement => "Implement",
        Stage::Review => "Review",
        Stage::Merge => "Merge",
        Stage::Done => "Done",
    }
}

/// The stage cell for a label. `None` -> `?` (git-undeterminable); a stage whose
/// forge-gated refinement is unconfirmed gets a trailing `?`.
pub fn stage_cell(stage: &Option<StageView>) -> String {
    match stage {
        None => "?".to_string(),
        Some(v) if v.forge_certain => stage_name(v.stage).to_string(),
        Some(v) => format!("{}?", stage_name(v.stage)),
    }
}

/// Render the board as a deterministic mermaid `graph TD`.
pub fn render(board: &Board) -> String {
    let mut nodes: Vec<&BoardNode> = board.nodes.iter().collect();
    nodes.sort_by(|a, b| a.id.cmp(&b.id));

    let mut out = String::from("graph TD\n");
    for n in &nodes {
        out.push_str(&format!(
            "  {}[\"{} · {} · {}\"]\n",
            node_id(&n.id),
            n.id,
            stage_cell(&n.stage),
            glyph(n.health),
        ));
    }

    // Precedence edges: for `node.depends_on = [dep]`, emit `dep --> node`.
    let mut edges: Vec<(String, String)> = Vec::new();
    for n in &nodes {
        for dep in &n.depends_on {
            edges.push((dep.clone(), n.id.clone()));
        }
    }
    edges.sort();
    for (from, to) in &edges {
        out.push_str(&format!("  {} --> {}\n", node_id(from), node_id(to)));
    }

    // Colorless styling: a single fixed class applied to every node regardless of
    // health. Health lives only in the label glyph, never here.
    if !nodes.is_empty() {
        out.push_str("classDef flow fill:#fff,stroke:#999;\n");
        let ids: Vec<String> = nodes.iter().map(|n| node_id(&n.id)).collect();
        out.push_str(&format!("class {} flow;\n", ids.join(",")));
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: &str, deps: &[&str], stage: Option<StageView>, health: Health) -> BoardNode {
        BoardNode {
            id: id.to_string(),
            depends_on: deps.iter().map(|s| s.to_string()).collect(),
            stage,
            health,
        }
    }

    fn certain(s: Stage) -> Option<StageView> {
        Some(StageView {
            stage: s,
            forge_certain: true,
        })
    }

    #[test]
    fn renders_header_and_sorted_node_labels() {
        let board = Board {
            nodes: vec![
                node("pay-slice", &[], certain(Stage::Review), Health::Sound),
                node(
                    "auth-slice",
                    &[],
                    certain(Stage::Implement),
                    Health::Critical,
                ),
            ],
        };
        let out = render(&board);
        assert!(out.starts_with("graph TD\n"));
        // sorted by id: auth before pay
        let auth = out.find("auth-slice").unwrap();
        let pay = out.find("pay-slice").unwrap();
        assert!(auth < pay);
        assert!(out.contains(r#"auth_slice["auth-slice · Implement · ◍"]"#));
        assert!(out.contains(r#"pay_slice["pay-slice · Review · ●"]"#));
    }

    #[test]
    fn glyph_covers_every_health() {
        assert_eq!(glyph(Health::Sound), '●');
        assert_eq!(glyph(Health::Warn), '◐');
        assert_eq!(glyph(Health::Critical), '◍');
        assert_eq!(glyph(Health::Unknown), '?');
    }

    #[test]
    fn stage_cell_renders_unknown_uncertain_and_certain() {
        assert_eq!(stage_cell(&None), "?");
        assert_eq!(
            stage_cell(&Some(StageView {
                stage: Stage::Implement,
                forge_certain: true
            })),
            "Implement"
        );
        // forge-gated refinement unconfirmed -> trailing '?'
        assert_eq!(
            stage_cell(&Some(StageView {
                stage: Stage::Review,
                forge_certain: false
            })),
            "Review?"
        );
    }

    #[test]
    fn unknown_stage_and_health_render_as_question_marks() {
        let board = Board {
            nodes: vec![node("x", &[], None, Health::Unknown)],
        };
        let out = render(&board);
        assert!(out.contains(r#"x["x · ? · ?"]"#));
    }

    #[test]
    fn renders_precedence_edges_sorted() {
        // pay depends on auth -> precedence edge auth --> pay
        let board = Board {
            nodes: vec![
                node("auth", &[], certain(Stage::Done), Health::Sound),
                node("pay", &["auth"], certain(Stage::Implement), Health::Sound),
            ],
        };
        let out = render(&board);
        assert!(out.contains("auth --> pay"));
        assert!(!out.contains("pay --> auth"));
    }

    #[test]
    fn applies_one_colorless_class_to_all_nodes() {
        let board = Board {
            nodes: vec![
                node("a", &[], certain(Stage::Implement), Health::Critical),
                node("b", &["a"], certain(Stage::Review), Health::Sound),
            ],
        };
        let out = render(&board);
        let defs: Vec<&str> = out
            .lines()
            .filter(|l| l.trim_start().starts_with("classDef"))
            .collect();
        assert_eq!(defs.len(), 1);
        assert!(defs[0].contains("classDef flow"));
        assert!(out.contains("class a,b flow;"));
    }

    /// The headline invariant: styling is a function of structure only, never of
    /// health. Flipping every node's health leaves the classDef/class/style lines
    /// byte-identical (design §8.2 / §5.3).
    #[test]
    fn styling_never_varies_with_health() {
        let mk = |h: Health| Board {
            nodes: vec![
                node("a", &[], certain(Stage::Implement), h),
                node("b", &["a"], None, h),
            ],
        };
        let style_lines = |s: &str| {
            s.lines()
                .filter(|l| {
                    let t = l.trim_start();
                    t.starts_with("classDef") || t.starts_with("class ") || t.starts_with("style")
                })
                .map(|l| l.to_string())
                .collect::<Vec<_>>()
        };
        let sound = render(&mk(Health::Sound));
        let critical = render(&mk(Health::Critical));
        let unknown = render(&mk(Health::Unknown));
        assert_eq!(style_lines(&sound), style_lines(&critical));
        assert_eq!(style_lines(&sound), style_lines(&unknown));
        // Sanity: health DID change the output — just not the styling (it's in the label).
        assert_ne!(sound, critical);
    }

    #[test]
    fn empty_board_is_just_the_header() {
        let board = Board { nodes: vec![] };
        assert_eq!(render(&board), "graph TD\n");
    }
}
