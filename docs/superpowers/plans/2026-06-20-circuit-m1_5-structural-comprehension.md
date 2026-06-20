# Circuit M1.5 — Structural Comprehension Walking Skeleton Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a deterministic, zero-LLM function + call-graph substrate beneath M1's module graph, detect entry points, trace each to its reachable function set (the unnamed "feature groups"), and expose it via `circuit comprehend`.

**Architecture:** Extends the `lang` parse adapter to extract function declarations + their internal call names from Rust via tree-sitter, then adds a new `comprehension` subsystem (a function-level `CallGraph`, entry-point detection, reachability traces) consumed by the `app` use-case layer and a new CLI command. Follows the established pure-core + IO-adapter split (mirrors `builder::build_graph_from_sources` vs `build_graph`). This is the foundation slice of the Comprehension pillar (spec §11, **MVP-A**); semantic naming (Tau), the layered-graph-B renderer, and impact/blast-radius are **separate later plans**.

**Tech Stack:** Rust 2021, tree-sitter 0.22 + tree-sitter-rust 0.21, walkdir, clap 4, anyhow; assert_cmd + tempfile for integration tests.

## Global Constraints

- `#![forbid(unsafe_code)]` is set crate-wide (`lib.rs`, `main.rs`) — never introduce `unsafe`.
- `thiserror` at public boundaries, `anyhow` internally (this slice is internal/CLI → `anyhow::Result`).
- **Determinism is a product invariant:** all derived output must be deterministic — sort and dedupe before returning or printing. No `HashMap` iteration order in output.
- **Zero LLM in this slice.** Structure only. No network, no Tau, no semantic labels.
- Keep the pure-core (operates on in-memory `(module, FnDecl)` pairs) separate from the IO adapter (walks the filesystem) — mirror `builder.rs`.
- The call graph is **name-based and approximate** (receiver type is ignored; an ambiguous callee name links to every function with that name). This is intentional and honest — it is the deterministic skeleton later refined by clustering/Tau. Do not attempt type resolution here.
- Match existing test style: inline `#[cfg(test)] mod tests` per module; integration tests in `tests/cli.rs`.

---

### Task 1: Extract function declarations + call names from Rust source

**Files:**
- Modify: `src/lang/mod.rs` (add the `FnDecl` type)
- Modify: `src/lang/rust.rs` (add `fn_decls_in_source` + helpers)

**Interfaces:**
- Consumes: `tree_sitter`, existing `parse` in `src/lang/rust.rs`.
- Produces:
  - `lang::FnDecl { pub name: String, pub is_pub: bool, pub is_test: bool, pub is_main: bool, pub calls: Vec<String> }` (derives `Clone, Debug, PartialEq, Eq`)
  - `lang::rust::fn_decls_in_source(src: &str) -> Vec<FnDecl>` — functions in source order; `calls` are the trailing identifiers of `call_expression` callees inside each function body (macros like `println!` are not calls and are excluded).

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `src/lang/rust.rs`:

```rust
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
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib lang::rust::tests::extracts_functions_with_flags_and_calls`
Expected: FAIL — `cannot find function fn_decls_in_source`.

- [ ] **Step 3: Add the `FnDecl` type to `src/lang/mod.rs`**

Insert near the top of `src/lang/mod.rs`, after the `pub mod rust;` line:

```rust
/// A function declaration extracted from a source file (language-agnostic shape).
/// `calls` holds the trailing identifier of each call expression in the body
/// (e.g. `a::b::foo()` and `x.foo()` both contribute `"foo"`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FnDecl {
    pub name: String,
    pub is_pub: bool,
    pub is_test: bool,
    pub is_main: bool,
    pub calls: Vec<String>,
}
```

- [ ] **Step 4: Implement extraction in `src/lang/rust.rs`**

Change the first line of `src/lang/rust.rs` from `use super::extract_crate_dep;` to:

```rust
use super::{extract_crate_dep, FnDecl};
```

Then add, after the existing `crate_deps_in_source` function:

```rust
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
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --lib lang::`
Expected: PASS (the new test plus the existing `lang` tests).

- [ ] **Step 6: Commit**

```bash
git add src/lang/mod.rs src/lang/rust.rs
git commit -m "feat(comprehension): extract Rust function decls + call names via tree-sitter"
```

---

### Task 2: Function-level `CallGraph` with reachability

**Files:**
- Create: `src/comprehension/mod.rs` (module declaration only, this task)
- Create: `src/comprehension/callgraph.rs`
- Modify: `src/lib.rs` (register `pub mod comprehension;`)

**Interfaces:**
- Consumes: `lang::FnDecl` (Task 1).
- Produces:
  - `comprehension::callgraph::FnId` (= `usize`)
  - `comprehension::callgraph::FnNode { pub module: String, pub name: String, pub is_pub: bool, pub is_test: bool, pub is_main: bool }` with `fn qualified(&self) -> String` returning `"<module>::<name>"`
  - `comprehension::callgraph::CallGraph` with:
    - `build(decls: &[(String, FnDecl)]) -> CallGraph`
    - `nodes(&self) -> &[FnNode]`
    - `node(&self, id: FnId) -> &FnNode`
    - `edges(&self) -> Vec<(FnId, FnId)>` (sorted, deduped)
    - `reachable(&self, start: FnId) -> Vec<FnId>` (inclusive of `start`, ascending id order)

- [ ] **Step 1: Register the module and create the mod file**

Add to `src/lib.rs`, keeping the `pub mod` list alphabetical (between `cockpit` and `dag`):

```rust
pub mod comprehension;
```

Create `src/comprehension/mod.rs` with exactly:

```rust
pub mod callgraph;
```

- [ ] **Step 2: Write the failing test**

Create `src/comprehension/callgraph.rs` with only its test module first:

```rust
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
    fn builds_edges_and_reachability_by_name() {
        let decls = vec![
            ("root".to_string(), decl("main", false, true, &["greet"])),
            ("domain".to_string(), decl("greet", true, false, &[])),
        ];
        let g = CallGraph::build(&decls);

        assert_eq!(g.nodes().len(), 2);
        assert_eq!(g.node(0).qualified(), "root::main");
        assert_eq!(g.edges(), vec![(0, 1)]);
        assert_eq!(g.reachable(0), vec![0, 1]);
        assert_eq!(g.reachable(1), vec![1]);
    }
}
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test --lib comprehension::callgraph::tests::builds_edges_and_reachability_by_name`
Expected: FAIL — `cannot find type CallGraph in this scope`.

- [ ] **Step 4: Implement the call graph above the test module**

Prepend to `src/comprehension/callgraph.rs` (before the `#[cfg(test)]` block):

```rust
use std::collections::{BTreeSet, HashMap, VecDeque};

use crate::lang::FnDecl;

pub type FnId = usize;

#[derive(Clone, Debug)]
pub struct FnNode {
    pub module: String,
    pub name: String,
    pub is_pub: bool,
    pub is_test: bool,
    pub is_main: bool,
}

impl FnNode {
    pub fn qualified(&self) -> String {
        format!("{}::{}", self.module, self.name)
    }
}

#[derive(Default)]
pub struct CallGraph {
    nodes: Vec<FnNode>,
    by_name: HashMap<String, Vec<FnId>>,
    edges: BTreeSet<(FnId, FnId)>,
}

impl CallGraph {
    /// Build from (module, FnDecl) pairs. Calls resolve by name only: receiver
    /// type is ignored, so an ambiguous callee links to every match (approximate
    /// but deterministic — the skeleton later refined by clustering/Tau).
    pub fn build(decls: &[(String, FnDecl)]) -> Self {
        let mut g = CallGraph::default();
        for (module, d) in decls {
            let id = g.nodes.len();
            g.nodes.push(FnNode {
                module: module.clone(),
                name: d.name.clone(),
                is_pub: d.is_pub,
                is_test: d.is_test,
                is_main: d.is_main,
            });
            g.by_name.entry(d.name.clone()).or_default().push(id);
        }
        for (from, (_, d)) in decls.iter().enumerate() {
            for callee in &d.calls {
                if let Some(targets) = g.by_name.get(callee) {
                    for &to in targets {
                        if to != from {
                            g.edges.insert((from, to));
                        }
                    }
                }
            }
        }
        g
    }

    pub fn nodes(&self) -> &[FnNode] {
        &self.nodes
    }

    pub fn node(&self, id: FnId) -> &FnNode {
        &self.nodes[id]
    }

    pub fn edges(&self) -> Vec<(FnId, FnId)> {
        self.edges.iter().copied().collect()
    }

    /// All functions reachable from `start` (inclusive), in ascending id order.
    pub fn reachable(&self, start: FnId) -> Vec<FnId> {
        let mut adj: HashMap<FnId, Vec<FnId>> = HashMap::new();
        for &(f, t) in &self.edges {
            adj.entry(f).or_default().push(t);
        }
        let mut seen = BTreeSet::new();
        let mut q = VecDeque::new();
        seen.insert(start);
        q.push_back(start);
        while let Some(n) = q.pop_front() {
            if let Some(next) = adj.get(&n) {
                for &t in next {
                    if seen.insert(t) {
                        q.push_back(t);
                    }
                }
            }
        }
        seen.into_iter().collect()
    }
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --lib comprehension::`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src/lib.rs src/comprehension/mod.rs src/comprehension/callgraph.rs
git commit -m "feat(comprehension): function-level call graph with reachability"
```

---

### Task 3: Entry-point detection + `comprehend` core + text render

**Files:**
- Modify: `src/comprehension/mod.rs`

**Interfaces:**
- Consumes: `callgraph::{CallGraph, FnId}` (Task 2), `lang::FnDecl` (Task 1).
- Produces:
  - `comprehension::EntryKind` (`Main | Public | Test`, derives `Clone, Copy, Debug, PartialEq, Eq`)
  - `comprehension::FeatureGroup { pub entry: String, pub kind: EntryKind, pub members: Vec<String> }`
  - `comprehension::Comprehension { pub groups: Vec<FeatureGroup> }`
  - `comprehension::comprehend(decls: &[(String, FnDecl)]) -> Comprehension` — entry points are functions that are `main`, `#[test]`, or `pub`; each group's `members` is the sorted, deduped qualified names reachable from the entry; groups sorted by `entry`.
  - `comprehension::render_text(c: &Comprehension) -> String`

- [ ] **Step 1: Write the failing test**

Add a test module at the end of `src/comprehension/mod.rs`:

```rust
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

        let greet = c.groups.iter().find(|g| g.entry == "domain::greet").unwrap();
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
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib comprehension::tests::entry_points_trace_to_reachable_groups`
Expected: FAIL — `cannot find function comprehend in this scope`.

- [ ] **Step 3: Implement the core above the test module**

Insert into `src/comprehension/mod.rs`, replacing the existing single `pub mod callgraph;` line with:

```rust
pub mod callgraph;

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
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib comprehension::`
Expected: PASS (Task 2 + Task 3 tests).

- [ ] **Step 5: Commit**

```bash
git add src/comprehension/mod.rs
git commit -m "feat(comprehension): entry-point detection + reachable feature groups"
```

---

### Task 4: IO adapter — scan a repo into `(module, FnDecl)` pairs

**Files:**
- Create: `src/comprehension/scan.rs`
- Modify: `src/comprehension/mod.rs` (add `pub mod scan;`)

**Interfaces:**
- Consumes: `lang::rust::fn_decls_in_source` (Task 1), `lang::module_name_from_rel` (existing).
- Produces: `comprehension::scan::scan_functions(root: &Path) -> anyhow::Result<Vec<(String, FnDecl)>>` — walks `<root>/src` (or `<root>` if no `src`), parsing every `.rs` file; errors if `root` does not exist. Module name derived exactly as `builder::build_graph` does.

- [ ] **Step 1: Add the module declaration**

Add to the top of `src/comprehension/mod.rs`, directly under `pub mod callgraph;`:

```rust
pub mod scan;
```

- [ ] **Step 2: Write the failing test**

Create `src/comprehension/scan.rs` with only its test module first:

```rust
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
        assert!(decls.iter().any(|(m, d)| m == "domain" && d.name == "greet"));
    }

    #[test]
    fn missing_path_is_an_error() {
        assert!(scan_functions(std::path::Path::new("/no/such/circuit/xyz")).is_err());
    }
}
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test --lib comprehension::scan::tests::scans_functions_with_module_names`
Expected: FAIL — `cannot find function scan_functions in this scope`.

- [ ] **Step 4: Implement the scanner above the test module**

Prepend to `src/comprehension/scan.rs`:

```rust
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
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --lib comprehension::scan::`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src/comprehension/mod.rs src/comprehension/scan.rs
git commit -m "feat(comprehension): IO scanner from repo to (module, FnDecl) pairs"
```

---

### Task 5: App use-case — `comprehend(path) -> String`

**Files:**
- Modify: `src/app.rs` (add the `comprehend` use-case function)

**Interfaces:**
- Consumes: `comprehension::scan::scan_functions` (Task 4), `comprehension::{comprehend, render_text}` (Task 3).
- Produces: `app::comprehend(path: &std::path::Path) -> anyhow::Result<String>` — the formatted structural-comprehension report for a repo path.

- [ ] **Step 1: Write the failing test**

Add to the `tests` module at the bottom of `src/app.rs` (it already has one; place this test inside it):

```rust
    #[test]
    fn comprehend_reports_entry_points_for_a_repo() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        std::fs::create_dir_all(src.join("domain")).unwrap();
        std::fs::write(src.join("main.rs"), "fn main() { greet(); }").unwrap();
        std::fs::write(src.join("domain/mod.rs"), "pub fn greet() {}").unwrap();

        let out = super::comprehend(dir.path()).unwrap();
        assert!(out.contains("[main] root::main"));
        assert!(out.contains("domain::greet"));
    }
```

If `src/app.rs` has no `#[cfg(test)] mod tests` block, add one at the end of the file:

```rust
#[cfg(test)]
mod tests {
    #[test]
    fn comprehend_reports_entry_points_for_a_repo() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        std::fs::create_dir_all(src.join("domain")).unwrap();
        std::fs::write(src.join("main.rs"), "fn main() { greet(); }").unwrap();
        std::fs::write(src.join("domain/mod.rs"), "pub fn greet() {}").unwrap();

        let out = super::comprehend(dir.path()).unwrap();
        assert!(out.contains("[main] root::main"));
        assert!(out.contains("domain::greet"));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib app::tests::comprehend_reports_entry_points_for_a_repo`
Expected: FAIL — `cannot find function comprehend in module super` (or the `app` module).

- [ ] **Step 3: Implement the use-case**

Add this function to `src/app.rs` (top-level, e.g. directly after the existing `analyze` function):

```rust
/// Structural comprehension (deterministic, no LLM): entry points and the
/// functions reachable from each (the unnamed feature groups).
pub fn comprehend(path: &std::path::Path) -> anyhow::Result<String> {
    let decls = crate::comprehension::scan::scan_functions(path)?;
    let result = crate::comprehension::comprehend(&decls);
    Ok(crate::comprehension::render_text(&result))
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib app::tests::comprehend_reports_entry_points_for_a_repo`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/app.rs
git commit -m "feat(comprehension): app use-case comprehend(path) -> report"
```

---

### Task 6: CLI command `circuit comprehend` + integration test

**Files:**
- Modify: `src/main.rs` (add the `Comprehend` subcommand + handler)
- Modify: `tests/cli.rs` (integration test)

**Interfaces:**
- Consumes: `app::comprehend` (Task 5).
- Produces: the `circuit comprehend [path]` CLI command (defaults `path` to `.`), printing the report to stdout.

- [ ] **Step 1: Write the failing integration test**

Add to `tests/cli.rs` (it uses `assert_cmd` + `tempfile` — match the existing tests in that file for imports; if `Command`/`tempdir` are not already imported there, add `use assert_cmd::Command;` and `use tempfile::tempdir;` alongside the existing uses):

```rust
#[test]
fn comprehend_lists_entry_points() {
    let dir = tempdir().unwrap();
    let src = dir.path().join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("main.rs"), "fn main() { greet(); }\nfn greet() {}").unwrap();

    Command::cargo_bin("circuit")
        .unwrap()
        .arg("comprehend")
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicates::str::contains("[main] root::main"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --test cli comprehend_lists_entry_points`
Expected: FAIL — the binary rejects the unknown `comprehend` subcommand (non-zero exit).

- [ ] **Step 3: Add the subcommand variant**

In `src/main.rs`, add a variant to the `Command` enum (place it after the `Analyze` variant):

```rust
    /// Structural comprehension: entry points + reachable function groups (no LLM)
    Comprehend {
        #[arg(default_value = ".")]
        path: PathBuf,
    },
```

- [ ] **Step 4: Wire the match arm and handler**

In `src/main.rs`, add to the `match cli.command` block (after the `Command::Analyze` arm):

```rust
        Command::Comprehend { path } => run_comprehend(&path),
```

Then add the handler function (next to `run_analyze`):

```rust
fn run_comprehend(path: &Path) -> Result<()> {
    println!("{}", circuit::app::comprehend(path)?);
    Ok(())
}
```

- [ ] **Step 5: Run the integration test + full suite to verify they pass**

Run: `cargo test --test cli comprehend_lists_entry_points`
Expected: PASS.

Run: `cargo test`
Expected: PASS (whole suite — unit + integration).

- [ ] **Step 6: Commit**

```bash
git add src/main.rs tests/cli.rs
git commit -m "feat(comprehension): circuit comprehend CLI command"
```

---

### Task 7: Dogfood verification on Circuit's own repo

**Files:** none (verification only).

- [ ] **Step 1: Run comprehend on Circuit itself**

Run: `cargo run -- comprehend .`
Expected: a non-empty report beginning with an `N entry point(s)` line, listing `[main] root::main` and multiple `[pub] …` and `[test] …` entry points with their reachable members. Eyeball that members look plausible (e.g. `root::main`'s group spans several modules).

- [ ] **Step 2: Confirm determinism**

Run: `cargo run -- comprehend . > /tmp/c1.txt && cargo run -- comprehend . > /tmp/c2.txt && diff /tmp/c1.txt /tmp/c2.txt && echo DETERMINISTIC`
Expected: prints `DETERMINISTIC` (no diff).

- [ ] **Step 3: Format + lint**

Run: `cargo fmt && cargo clippy --all-targets -- -D warnings`
Expected: no diffs from fmt, no clippy warnings. Fix any, then:

```bash
git add -A
git commit -m "chore(comprehension): fmt + clippy clean for structural slice"
```

---

## Self-Review

**Spec coverage (against §6.1 static signal, §11 MVP-A, §14 exit criteria for the structural slice):**
- Entry-point index → Task 3 (`EntryKind`) + Task 6 (CLI). ✅
- Call-trace feature groups (unnamed) → Task 2 (call graph) + Task 3 (`reachable` → `members`). ✅
- Deterministic, zero-LLM, usable read-only on any repo → Tasks 1–7; dogfood in Task 7. ✅
- Unit-tested on fixture repos → Tasks 1–5 unit tests + Task 6 integration. ✅
- **Deliberately out of scope for this plan (later MVP-A plans):** layered-graph-B renderer, impact/blast-radius view, the clustering pre-pass (§6.1 signal 2) and opt-in dynamic test-trace seeding (§6.1 signal 3). This plan delivers only the static-signal substrate they build on. Noted so the gap is explicit, not silent.

**Placeholder scan:** No TBD/TODO; every code step contains complete code; every command has an expected result. ✅

**Type consistency:** `FnDecl` fields (`name/is_pub/is_test/is_main/calls`) are identical across Tasks 1–4. `CallGraph::build`/`nodes`/`node`/`edges`/`reachable` and `FnNode::qualified` used in Tasks 2–3 match their definitions. `comprehend`/`render_text` signatures match between Tasks 3 and 5. `scan_functions` return type matches `comprehend`'s parameter (`&[(String, FnDecl)]`). ✅
