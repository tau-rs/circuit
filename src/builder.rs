use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::Result;
use walkdir::WalkDir;

use crate::graph::ArchGraph;
use crate::lang::module_name_from_rel;
use crate::lang::rust::crate_deps_in_source;

/// Pure core: build a graph from (module_name, source_text) pairs.
/// Multiple sources may share a module name; modules accumulate.
pub fn build_graph_from_sources(sources: &[(String, String)]) -> ArchGraph {
    let mut g = ArchGraph::new();
    for (module, _) in sources {
        g.ensure_module(module);
    }
    for (module, src) in sources {
        let from = g.ensure_module(module);
        for dep in crate_deps_in_source(src) {
            let to = g.ensure_module(&dep);
            g.add_edge(from, to);
        }
    }
    g
}

/// IO adapter: walk `<root>/src` (or `<root>`), read `.rs` files, build the graph.
pub fn build_graph(root: &Path) -> Result<ArchGraph> {
    if !root.exists() {
        anyhow::bail!("path not found: {}", root.display());
    }
    let src_root = root.join("src");
    let base = if src_root.is_dir() {
        src_root
    } else {
        root.to_path_buf()
    };

    let mut sources = Vec::new();
    for entry in WalkDir::new(&base).into_iter().filter_map(|e| e.ok()) {
        let p = entry.path();
        if p.extension().and_then(|e| e.to_str()) == Some("rs") {
            let rel = p
                .strip_prefix(&base)
                .unwrap_or(p)
                .to_string_lossy()
                .replace('\\', "/");
            let module = module_name_from_rel(&rel);
            let text = std::fs::read_to_string(p)?;
            sources.push((module, text));
        }
    }
    Ok(build_graph_from_sources(&sources))
}

/// IO adapter: map each module name to the sorted, deduped relative `.rs`
/// paths that contribute to it. Mirrors `build_graph`'s walk; used by the
/// interactive HTML map for drill-to-file. Keys/values are deterministic.
pub fn module_files(root: &Path) -> Result<BTreeMap<String, Vec<String>>> {
    if !root.exists() {
        anyhow::bail!("path not found: {}", root.display());
    }
    let src_root = root.join("src");
    let base = if src_root.is_dir() {
        src_root
    } else {
        root.to_path_buf()
    };

    let mut map: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for entry in WalkDir::new(&base).into_iter().filter_map(|e| e.ok()) {
        let p = entry.path();
        if p.extension().and_then(|e| e.to_str()) == Some("rs") {
            let rel = p
                .strip_prefix(&base)
                .unwrap_or(p)
                .to_string_lossy()
                .replace('\\', "/");
            let module = module_name_from_rel(&rel);
            map.entry(module).or_default().insert(rel);
        }
    }
    Ok(map
        .into_iter()
        .map(|(k, v)| (k, v.into_iter().collect()))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_edges_between_modules() {
        let sources = vec![
            (
                "adapters".to_string(),
                "use crate::domain::Order;".to_string(),
            ),
            ("domain".to_string(), "pub struct Order;".to_string()),
        ];
        let g = build_graph_from_sources(&sources);
        let a = g.module_id("adapters").unwrap();
        let d = g.module_id("domain").unwrap();
        assert_eq!(g.edges(), vec![(a, d)]);
    }

    #[test]
    fn build_graph_reads_a_temp_repo() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        std::fs::create_dir_all(src.join("domain")).unwrap();
        std::fs::write(src.join("domain/order.rs"), "pub struct Order;").unwrap();
        std::fs::write(src.join("adapters.rs"), "use crate::domain::Order;").unwrap();

        let g = build_graph(dir.path()).unwrap();
        let a = g.module_id("adapters").unwrap();
        let d = g.module_id("domain").unwrap();
        assert_eq!(g.edges(), vec![(a, d)]);
    }

    #[test]
    fn missing_path_is_an_error() {
        let result = build_graph(std::path::Path::new("/no/such/circuit/path/xyz"));
        assert!(result.is_err());
    }

    #[test]
    fn module_files_maps_modules_to_sorted_paths() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        std::fs::create_dir_all(src.join("app")).unwrap();
        std::fs::write(src.join("main.rs"), "fn main() {}").unwrap();
        std::fs::write(src.join("app/mod.rs"), "pub fn run() {}").unwrap();

        let m = module_files(dir.path()).unwrap();

        assert_eq!(m.get("root").unwrap(), &vec!["main.rs".to_string()]);
        assert_eq!(m.get("app").unwrap(), &vec!["app/mod.rs".to_string()]);
    }

    #[test]
    fn module_files_missing_path_is_an_error() {
        assert!(module_files(std::path::Path::new("/no/such/circuit/xyz")).is_err());
    }
}
