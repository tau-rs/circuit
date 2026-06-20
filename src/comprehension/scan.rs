use std::path::Path;

use anyhow::Result;
use walkdir::WalkDir;

use crate::lang::module_name_from_rel;
use crate::lang::rust::fn_decls_in_source;
use crate::lang::FnDecl;

/// Walk `<root>/src` (or `<root>` when there is no `src`), parse every `.rs`
/// file, and return (module, FnDecl) pairs. Mirrors `builder::build_graph`.
pub fn scan_functions(root: &Path) -> Result<Vec<(String, FnDecl)>> {
    if !root.exists() {
        anyhow::bail!("path not found: {}", root.display());
    }
    let src_root = root.join("src");
    let base = if src_root.is_dir() {
        src_root
    } else {
        root.to_path_buf()
    };

    let mut out = Vec::new();
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
            for d in fn_decls_in_source(&text) {
                out.push((module.clone(), d));
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scans_functions_with_module_names() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        std::fs::create_dir_all(src.join("domain")).unwrap();
        std::fs::write(src.join("main.rs"), "fn main() { greet(); }").unwrap();
        std::fs::write(src.join("domain/mod.rs"), "pub fn greet() {}").unwrap();

        let decls = scan_functions(dir.path()).unwrap();

        assert!(decls.iter().any(|(m, d)| m == "root" && d.name == "main"));
        assert!(decls
            .iter()
            .any(|(m, d)| m == "domain" && d.name == "greet"));
    }

    #[test]
    fn missing_path_is_an_error() {
        assert!(scan_functions(std::path::Path::new("/no/such/circuit/xyz")).is_err());
    }
}
