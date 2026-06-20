pub mod callgraph;
pub mod scan;

use std::fmt::Write;

use crate::lang::FnDecl;
use callgraph::CallGraph;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryKind {
    Main,
    Public,
    Test,
}

#[derive(Clone, Debug)]
pub struct FeatureGroup {
    /// Qualified name of the entry-point function.
    pub entry: String,
    pub kind: EntryKind,
    /// Qualified names reachable from the entry (sorted, deduped, inclusive).
    pub members: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub struct Comprehension {
    pub groups: Vec<FeatureGroup>,
}

/// Pure core: build the call graph, detect entry points (main / `#[test]` / pub),
/// and trace each to its reachable group. Deterministic (everything sorted).
pub fn comprehend(decls: &[(String, FnDecl)]) -> Comprehension {
    let g = CallGraph::build(decls);
    let mut groups = Vec::new();
    for (id, node) in g.nodes().iter().enumerate() {
        let kind = if node.is_main {
            EntryKind::Main
        } else if node.is_test {
            EntryKind::Test
        } else if node.is_pub {
            EntryKind::Public
        } else {
            continue;
        };
        let mut members: Vec<String> = g
            .reachable(id)
            .into_iter()
            .map(|m| g.node(m).qualified())
            .collect();
        members.sort();
        members.dedup();
        groups.push(FeatureGroup {
            entry: node.qualified(),
            kind,
            members,
        });
    }
    groups.sort_by(|a, b| a.entry.cmp(&b.entry));
    Comprehension { groups }
}

/// Deterministic plain-text render of the comprehension result.
pub fn render_text(c: &Comprehension) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "{} entry point(s)", c.groups.len());
    for grp in &c.groups {
        let kind = match grp.kind {
            EntryKind::Main => "main",
            EntryKind::Public => "pub",
            EntryKind::Test => "test",
        };
        let _ = writeln!(
            out,
            "\n[{}] {} — {} unit(s)",
            kind,
            grp.entry,
            grp.members.len()
        );
        for m in &grp.members {
            let _ = writeln!(out, "  {m}");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::FnDecl;

    fn decl(name: &str, is_pub: bool, is_main: bool, calls: &[&str]) -> FnDecl {
        FnDecl {
            name: name.into(),
            is_pub,
            is_test: false,
            is_main,
            calls: calls.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn entry_points_trace_to_reachable_groups() {
        let decls = vec![
            ("root".to_string(), decl("main", false, true, &["greet"])),
            ("domain".to_string(), decl("greet", true, false, &[])),
        ];
        let c = comprehend(&decls);

        assert_eq!(c.groups.len(), 2);
        let main = c.groups.iter().find(|g| g.entry == "root::main").unwrap();
        assert_eq!(main.kind, EntryKind::Main);
        assert_eq!(main.members, vec!["domain::greet", "root::main"]);

        let greet = c
            .groups
            .iter()
            .find(|g| g.entry == "domain::greet")
            .unwrap();
        assert_eq!(greet.kind, EntryKind::Public);
        assert_eq!(greet.members, vec!["domain::greet"]);
    }

    #[test]
    fn render_text_lists_entries_and_members() {
        let decls = vec![("root".to_string(), decl("main", false, true, &[]))];
        let out = render_text(&comprehend(&decls));
        assert!(out.contains("[main] root::main"));
    }
}
