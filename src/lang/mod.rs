pub mod rust;

/// Extract the top-level crate-internal module a `use` line depends on.
/// Returns `None` for external crates, `super`/`self` paths, glob/grouped
/// imports whose first segment is ambiguous, and non-`use` text.
pub fn extract_crate_dep(use_text: &str) -> Option<String> {
    let t = use_text.trim();
    let t = t.strip_prefix("pub ").unwrap_or(t).trim_start();
    let t = t.strip_prefix("use ")?;
    let t = t.trim().trim_end_matches(';').trim();

    let mut segs = t.split("::");
    if segs.next()? != "crate" {
        return None;
    }
    let module = segs.next()?.trim();
    if module.is_empty() || module.contains('{') || module.contains('*') {
        return None;
    }
    Some(module.to_string())
}

/// Derive a top-level module name from a path relative to the source root.
/// `domain/order.rs` -> `domain`; `graph.rs` -> `graph`; `main.rs`/`lib.rs` -> `root`.
pub fn module_name_from_rel(rel: &str) -> String {
    let rel = rel.trim_start_matches("./");
    let parts: Vec<&str> = rel.split('/').filter(|p| !p.is_empty()).collect();
    if parts.len() >= 2 {
        return parts[0].to_string();
    }
    let file = parts.first().copied().unwrap_or("").trim_end_matches(".rs");
    if file == "main" || file == "lib" {
        "root".to_string()
    } else {
        file.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_simple_crate_dep() {
        assert_eq!(
            extract_crate_dep("use crate::graph::ArchGraph;"),
            Some("graph".into())
        );
        assert_eq!(extract_crate_dep("use crate::lang;"), Some("lang".into()));
        assert_eq!(
            extract_crate_dep("pub use crate::render::mermaid;"),
            Some("render".into())
        );
    }

    #[test]
    fn grouped_under_a_module_still_resolves() {
        assert_eq!(
            extract_crate_dep("use crate::layer::{layer_of, Layer};"),
            Some("layer".into())
        );
    }

    #[test]
    fn ignores_external_and_ambiguous() {
        assert_eq!(extract_crate_dep("use std::collections::HashMap;"), None);
        assert_eq!(extract_crate_dep("use super::foo;"), None);
        assert_eq!(extract_crate_dep("use crate::{a, b};"), None);
        assert_eq!(extract_crate_dep("let x = 1;"), None);
    }

    #[test]
    fn derives_module_name_from_path() {
        assert_eq!(module_name_from_rel("domain/order.rs"), "domain");
        assert_eq!(module_name_from_rel("graph.rs"), "graph");
        assert_eq!(module_name_from_rel("lang/rust.rs"), "lang");
        assert_eq!(module_name_from_rel("main.rs"), "root");
        assert_eq!(module_name_from_rel("lib.rs"), "root");
    }
}
