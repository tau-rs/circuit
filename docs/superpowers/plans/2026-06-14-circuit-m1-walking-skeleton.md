# Circuit M1 Walking Skeleton — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A CLI (`circuit analyze <path>`) that parses a Rust repo with tree-sitter, builds a top-level module dependency graph, computes two deterministic indicators (No-cycles / ADP and Dependency rule), and emits a mermaid architecture diagram.

**Architecture:** Hexagonal, dogfooding the product's own thesis. Pure domain logic (graph model, indicators, mermaid render, string parsers) is isolated from IO adapters (tree-sitter parsing, filesystem walking, CLI). Everything derivable is computed from the repo and never stored.

**Tech Stack:** Rust 2021, `tree-sitter` + `tree-sitter-rust` (parsing), `walkdir` (filesystem), `clap` (CLI), `anyhow` (errors); dev: `assert_cmd`, `predicates`, `tempfile`.

**Parallelization (task DAG):** After Task 1, Tasks 2 and 3 are independent (parallel-eligible). After Task 4, Tasks 7 and 8 are independent (parallel-eligible). Task 9 depends on the `Violation` type from Task 8.

---

## File structure

| File | Responsibility |
|---|---|
| `Cargo.toml` | Package + deps |
| `src/lib.rs` | Library root; module declarations; `#![forbid(unsafe_code)]` |
| `src/layer.rs` | `Layer` enum, `layer_of` convention map, `rank` ordering |
| `src/graph.rs` | `ArchGraph` model: modules + deduped edges (derived, never stored) |
| `src/lang/mod.rs` | Pure string parsers: `extract_crate_dep`, `module_name_from_rel` |
| `src/lang/rust.rs` | tree-sitter adapter: `parse`, `crate_deps_in_source` |
| `src/builder.rs` | `build_graph_from_sources` (pure) + `build_graph` (IO) |
| `src/indicators/mod.rs` | Indicator module declarations |
| `src/indicators/cycles.rs` | `find_cycles` (Tarjan SCC) |
| `src/indicators/dependency_rule.rs` | `Violation`, `violations` |
| `src/render/mod.rs` | Render module declarations |
| `src/render/mermaid.rs` | `render` graph + findings → mermaid string |
| `src/main.rs` | `clap` CLI: `circuit analyze <path>` |
| `tests/cli.rs` | End-to-end CLI integration test |

---

## Task 1: Scaffold the cargo project

**Files:**
- Create: `Cargo.toml`
- Create: `src/lib.rs`
- Create: `src/main.rs`
- Modify: `.gitignore`

- [ ] **Step 1: Create the cargo manifest**

Create `Cargo.toml`:

```toml
[package]
name = "circuit"
version = "0.1.0"
edition = "2021"
description = "Local-first, git-driven IDE core — architecture derivation & visualization"

[dependencies]
tree-sitter = "0.22"
tree-sitter-rust = "0.21"
walkdir = "2"
clap = { version = "4", features = ["derive"] }
anyhow = "1"

[dev-dependencies]
assert_cmd = "2"
predicates = "3"
tempfile = "3"
```

> Version note: pinned to `tree-sitter 0.22` + `tree-sitter-rust 0.21`, where `tree_sitter_rust::language()` returns a `Language` and `Parser::set_language` takes `&Language`. If you bump `tree-sitter-rust` to ≥0.23, replace `&tree_sitter_rust::language()` with `&tree_sitter_rust::LANGUAGE.into()`.

- [ ] **Step 2: Create the library root with a smoke test**

Create `src/lib.rs`:

```rust
#![forbid(unsafe_code)]

pub mod builder;
pub mod graph;
pub mod indicators;
pub mod lang;
pub mod layer;
pub mod render;

#[cfg(test)]
mod smoke {
    #[test]
    fn harness_runs() {
        assert_eq!(2 + 2, 4);
    }
}
```

- [ ] **Step 3: Create a minimal binary**

Create `src/main.rs`:

```rust
fn main() {
    println!("circuit");
}
```

> The module declarations in `lib.rs` reference files created in later tasks, so the crate will not compile until they exist. Create empty stub files now so Step 4 compiles:
> - `src/layer.rs`, `src/graph.rs`, `src/builder.rs` — empty
> - `src/lang/mod.rs` with `pub mod rust;` and `src/lang/rust.rs` — empty
> - `src/indicators/mod.rs` with `pub mod cycles;` and `pub mod dependency_rule;`, plus empty `src/indicators/cycles.rs`, `src/indicators/dependency_rule.rs`
> - `src/render/mod.rs` with `pub mod mermaid;`, plus empty `src/render/mermaid.rs`

- [ ] **Step 4: Run the smoke test to verify the harness**

Run: `cargo test --lib smoke`
Expected: PASS (`harness_runs ... ok`). First run downloads and compiles deps.

- [ ] **Step 5: Ignore build artifacts and commit**

Append to `.gitignore`:

```
/target
```

```bash
git add Cargo.toml Cargo.lock src/ .gitignore
git commit -m "chore: scaffold circuit crate (M1 skeleton)"
```

---

## Task 2: Layer model  _(parallel-eligible with Task 3)_

**Files:**
- Modify: `src/layer.rs`

- [ ] **Step 1: Write the failing tests**

Replace `src/layer.rs` with:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layer {
    Domain,
    Application,
    Adapter,
    Unknown,
}

/// Assign a layer to a top-level module name by convention.
pub fn layer_of(module: &str) -> Layer {
    match module {
        "domain" | "entities" | "model" => Layer::Domain,
        "application" | "app" | "usecase" | "usecases" | "use_cases" => Layer::Application,
        "adapters" | "adapter" | "infra" | "infrastructure" | "persistence" | "cli" | "render"
        | "lang" => Layer::Adapter,
        _ => Layer::Unknown,
    }
}

/// Inward-ness rank: lower = more inner. `None` means "unranked" (skip in rules).
pub fn rank(layer: Layer) -> Option<u8> {
    match layer {
        Layer::Domain => Some(1),
        Layer::Application => Some(2),
        Layer::Adapter => Some(3),
        Layer::Unknown => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_known_names_to_layers() {
        assert_eq!(layer_of("domain"), Layer::Domain);
        assert_eq!(layer_of("usecase"), Layer::Application);
        assert_eq!(layer_of("adapters"), Layer::Adapter);
        assert_eq!(layer_of("render"), Layer::Adapter);
    }

    #[test]
    fn unknown_names_are_unknown() {
        assert_eq!(layer_of("graph"), Layer::Unknown);
        assert_eq!(layer_of("widgets"), Layer::Unknown);
    }

    #[test]
    fn rank_orders_inner_below_outer() {
        assert!(rank(Layer::Domain) < rank(Layer::Adapter));
        assert_eq!(rank(Layer::Unknown), None);
    }
}
```

- [ ] **Step 2: Run tests to verify they pass**

Run: `cargo test --lib layer`
Expected: PASS (3 tests). (Test and implementation are written together here because the logic is a pure lookup table; the tests fully pin the behavior.)

- [ ] **Step 3: Commit**

```bash
git add src/layer.rs
git commit -m "feat: layer convention map and inward-ness rank"
```

---

## Task 3: Pure source parsers  _(parallel-eligible with Task 2)_

**Files:**
- Modify: `src/lang/mod.rs`

- [ ] **Step 1: Write the failing tests**

Replace `src/lang/mod.rs` with:

```rust
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
        assert_eq!(extract_crate_dep("use crate::graph::ArchGraph;"), Some("graph".into()));
        assert_eq!(extract_crate_dep("use crate::lang;"), Some("lang".into()));
        assert_eq!(extract_crate_dep("pub use crate::render::mermaid;"), Some("render".into()));
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
```

- [ ] **Step 2: Run tests to verify they pass**

Run: `cargo test --lib lang::tests`
Expected: PASS (4 tests).

- [ ] **Step 3: Commit**

```bash
git add src/lang/mod.rs
git commit -m "feat: pure crate-dep and module-name parsers"
```

---

## Task 4: Architecture graph model

**Files:**
- Modify: `src/graph.rs`

- [ ] **Step 1: Write the failing tests**

Replace `src/graph.rs` with:

```rust
use std::collections::{BTreeSet, HashMap};

use crate::layer::{layer_of, Layer};

pub type ModuleId = usize;

#[derive(Clone, Debug)]
pub struct Module {
    pub name: String,
    pub layer: Layer,
}

#[derive(Default)]
pub struct ArchGraph {
    modules: Vec<Module>,
    index: HashMap<String, ModuleId>,
    edges: BTreeSet<(ModuleId, ModuleId)>,
}

impl ArchGraph {
    pub fn new() -> Self {
        Self::default()
    }

    /// Idempotent: returns the existing id or inserts a new module (layer assigned by convention).
    pub fn ensure_module(&mut self, name: &str) -> ModuleId {
        if let Some(&id) = self.index.get(name) {
            return id;
        }
        let id = self.modules.len();
        self.modules.push(Module { name: name.to_string(), layer: layer_of(name) });
        self.index.insert(name.to_string(), id);
        id
    }

    /// Adds a directed dependency edge. Self-edges are ignored; duplicates are deduped.
    pub fn add_edge(&mut self, from: ModuleId, to: ModuleId) {
        if from != to {
            self.edges.insert((from, to));
        }
    }

    pub fn module_id(&self, name: &str) -> Option<ModuleId> {
        self.index.get(name).copied()
    }

    pub fn modules(&self) -> &[Module] {
        &self.modules
    }

    /// Edges as a sorted, deduped vector.
    pub fn edges(&self) -> Vec<(ModuleId, ModuleId)> {
        self.edges.iter().copied().collect()
    }

    pub fn name(&self, id: ModuleId) -> &str {
        &self.modules[id].name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ensure_module_is_idempotent_and_assigns_layer() {
        let mut g = ArchGraph::new();
        let a = g.ensure_module("domain");
        let b = g.ensure_module("domain");
        assert_eq!(a, b);
        assert_eq!(g.modules().len(), 1);
        assert_eq!(g.modules()[a].layer, Layer::Domain);
    }

    #[test]
    fn edges_are_deduped_and_self_edges_ignored() {
        let mut g = ArchGraph::new();
        let a = g.ensure_module("adapters");
        let d = g.ensure_module("domain");
        g.add_edge(a, d);
        g.add_edge(a, d);
        g.add_edge(d, d);
        assert_eq!(g.edges(), vec![(a, d)]);
    }
}
```

- [ ] **Step 2: Run tests to verify they pass**

Run: `cargo test --lib graph::tests`
Expected: PASS (2 tests).

- [ ] **Step 3: Commit**

```bash
git add src/graph.rs
git commit -m "feat: ArchGraph model with deduped edges"
```

---

## Task 5: Rust tree-sitter adapter

**Files:**
- Modify: `src/lang/rust.rs`

- [ ] **Step 1: Write the failing test**

Replace `src/lang/rust.rs` with:

```rust
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
```

- [ ] **Step 2: Run the test to verify it passes**

Run: `cargo test --lib lang::rust`
Expected: PASS (2 tests). If you hit a `set_language` type error, see the version note in Task 1 Step 1.

- [ ] **Step 3: Commit**

```bash
git add src/lang/rust.rs
git commit -m "feat: tree-sitter Rust adapter extracting crate deps"
```

---

## Task 6: Graph builder

**Files:**
- Modify: `src/builder.rs`

- [ ] **Step 1: Write the failing tests**

Replace `src/builder.rs` with:

```rust
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
    let src_root = root.join("src");
    let base = if src_root.is_dir() { src_root } else { root.to_path_buf() };

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_edges_between_modules() {
        let sources = vec![
            ("adapters".to_string(), "use crate::domain::Order;".to_string()),
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
}
```

- [ ] **Step 2: Run tests to verify they pass**

Run: `cargo test --lib builder::tests`
Expected: PASS (2 tests).

- [ ] **Step 3: Commit**

```bash
git add src/builder.rs
git commit -m "feat: build architecture graph from sources and from a repo"
```

---

## Task 7: No-cycles (ADP) indicator  _(parallel-eligible with Task 8)_

**Files:**
- Modify: `src/indicators/cycles.rs`

- [ ] **Step 1: Write the failing tests**

Replace `src/indicators/cycles.rs` with:

```rust
use crate::graph::{ArchGraph, ModuleId};

/// Strongly-connected components of size > 1 are dependency cycles.
/// Returns each cycle as a sorted list of module names; the outer list is sorted.
pub fn find_cycles(graph: &ArchGraph) -> Vec<Vec<String>> {
    let n = graph.modules().len();
    let mut adj: Vec<Vec<ModuleId>> = vec![Vec::new(); n];
    for (f, t) in graph.edges() {
        adj[f].push(t);
    }

    let sccs = Tarjan::run(&adj, n);
    let mut cycles: Vec<Vec<String>> = sccs
        .into_iter()
        .filter(|c| c.len() > 1)
        .map(|mut c| {
            c.sort();
            c.into_iter().map(|i| graph.name(i).to_string()).collect()
        })
        .collect();
    cycles.sort();
    cycles
}

struct Tarjan<'a> {
    adj: &'a Vec<Vec<ModuleId>>,
    index: Vec<usize>,
    low: Vec<usize>,
    on_stack: Vec<bool>,
    stack: Vec<usize>,
    idx: usize,
    sccs: Vec<Vec<usize>>,
}

impl<'a> Tarjan<'a> {
    fn run(adj: &'a Vec<Vec<ModuleId>>, n: usize) -> Vec<Vec<usize>> {
        let mut t = Tarjan {
            adj,
            index: vec![usize::MAX; n],
            low: vec![0; n],
            on_stack: vec![false; n],
            stack: Vec::new(),
            idx: 0,
            sccs: Vec::new(),
        };
        for v in 0..n {
            if t.index[v] == usize::MAX {
                t.strongconnect(v);
            }
        }
        t.sccs
    }

    fn strongconnect(&mut self, v: usize) {
        self.index[v] = self.idx;
        self.low[v] = self.idx;
        self.idx += 1;
        self.stack.push(v);
        self.on_stack[v] = true;

        for &w in &self.adj[v].clone() {
            if self.index[w] == usize::MAX {
                self.strongconnect(w);
                self.low[v] = self.low[v].min(self.low[w]);
            } else if self.on_stack[w] {
                self.low[v] = self.low[v].min(self.index[w]);
            }
        }

        if self.low[v] == self.index[v] {
            let mut comp = Vec::new();
            loop {
                let w = self.stack.pop().unwrap();
                self.on_stack[w] = false;
                comp.push(w);
                if w == v {
                    break;
                }
            }
            self.sccs.push(comp);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acyclic_graph_has_no_cycles() {
        let mut g = ArchGraph::new();
        let a = g.ensure_module("adapters");
        let d = g.ensure_module("domain");
        g.add_edge(a, d);
        assert!(find_cycles(&g).is_empty());
    }

    #[test]
    fn detects_a_two_node_cycle() {
        let mut g = ArchGraph::new();
        let a = g.ensure_module("a");
        let b = g.ensure_module("b");
        g.add_edge(a, b);
        g.add_edge(b, a);
        assert_eq!(find_cycles(&g), vec![vec!["a".to_string(), "b".to_string()]]);
    }
}
```

- [ ] **Step 2: Run tests to verify they pass**

Run: `cargo test --lib indicators::cycles`
Expected: PASS (2 tests).

- [ ] **Step 3: Commit**

```bash
git add src/indicators/cycles.rs
git commit -m "feat: no-cycles (ADP) indicator via Tarjan SCC"
```

---

## Task 8: Dependency-rule indicator  _(parallel-eligible with Task 7)_

**Files:**
- Modify: `src/indicators/dependency_rule.rs`

- [ ] **Step 1: Write the failing tests**

Replace `src/indicators/dependency_rule.rs` with:

```rust
use crate::graph::ArchGraph;
use crate::layer::{rank, Layer};

/// An inner layer depending on an outer one (violates the Dependency Rule).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Violation {
    pub from: String,
    pub to: String,
    pub from_layer: Layer,
    pub to_layer: Layer,
}

/// Report edges where a more-inner module depends on a more-outer one.
/// Edges touching an `Unknown` (unranked) layer are skipped — we never fake a verdict.
pub fn violations(graph: &ArchGraph) -> Vec<Violation> {
    let mut out = Vec::new();
    for (f, t) in graph.edges() {
        let from_layer = graph.modules()[f].layer;
        let to_layer = graph.modules()[t].layer;
        if let (Some(rf), Some(rt)) = (rank(from_layer), rank(to_layer)) {
            if rf < rt {
                out.push(Violation {
                    from: graph.name(f).to_string(),
                    to: graph.name(t).to_string(),
                    from_layer,
                    to_layer,
                });
            }
        }
    }
    out.sort_by(|a, b| (&a.from, &a.to).cmp(&(&b.from, &b.to)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outer_depending_on_inner_is_allowed() {
        let mut g = ArchGraph::new();
        let a = g.ensure_module("adapters");
        let d = g.ensure_module("domain");
        g.add_edge(a, d);
        assert!(violations(&g).is_empty());
    }

    #[test]
    fn inner_depending_on_outer_is_a_violation() {
        let mut g = ArchGraph::new();
        let d = g.ensure_module("domain");
        let a = g.ensure_module("adapters");
        g.add_edge(d, a);
        let v = violations(&g);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].from, "domain");
        assert_eq!(v[0].to, "adapters");
        assert_eq!(v[0].from_layer, Layer::Domain);
        assert_eq!(v[0].to_layer, Layer::Adapter);
    }

    #[test]
    fn unknown_layers_are_skipped() {
        let mut g = ArchGraph::new();
        let graph_mod = g.ensure_module("graph");
        let a = g.ensure_module("adapters");
        g.add_edge(graph_mod, a);
        assert!(violations(&g).is_empty());
    }
}
```

- [ ] **Step 2: Run tests to verify they pass**

Run: `cargo test --lib indicators::dependency_rule`
Expected: PASS (3 tests).

- [ ] **Step 3: Commit**

```bash
git add src/indicators/dependency_rule.rs
git commit -m "feat: dependency-rule indicator with unknown-layer skip"
```

---

## Task 9: Mermaid renderer

**Files:**
- Modify: `src/render/mermaid.rs`

- [ ] **Step 1: Write the failing tests**

Replace `src/render/mermaid.rs` with:

```rust
use std::collections::HashSet;

use crate::graph::ArchGraph;
use crate::indicators::dependency_rule::Violation;

fn node_id(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect()
}

/// Render the graph as a mermaid `graph TD`. Cyclic modules are marked `⟲`;
/// violating edges are labelled `|VIOLATION|`. Output is deterministic (sorted).
pub fn render(graph: &ArchGraph, violations: &[Violation], cycles: &[Vec<String>]) -> String {
    let cyclic: HashSet<&str> = cycles.iter().flatten().map(|s| s.as_str()).collect();
    let viol: HashSet<(&str, &str)> =
        violations.iter().map(|v| (v.from.as_str(), v.to.as_str())).collect();

    let mut out = String::from("graph TD\n");

    let mut mods: Vec<&crate::graph::Module> = graph.modules().iter().collect();
    mods.sort_by(|a, b| a.name.cmp(&b.name));
    for m in &mods {
        let mark = if cyclic.contains(m.name.as_str()) { " ⟲" } else { "" };
        out.push_str(&format!(
            "  {}[\"{}<br/>({:?}){}\"]\n",
            node_id(&m.name),
            m.name,
            m.layer,
            mark
        ));
    }

    let mut edges: Vec<(String, String)> = graph
        .edges()
        .into_iter()
        .map(|(f, t)| (graph.name(f).to_string(), graph.name(t).to_string()))
        .collect();
    edges.sort();
    for (f, t) in edges {
        if viol.contains(&(f.as_str(), t.as_str())) {
            out.push_str(&format!("  {} -->|VIOLATION| {}\n", node_id(&f), node_id(&t)));
        } else {
            out.push_str(&format!("  {} --> {}\n", node_id(&f), node_id(&t)));
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_nodes_and_edges_deterministically() {
        let mut g = ArchGraph::new();
        let a = g.ensure_module("adapters");
        let d = g.ensure_module("domain");
        g.add_edge(a, d);

        let out = render(&g, &[], &[]);
        assert!(out.starts_with("graph TD\n"));
        assert!(out.contains("adapters[\"adapters<br/>(Adapter)\"]"));
        assert!(out.contains("domain[\"domain<br/>(Domain)\"]"));
        assert!(out.contains("adapters --> domain"));
    }

    #[test]
    fn marks_violations_and_cycles() {
        let mut g = ArchGraph::new();
        let d = g.ensure_module("domain");
        let a = g.ensure_module("adapters");
        g.add_edge(d, a);

        let violations = vec![Violation {
            from: "domain".into(),
            to: "adapters".into(),
            from_layer: crate::layer::Layer::Domain,
            to_layer: crate::layer::Layer::Adapter,
        }];
        let cycles = vec![vec!["adapters".to_string(), "domain".to_string()]];

        let out = render(&g, &violations, &cycles);
        assert!(out.contains("domain -->|VIOLATION| adapters"));
        assert!(out.contains("(Domain) ⟲"));
    }
}
```

- [ ] **Step 2: Run tests to verify they pass**

Run: `cargo test --lib render::mermaid`
Expected: PASS (2 tests).

- [ ] **Step 3: Commit**

```bash
git add src/render/mermaid.rs
git commit -m "feat: deterministic mermaid renderer with violation/cycle marks"
```

---

## Task 10: CLI wiring + integration test

**Files:**
- Modify: `src/main.rs`
- Create: `tests/cli.rs`

- [ ] **Step 1: Write the failing integration test**

Create `tests/cli.rs`:

```rust
use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn analyze_reports_indicators_and_mermaid() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("src");
    std::fs::create_dir_all(src.join("domain")).unwrap();
    // domain depends on adapters -> a dependency-rule violation
    std::fs::write(src.join("domain/order.rs"), "use crate::adapters::Db;").unwrap();
    std::fs::write(src.join("adapters.rs"), "pub struct Db;").unwrap();

    Command::cargo_bin("circuit")
        .unwrap()
        .arg("analyze")
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Dependency rule"))
        .stdout(predicate::str::contains("VIOLATION"))
        .stdout(predicate::str::contains("graph TD"));
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --test cli`
Expected: FAIL — the binary still prints only `circuit`, so the `stdout` predicates don't match.

- [ ] **Step 3: Implement the CLI**

Replace `src/main.rs` with:

```rust
use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "circuit", about = "Architecture derivation & visualization")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Analyze a Rust repo: indicators + mermaid diagram
    Analyze {
        #[arg(default_value = ".")]
        path: PathBuf,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Analyze { path } => {
            let graph = circuit::builder::build_graph(&path)?;
            let cycles = circuit::indicators::cycles::find_cycles(&graph);
            let violations = circuit::indicators::dependency_rule::violations(&graph);

            println!(
                "Architecture — No-cycles (ADP): {}",
                if cycles.is_empty() {
                    "● SOUND".to_string()
                } else {
                    format!("⛔ {} cyclic group(s)", cycles.len())
                }
            );
            for c in &cycles {
                println!("  cycle: {}", c.join(" → "));
            }

            println!(
                "Architecture — Dependency rule: {}",
                if violations.is_empty() {
                    "● SOUND".to_string()
                } else {
                    format!("⛔ {} violation(s)", violations.len())
                }
            );
            for v in &violations {
                println!(
                    "  {} ({:?}) → {} ({:?})  VIOLATION",
                    v.from, v.from_layer, v.to, v.to_layer
                );
            }

            println!("\n--- mermaid ---");
            println!("{}", circuit::render::mermaid::render(&graph, &violations, &cycles));
        }
    }
    Ok(())
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test --test cli`
Expected: PASS.

- [ ] **Step 5: Run the full suite**

Run: `cargo test`
Expected: PASS (all unit + integration tests).

- [ ] **Step 6: Commit**

```bash
git add src/main.rs tests/cli.rs
git commit -m "feat: circuit analyze CLI with integration test"
```

---

## Task 11: Dogfood on Circuit's own repo

**Files:** none (verification + docs)

- [ ] **Step 1: Run the analyzer on the crate itself**

Run: `cargo run -- analyze .`
Expected: prints both indicators (No-cycles likely `● SOUND`; Dependency rule likely `● SOUND` because the lib's internal modules are mostly `Unknown`/`Adapter`, which truthfully produces no violations) followed by a `graph TD` block whose nodes include `builder`, `graph`, `indicators`, `lang`, `layer`, `render` with edges between them.

- [ ] **Step 2: Capture the output as a sample artifact**

Run: `cargo run -- analyze . > docs/superpowers/samples/m1-self-analysis.txt`
(Create the `docs/superpowers/samples/` directory if needed.)

- [ ] **Step 3: Commit**

```bash
git add docs/superpowers/samples/m1-self-analysis.txt
git commit -m "docs: capture M1 self-analysis sample output"
```

---

## Self-Review

**Spec coverage (against `2026-06-14-circuit-vision-design.md` §16 M1 and §6.1):**
- Deterministic derivation from repo (tree-sitter) → Tasks 5, 6 ✓
- Architecture graph (derived, never stored) → Task 4 ✓
- Dependency rule indicator → Task 8 ✓
- No-cycles (ADP) indicator → Task 7 ✓
- mermaid/UML render → Task 9 ✓
- CLI-first, usable read-only on any repo → Task 10 ✓
- Adding a second language is implementing the `lang` trait → *partially*: M1 skeleton wires Rust directly rather than behind a trait object. A `LanguageAdapter` trait is deferred to the M1 full plan (the skeleton proves the pipeline first); `crate_deps_in_source` + `module_name_from_rel` are the seam that the trait will formalize. Noted as intentional scope.
- Determinism honesty (skip unrankable) → Task 8 `unknown_layers_are_skipped` ✓

**Placeholder scan:** none — every step contains complete code and exact commands.

**Type consistency:** `ArchGraph` API (`ensure_module`, `add_edge`, `edges`, `name`, `module_id`, `modules`) is consistent across Tasks 4/6/7/8/9. `Violation` fields (`from`, `to`, `from_layer`, `to_layer`) match between Tasks 8 and 9. `find_cycles -> Vec<Vec<String>>` and `violations -> Vec<Violation>` consumed identically in Tasks 9 and 10.

**Scope:** single crate, one milestone slice, produces a working CLI. Appropriately bounded.
