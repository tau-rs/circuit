use super::extract_crate_dep;

/// Parse Rust source into a tree-sitter tree.
pub fn parse(src: &str) -> tree_sitter::Tree {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_rust::language())
        .expect("load Rust grammar");
    parser.parse(src, None).expect("parse Rust source")
}

fn collect_use_texts(node: tree_sitter::Node, src: &str, out: &mut Vec<String>) {
    if node.kind() == "use_declaration" {
        if let Ok(t) = node.utf8_text(src.as_bytes()) {
            out.push(t.to_string());
        }
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_use_texts(child, src, out);
    }
}

/// All top-level crate-internal modules this source depends on (sorted, deduped).
pub fn crate_deps_in_source(src: &str) -> Vec<String> {
    let tree = parse(src);
    let mut uses = Vec::new();
    collect_use_texts(tree.root_node(), src, &mut uses);
    let mut deps: Vec<String> = uses.iter().filter_map(|u| extract_crate_dep(u)).collect();
    deps.sort();
    deps.dedup();
    deps
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_crate_deps_ignoring_external() {
        let src = r#"
            use std::fmt;
            use crate::graph::ArchGraph;
            use crate::layer::{layer_of, Layer};

            pub fn f() {}
        "#;
        assert_eq!(crate_deps_in_source(src), vec!["graph".to_string(), "layer".to_string()]);
    }

    #[test]
    fn no_crate_deps_returns_empty() {
        let src = "use std::io;\npub struct X;";
        assert!(crate_deps_in_source(src).is_empty());
    }
}
