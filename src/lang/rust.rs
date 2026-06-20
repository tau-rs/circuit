use super::{extract_crate_dep, FnDecl};

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

/// Last identifier segment of a call callee: `a::b::foo` -> `foo`, `x.foo` -> `foo`.
fn callee_name(text: &str) -> Option<String> {
    let head = text.split(['(', '<', ' ']).next().unwrap_or(text);
    let seg = head.rsplit(['.', ':']).next().unwrap_or(head).trim();
    if seg.is_empty() {
        None
    } else {
        Some(seg.to_string())
    }
}

fn collect_calls(node: tree_sitter::Node, src: &str, out: &mut Vec<String>) {
    if node.kind() == "call_expression" {
        if let Some(callee) = node.child_by_field_name("function") {
            if let Ok(t) = callee.utf8_text(src.as_bytes()) {
                if let Some(name) = callee_name(t) {
                    out.push(name);
                }
            }
        }
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_calls(child, src, out);
    }
}

/// A function is a test if a preceding attribute (skipping comments) mentions `test`.
fn is_test_fn(node: tree_sitter::Node, src: &str) -> bool {
    let mut sib = node.prev_sibling();
    while let Some(s) = sib {
        match s.kind() {
            "attribute_item" => {
                if s
                    .utf8_text(src.as_bytes())
                    .map(|t| t.contains("test"))
                    .unwrap_or(false)
                {
                    return true;
                }
                sib = s.prev_sibling();
            }
            "line_comment" | "block_comment" => sib = s.prev_sibling(),
            _ => break,
        }
    }
    false
}

fn collect_fns(node: tree_sitter::Node, src: &str, out: &mut Vec<FnDecl>) {
    if node.kind() == "function_item" {
        let name = node
            .child_by_field_name("name")
            .and_then(|n| n.utf8_text(src.as_bytes()).ok())
            .unwrap_or("")
            .to_string();
        if !name.is_empty() {
            let mut is_pub = false;
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == "visibility_modifier" {
                    is_pub = true;
                }
            }
            let mut calls = Vec::new();
            if let Some(body) = node.child_by_field_name("body") {
                collect_calls(body, src, &mut calls);
            }
            out.push(FnDecl {
                is_main: name == "main",
                is_test: is_test_fn(node, src),
                is_pub,
                name,
                calls,
            });
        }
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_fns(child, src, out);
    }
}

/// All function declarations in a source file, in source order.
pub fn fn_decls_in_source(src: &str) -> Vec<FnDecl> {
    let tree = parse(src);
    let mut out = Vec::new();
    collect_fns(tree.root_node(), src, &mut out);
    out
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
        assert_eq!(
            crate_deps_in_source(src),
            vec!["graph".to_string(), "layer".to_string()]
        );
    }

    #[test]
    fn no_crate_deps_returns_empty() {
        let src = "use std::io;\npub struct X;";
        assert!(crate_deps_in_source(src).is_empty());
    }

    #[test]
    fn extracts_functions_with_flags_and_calls() {
        let src = r#"
            fn main() { greet(); }
            pub fn greet() { let _ = format(); }
            #[test]
            fn it_works() { greet(); }
            fn helper() {}
        "#;
        let decls = fn_decls_in_source(src);
        assert_eq!(decls.len(), 4);

        let main = &decls[0];
        assert_eq!(main.name, "main");
        assert!(main.is_main);
        assert!(main.calls.contains(&"greet".to_string()));

        let greet = &decls[1];
        assert_eq!(greet.name, "greet");
        assert!(greet.is_pub);
        assert!(greet.calls.contains(&"format".to_string()));

        let it_works = &decls[2];
        assert!(it_works.is_test);
        assert!(!it_works.is_pub);
    }
}
