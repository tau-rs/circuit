# Circuit `map` — Layered Graph B Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `circuit map <path> [--feature <selector>] [--mermaid]` — a deterministic, zero-LLM module-level layered graph (layer columns + inward-edge classification + feature-induced-subgraph overlay), with mermaid as an export format only.

**Architecture:** Hexagonal, mirroring the `impact` verb. A pure core (`src/comprehension/layered.rs`) fuses `graph::ArchGraph` (modules + layers + edges) with `callgraph::CallGraph` (functions + reachability). An app use-case (`app::map`) owns IO and rendering selection. A thin CLI verb (`Command::Map`) prints the result. Mermaid gets a layered variant in `render::mermaid`.

**Tech Stack:** Rust, `clap` (derive), `anyhow` (app-internal). No new dependencies.

## Global Constraints

- **Zero LLM / zero network.** Pure deterministic computation only.
- **Determinism:** all rendered output sorted (fixed column order, modules by name, edges by `(from,to)`). No `HashMap` iteration reaches output.
- **Hexagonal:** core in `src/comprehension/layered.rs` is pure (no IO); IO (scan/build) lives in `app::map`; CLI is a thin wrapper.
- **`analyze` output must stay byte-stable** — do not touch its code paths.
- **`layer_of` is NOT modified** in this slice (nested modules → `Unknown` is acceptable, identical to `analyze` today).
- **Done gate (every task):** `cargo test` green; `cargo clippy --all-targets -- -D warnings` clean; `cargo fmt --all` clean.
- Reference spec: `docs/superpowers/specs/2026-06-29-circuit-map-layered-graph-design.md`.

**Existing APIs you will consume (verified signatures):**
- `crate::graph::{ArchGraph, Module, ModuleId}` — `ModuleId = usize`; `g.modules() -> &[Module]`; `Module { name: String, layer: Layer }` (both fields `pub`); `g.module_id(&str) -> Option<ModuleId>`; `g.name(ModuleId) -> &str`; `g.edges() -> Vec<(ModuleId, ModuleId)>` (sorted, deduped); test builders `g.ensure_module(&str) -> ModuleId`, `g.add_edge(from, to)`.
- `crate::layer::{Layer, rank}` — `enum Layer { Domain, Application, Adapter, Unknown }` (derives `Clone, Copy, Debug, PartialEq, Eq`); `rank(Layer) -> Option<u8>` (Domain=1, Application=2, Adapter=3, Unknown=None).
- `crate::comprehension::callgraph::{CallGraph, FnNode, FnId}` — `CallGraph::build(&[(String, FnDecl)]) -> CallGraph`; `g.nodes() -> &[FnNode]`; `g.node(FnId) -> &FnNode`; `g.reachable(FnId) -> Vec<FnId>` (includes the start node); `FnNode { module: String, name: String, .. }`; `node.qualified() -> String` (`"module::name"`).
- `crate::comprehension::scan::scan_functions(&Path) -> Result<Vec<(String, FnDecl)>>`.
- `crate::builder::build_graph(&Path) -> Result<ArchGraph>`.
- `render::mermaid::node_id(&str) -> String` (private, same-file use only).

---

### Task 1: Core layered model + `layered()`

**Files:**
- Create: `src/comprehension/layered.rs`
- Modify: `src/comprehension/mod.rs:1-3` (add `pub mod layered;`)

**Interfaces:**
- Consumes: `graph::{ArchGraph, ModuleId}`, `layer::{Layer, rank}`.
- Produces:
  - `pub struct LayeredGraph { pub columns: Vec<LayerColumn>, pub edges: Vec<LgEdge> }`
  - `pub struct LayerColumn { pub layer: Layer, pub modules: Vec<ModuleId> }`
  - `pub struct LgEdge { pub from: ModuleId, pub to: ModuleId, pub dir: EdgeDir }`
  - `pub enum EdgeDir { Inward, Outward, Lateral, Unranked }`
  - `pub fn layered(g: &ArchGraph) -> LayeredGraph`

- [ ] **Step 1: Register the module**

In `src/comprehension/mod.rs`, add to the top module list (after `pub mod impact;`):

```rust
pub mod layered;
```

- [ ] **Step 2: Write the failing tests**

Create `src/comprehension/layered.rs` with only the test module first:

```rust
use std::fmt::Write;

use crate::graph::{ArchGraph, ModuleId};
use crate::layer::{rank, Layer};

#[cfg(test)]
mod tests {
    use super::*;

    /// adapters → app → domain (all inward), plus a domain → adapters violation.
    fn fixture() -> ArchGraph {
        let mut g = ArchGraph::new();
        let adapters = g.ensure_module("adapters");
        let app = g.ensure_module("app");
        let domain = g.ensure_module("domain");
        let widgets = g.ensure_module("widgets"); // Unknown layer
        g.add_edge(adapters, app); // inward (3 -> 2)
        g.add_edge(app, domain); // inward (2 -> 1)
        g.add_edge(domain, adapters); // outward (1 -> 3) = violation
        g.add_edge(adapters, widgets); // unranked (Adapter -> Unknown)
        g
    }

    #[test]
    fn columns_are_outside_in_and_name_sorted() {
        let lg = layered(&fixture());
        let order: Vec<Layer> = lg.columns.iter().map(|c| c.layer).collect();
        assert_eq!(
            order,
            vec![Layer::Adapter, Layer::Application, Layer::Domain, Layer::Unknown]
        );
        let g = fixture();
        let adapter_names: Vec<&str> = lg.columns[0]
            .modules
            .iter()
            .map(|&id| g.name(id))
            .collect();
        assert_eq!(adapter_names, vec!["adapters"]);
        let unknown_names: Vec<&str> = lg.columns[3]
            .modules
            .iter()
            .map(|&id| g.name(id))
            .collect();
        assert_eq!(unknown_names, vec!["widgets"]);
    }

    #[test]
    fn edge_directions_are_classified() {
        let g = fixture();
        let lg = layered(&g);
        let dir = |from: &str, to: &str| {
            let f = g.module_id(from).unwrap();
            let t = g.module_id(to).unwrap();
            lg.edges
                .iter()
                .find(|e| e.from == f && e.to == t)
                .map(|e| e.dir)
                .unwrap()
        };
        assert_eq!(dir("adapters", "app"), EdgeDir::Inward);
        assert_eq!(dir("domain", "adapters"), EdgeDir::Outward);
        assert_eq!(dir("adapters", "widgets"), EdgeDir::Unranked);
    }

    #[test]
    fn lateral_edge_is_classified() {
        let mut g = ArchGraph::new();
        let a = g.ensure_module("adapters");
        let r = g.ensure_module("render"); // also Adapter
        g.add_edge(a, r);
        let lg = layered(&g);
        assert_eq!(lg.edges[0].dir, EdgeDir::Lateral);
    }
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test --lib comprehension::layered`
Expected: FAIL — `cannot find function 'layered'`, `cannot find type 'EdgeDir'`.

- [ ] **Step 4: Write the implementation**

Insert above the `#[cfg(test)]` block in `src/comprehension/layered.rs`:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EdgeDir {
    Inward,
    Outward,
    Lateral,
    Unranked,
}

#[derive(Clone, Debug)]
pub struct LgEdge {
    pub from: ModuleId,
    pub to: ModuleId,
    pub dir: EdgeDir,
}

#[derive(Clone, Debug)]
pub struct LayerColumn {
    pub layer: Layer,
    pub modules: Vec<ModuleId>,
}

#[derive(Clone, Debug, Default)]
pub struct LayeredGraph {
    pub columns: Vec<LayerColumn>,
    pub edges: Vec<LgEdge>,
}

/// Fixed outside-in column order: outermost adapters first, domain core last,
/// unranked modules trailing. Arrows point inward toward the core.
const COLUMN_ORDER: [Layer; 4] = [
    Layer::Adapter,
    Layer::Application,
    Layer::Domain,
    Layer::Unknown,
];

fn edge_dir(from: Layer, to: Layer) -> EdgeDir {
    match (rank(from), rank(to)) {
        (Some(f), Some(t)) if t < f => EdgeDir::Inward,
        (Some(f), Some(t)) if t > f => EdgeDir::Outward,
        (Some(_), Some(_)) => EdgeDir::Lateral,
        _ => EdgeDir::Unranked,
    }
}

/// Pure core: bucket modules into fixed-order layer columns (name-sorted within
/// each) and classify every dependency edge by inward-ness. Deterministic.
pub fn layered(g: &ArchGraph) -> LayeredGraph {
    let columns = COLUMN_ORDER
        .iter()
        .map(|&layer| {
            let mut modules: Vec<ModuleId> = g
                .modules()
                .iter()
                .enumerate()
                .filter(|(_, m)| m.layer == layer)
                .map(|(id, _)| id)
                .collect();
            modules.sort_by(|&a, &b| g.name(a).cmp(g.name(b)));
            LayerColumn { layer, modules }
        })
        .collect();

    let edges = g
        .edges()
        .into_iter()
        .map(|(from, to)| LgEdge {
            from,
            to,
            dir: edge_dir(g.modules()[from].layer, g.modules()[to].layer),
        })
        .collect();

    LayeredGraph { columns, edges }
}
```

Note: the `use std::fmt::Write;` import is unused until Task 3 — keep it but add `#[allow(unused_imports)]` is NOT needed because Task 3 lands before any clippy gate on a merged branch; if implementing Task 1 in isolation and clippy complains, remove the `Write` import here and re-add it in Task 3. (Subagent-driven execution runs the clippy gate after the whole branch, so leave it.)

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --lib comprehension::layered`
Expected: PASS — 3 tests.

- [ ] **Step 6: Commit**

```bash
git add src/comprehension/layered.rs src/comprehension/mod.rs
git commit -m "feat(comprehension): layered-graph core model (columns + edge classification)"
```

---

### Task 2: Feature overlay (`overlay()`)

**Files:**
- Modify: `src/comprehension/layered.rs` (add `FeatureOverlay` + `overlay`)

**Interfaces:**
- Consumes: `LayeredGraph` (Task 1), `callgraph::CallGraph`, `graph::ArchGraph`.
- Produces:
  - `pub struct FeatureOverlay { pub selector: String, pub modules: Vec<ModuleId>, pub edges: Vec<usize> }`
  - `pub fn overlay(g: &ArchGraph, calls: &CallGraph, target: &str, lg: &LayeredGraph) -> FeatureOverlay`
  - `overlay.edges` are indices into `lg.edges`. Empty `modules` ⟺ no function matched (`reachable` always includes the start, so any match yields ≥1 module).

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module in `src/comprehension/layered.rs`:

```rust
use crate::comprehension::callgraph::CallGraph;
use crate::lang::FnDecl;

fn fn_decl(name: &str, calls: &[&str]) -> FnDecl {
    FnDecl {
        name: name.into(),
        is_pub: false,
        is_test: false,
        is_main: name == "main",
        calls: calls.iter().map(|s| s.to_string()).collect(),
    }
}

/// Graph + matching call data: app::run -> domain::work; adapters::main -> app::run.
fn overlay_fixture() -> (ArchGraph, CallGraph) {
    let mut g = ArchGraph::new();
    let adapters = g.ensure_module("adapters");
    let app = g.ensure_module("app");
    let domain = g.ensure_module("domain");
    g.add_edge(adapters, app);
    g.add_edge(app, domain);

    let decls = vec![
        ("adapters".to_string(), fn_decl("main", &["run"])),
        ("app".to_string(), fn_decl("run", &["work"])),
        ("domain".to_string(), fn_decl("work", &[])),
    ];
    (g, CallGraph::build(&decls))
}

#[test]
fn overlay_collects_reachable_modules_and_induced_edges() {
    let (g, calls) = overlay_fixture();
    let lg = layered(&g);
    let ov = overlay(&g, &calls, "main", &lg);

    let mut names: Vec<&str> = ov.modules.iter().map(|&id| g.name(id)).collect();
    names.sort();
    assert_eq!(names, vec!["adapters", "app", "domain"]);
    // Both edges (adapters->app, app->domain) are induced.
    assert_eq!(ov.edges.len(), 2);
    assert_eq!(ov.selector, "main");
}

#[test]
fn overlay_no_match_is_empty() {
    let (g, calls) = overlay_fixture();
    let lg = layered(&g);
    let ov = overlay(&g, &calls, "nope", &lg);
    assert!(ov.modules.is_empty());
    assert!(ov.edges.is_empty());
}

#[test]
fn overlay_unions_multiple_matches() {
    let mut g = ArchGraph::new();
    g.ensure_module("x");
    g.ensure_module("y");
    let decls = vec![
        ("x".to_string(), fn_decl("build", &[])),
        ("y".to_string(), fn_decl("build", &[])),
    ];
    let calls = CallGraph::build(&decls);
    let lg = layered(&g);
    let ov = overlay(&g, &calls, "build", &lg);
    let mut names: Vec<&str> = ov.modules.iter().map(|&id| g.name(id)).collect();
    names.sort();
    assert_eq!(names, vec!["x", "y"]);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib comprehension::layered`
Expected: FAIL — `cannot find function 'overlay'`, `cannot find type 'FeatureOverlay'`.

- [ ] **Step 3: Write the implementation**

Add to `src/comprehension/layered.rs` (after `layered`). Add the import at the top of the file alongside the existing `use` lines:

```rust
use std::collections::BTreeSet;

use crate::comprehension::callgraph::CallGraph;
```

Then the types and function:

```rust
#[derive(Clone, Debug, Default)]
pub struct FeatureOverlay {
    /// Raw selector the user passed.
    pub selector: String,
    /// Modules the feature's call-reachable functions live in (sorted by id, deduped).
    pub modules: Vec<ModuleId>,
    /// Indices into `LayeredGraph.edges` whose endpoints are both in `modules`.
    pub edges: Vec<usize>,
}

/// Resolve `target` like `impact` (bare name OR `module::name`, union all
/// matches), collect the modules of every call-reachable function, and induce
/// the subgraph edges among them. Empty `modules` means nothing matched.
pub fn overlay(
    g: &ArchGraph,
    calls: &CallGraph,
    target: &str,
    lg: &LayeredGraph,
) -> FeatureOverlay {
    let mut starts: Vec<usize> = Vec::new();
    for (id, node) in calls.nodes().iter().enumerate() {
        if node.name == target || node.qualified() == target {
            starts.push(id);
        }
    }

    let mut modset: BTreeSet<ModuleId> = BTreeSet::new();
    for &s in &starts {
        for fid in calls.reachable(s) {
            if let Some(mid) = g.module_id(&calls.node(fid).module) {
                modset.insert(mid);
            }
        }
    }

    let edges: Vec<usize> = lg
        .edges
        .iter()
        .enumerate()
        .filter(|(_, e)| modset.contains(&e.from) && modset.contains(&e.to))
        .map(|(i, _)| i)
        .collect();

    FeatureOverlay {
        selector: target.to_string(),
        modules: modset.into_iter().collect(),
        edges,
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib comprehension::layered`
Expected: PASS — 6 tests total.

- [ ] **Step 5: Commit**

```bash
git add src/comprehension/layered.rs
git commit -m "feat(comprehension): feature-induced-subgraph overlay for layered graph"
```

---

### Task 3: Text render (`render_text`)

**Files:**
- Modify: `src/comprehension/layered.rs` (add `render_text`)

**Interfaces:**
- Consumes: `ArchGraph`, `LayeredGraph`, `Option<&FeatureOverlay>`.
- Produces: `pub fn render_text(g: &ArchGraph, lg: &LayeredGraph, overlay: Option<&FeatureOverlay>) -> String`
  - No-match (overlay present, `modules` empty) → `"no function matches '<selector>'\n"`.
  - Otherwise: header `layers (inward →)`, one line per column (`(none)` when empty, `*` marks overlay members), an `edges:` summary line, one `⚠` line per outward edge, and a `feature ·` trailer when overlay is present.

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module in `src/comprehension/layered.rs`:

```rust
#[test]
fn render_lists_columns_and_edge_summary() {
    let lg = layered(&fixture());
    let g = fixture();
    let out = render_text(&g, &lg, None);
    assert!(out.contains("layers (inward →)"));
    assert!(out.contains("[Adapter"));
    assert!(out.contains("adapters"));
    assert!(out.contains("edges:"));
    // domain -> adapters is an outward violation in the fixture.
    assert!(out.contains("⚠ domain → adapters"));
}

#[test]
fn render_marks_feature_members_and_trailer() {
    let (g, calls) = overlay_fixture();
    let lg = layered(&g);
    let ov = overlay(&g, &calls, "main", &lg);
    let out = render_text(&g, &lg, Some(&ov));
    assert!(out.contains("app*"));
    assert!(out.contains("feature · main"));
    assert!(out.contains("spans 3 modules"));
}

#[test]
fn render_no_match_notice() {
    let (g, calls) = overlay_fixture();
    let lg = layered(&g);
    let ov = overlay(&g, &calls, "nope", &lg);
    let out = render_text(&g, &lg, Some(&ov));
    assert_eq!(out, "no function matches 'nope'\n");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib comprehension::layered`
Expected: FAIL — `cannot find function 'render_text'`.

- [ ] **Step 3: Write the implementation**

Add to `src/comprehension/layered.rs`. (The `use std::fmt::Write;` import is already at the top from Task 1.)

```rust
/// Deterministic plain-text render: layer columns + edge summary + optional
/// feature overlay trailer.
pub fn render_text(g: &ArchGraph, lg: &LayeredGraph, overlay: Option<&FeatureOverlay>) -> String {
    if let Some(ov) = overlay {
        if ov.modules.is_empty() {
            return format!("no function matches '{}'\n", ov.selector);
        }
    }
    let members: BTreeSet<ModuleId> = overlay
        .map(|o| o.modules.iter().copied().collect())
        .unwrap_or_default();

    let mut out = String::new();
    let _ = writeln!(out, "layers (inward →)");
    for col in &lg.columns {
        let names: Vec<String> = col
            .modules
            .iter()
            .map(|&id| {
                let star = if members.contains(&id) { "*" } else { "" };
                format!("{}{}", g.name(id), star)
            })
            .collect();
        let body = if names.is_empty() {
            "(none)".to_string()
        } else {
            names.join("  ")
        };
        let _ = writeln!(out, "  [{:<11}] {}", format!("{:?}", col.layer), body);
    }

    let (mut inward, mut outward, mut lateral, mut unranked) = (0u32, 0u32, 0u32, 0u32);
    for e in &lg.edges {
        match e.dir {
            EdgeDir::Inward => inward += 1,
            EdgeDir::Outward => outward += 1,
            EdgeDir::Lateral => lateral += 1,
            EdgeDir::Unranked => unranked += 1,
        }
    }
    let _ = writeln!(
        out,
        "edges: {}  (inward {} · lateral {} · outward/violation {} · unranked {})",
        lg.edges.len(),
        inward,
        lateral,
        outward,
        unranked
    );
    for e in &lg.edges {
        if matches!(e.dir, EdgeDir::Outward) {
            let _ = writeln!(
                out,
                "  ⚠ {} → {}  (outward — dependency-rule violation)",
                g.name(e.from),
                g.name(e.to)
            );
        }
    }

    if let Some(ov) = overlay {
        let crossed: Vec<String> = lg
            .columns
            .iter()
            .filter(|c| c.modules.iter().any(|id| members.contains(id)))
            .map(|c| format!("{:?}", c.layer))
            .collect();
        let _ = writeln!(
            out,
            "feature · {} — spans {} modules, {} induced edges; crosses {}",
            ov.selector,
            ov.modules.len(),
            ov.edges.len(),
            crossed.join(" → ")
        );
    }
    out
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib comprehension::layered`
Expected: PASS — 9 tests total.

- [ ] **Step 5: Commit**

```bash
git add src/comprehension/layered.rs
git commit -m "feat(comprehension): deterministic text render for layered map"
```

---

### Task 4: Mermaid layered export (`render_layered`)

**Files:**
- Modify: `src/render/mermaid.rs` (add `render_layered` + tests)

**Interfaces:**
- Consumes: `graph::ArchGraph`, `comprehension::layered::{LayeredGraph, FeatureOverlay, EdgeDir}`, private `node_id`.
- Produces: `pub fn render_layered(g: &ArchGraph, lg: &LayeredGraph, overlay: Option<&FeatureOverlay>) -> String`
  - `flowchart LR`, one `subgraph <Layer>` per non-empty column, `-->|VIOLATION|` for outward edges, overlay members bolded via `classDef feat` + `class … feat;`.

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module in `src/render/mermaid.rs`:

```rust
#[test]
fn layered_render_has_subgraph_per_nonempty_layer() {
    use crate::comprehension::layered::layered;
    let mut g = ArchGraph::new();
    let a = g.ensure_module("adapters");
    let d = g.ensure_module("domain");
    g.add_edge(a, d);
    let lg = layered(&g);

    let out = render_layered(&g, &lg, None);
    assert!(out.starts_with("flowchart LR\n"));
    assert!(out.contains("subgraph Adapter"));
    assert!(out.contains("subgraph Domain"));
    // Empty Application column is omitted.
    assert!(!out.contains("subgraph Application"));
    assert!(out.contains("adapters --> domain"));
}

#[test]
fn layered_render_bolds_overlay_members() {
    use crate::comprehension::callgraph::CallGraph;
    use crate::comprehension::layered::{layered, overlay};
    use crate::lang::FnDecl;

    let mut g = ArchGraph::new();
    g.ensure_module("app");
    let lg = layered(&g);
    let decls = vec![(
        "app".to_string(),
        FnDecl {
            name: "run".into(),
            is_pub: true,
            is_test: false,
            is_main: false,
            calls: vec![],
        },
    )];
    let calls = CallGraph::build(&decls);
    let ov = overlay(&g, &calls, "run", &lg);

    let out = render_layered(&g, &lg, Some(&ov));
    assert!(out.contains("classDef feat"));
    assert!(out.contains("class app feat;"));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib render::mermaid`
Expected: FAIL — `cannot find function 'render_layered'`.

- [ ] **Step 3: Write the implementation**

Add to `src/render/mermaid.rs` (after `render`). Add the needed import near the top imports:

```rust
use crate::comprehension::layered::{EdgeDir, FeatureOverlay, LayeredGraph};
```

```rust
/// Render the layered graph as a mermaid `flowchart LR`: one subgraph per
/// non-empty layer column, outward edges flagged `|VIOLATION|`, and (when an
/// overlay is given) its modules bolded via a `feat` class. Export only.
pub fn render_layered(
    g: &ArchGraph,
    lg: &LayeredGraph,
    overlay: Option<&FeatureOverlay>,
) -> String {
    let members: HashSet<ModuleId> = overlay
        .map(|o| o.modules.iter().copied().collect())
        .unwrap_or_default();

    let mut out = String::from("flowchart LR\n");
    for col in &lg.columns {
        if col.modules.is_empty() {
            continue;
        }
        out.push_str(&format!("  subgraph {:?}\n", col.layer));
        for &id in &col.modules {
            out.push_str(&format!("    {}[\"{}\"]\n", node_id(g.name(id)), g.name(id)));
        }
        out.push_str("  end\n");
    }
    for e in &lg.edges {
        let arrow = if matches!(e.dir, EdgeDir::Outward) {
            "-->|VIOLATION|"
        } else {
            "-->"
        };
        out.push_str(&format!(
            "  {} {} {}\n",
            node_id(g.name(e.from)),
            arrow,
            node_id(g.name(e.to))
        ));
    }
    if !members.is_empty() {
        let mut ids: Vec<String> = members.iter().map(|&id| node_id(g.name(id))).collect();
        ids.sort();
        out.push_str("  classDef feat stroke-width:3px,font-weight:bold;\n");
        out.push_str(&format!("  class {} feat;\n", ids.join(",")));
    }
    out
}
```

Also add `ModuleId` to the graph import at the top of the file. Change:

```rust
use crate::graph::ArchGraph;
```
to:
```rust
use crate::graph::{ArchGraph, ModuleId};
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib render::mermaid`
Expected: PASS — existing 2 + new 2 = 4 tests.

- [ ] **Step 5: Commit**

```bash
git add src/render/mermaid.rs
git commit -m "feat(render): layered mermaid export with feature bolding"
```

---

### Task 5: App use-case + CLI verb + integration tests

**Files:**
- Modify: `src/app.rs` (add `pub fn map`)
- Modify: `src/main.rs` (add `Command::Map`, dispatch arm, `run_map`)
- Modify: `tests/cli.rs` (integration tests)

**Interfaces:**
- Consumes: everything from Tasks 1–4, `builder::build_graph`, `scan::scan_functions`, `CallGraph::build`.
- Produces:
  - `pub fn map(path: &std::path::Path, feature: Option<&str>, mermaid: bool) -> anyhow::Result<String>`
  - `Command::Map { path: PathBuf, feature: Option<String>, mermaid: bool }`
  - `fn run_map(path: &Path, feature: Option<&str>, mermaid: bool) -> Result<()>`

- [ ] **Step 1: Write the failing app test**

Add to the `tests` module in `src/app.rs` (mirror `analyze_self_emits_report_with_mermaid`):

```rust
#[test]
fn map_self_emits_layer_columns() {
    let out = map(std::path::Path::new("."), None, false).unwrap();
    assert!(out.contains("layers (inward →)"));
    assert!(out.contains("edges:"));
}

#[test]
fn map_mermaid_emits_flowchart() {
    let out = map(std::path::Path::new("."), None, true).unwrap();
    assert!(out.starts_with("flowchart LR\n"));
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --lib app::tests::map_self_emits_layer_columns`
Expected: FAIL — `cannot find function 'map'`.

- [ ] **Step 3: Implement `app::map`**

Add to `src/app.rs` (near `impact`):

```rust
/// Layered architecture map: layer columns + classified edges, with an optional
/// `--feature` induced-subgraph overlay. Mermaid is an export format only.
pub fn map(
    path: &std::path::Path,
    feature: Option<&str>,
    mermaid: bool,
) -> anyhow::Result<String> {
    let graph = crate::builder::build_graph(path)?;
    let lg = crate::comprehension::layered::layered(&graph);
    let overlay = match feature {
        Some(f) => {
            let decls = crate::comprehension::scan::scan_functions(path)?;
            let calls = crate::comprehension::callgraph::CallGraph::build(&decls);
            Some(crate::comprehension::layered::overlay(&graph, &calls, f, &lg))
        }
        None => None,
    };
    if mermaid {
        Ok(crate::render::mermaid::render_layered(&graph, &lg, overlay.as_ref()))
    } else {
        Ok(crate::comprehension::layered::render_text(
            &graph,
            &lg,
            overlay.as_ref(),
        ))
    }
}
```

- [ ] **Step 4: Run to verify app tests pass**

Run: `cargo test --lib app::tests::map_self_emits_layer_columns app::tests::map_mermaid_emits_flowchart`
Expected: PASS.

- [ ] **Step 5: Wire the CLI verb**

In `src/main.rs`, add a variant to `enum Command` (after the `Impact { .. }` variant):

```rust
    /// Layered architecture map: layer columns + feature overlay (no LLM)
    Map {
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Highlight a feature's induced subgraph (function name or `module::name`)
        #[arg(long)]
        feature: Option<String>,
        /// Emit mermaid export instead of text
        #[arg(long)]
        mermaid: bool,
    },
```

Add the dispatch arm in `fn main` (after the `Command::Impact { .. } => …` arm):

```rust
        Command::Map {
            path,
            feature,
            mermaid,
        } => run_map(&path, feature.as_deref(), mermaid),
```

Add the runner (near `run_impact`):

```rust
fn run_map(path: &Path, feature: Option<&str>, mermaid: bool) -> Result<()> {
    println!("{}", circuit::app::map(path, feature, mermaid)?);
    Ok(())
}
```

- [ ] **Step 6: Write the failing CLI integration tests**

Add to `tests/cli.rs` (mirror `impact_reports_dependents`; use a tempdir fixture with a `src/` tree):

```rust
#[test]
fn map_reports_layers() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("src");
    std::fs::create_dir_all(src.join("app")).unwrap();
    std::fs::write(src.join("main.rs"), "use crate::app::run;\nfn main() { run(); }").unwrap();
    std::fs::write(src.join("app/mod.rs"), "pub fn run() {}").unwrap();

    Command::cargo_bin("circuit")
        .unwrap()
        .arg("map")
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("layers (inward →)"))
        .stdout(predicate::str::contains("[Application"));
}

#[test]
fn map_feature_highlights_path() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("src");
    std::fs::create_dir_all(src.join("app")).unwrap();
    std::fs::write(src.join("main.rs"), "use crate::app::run;\nfn main() { run(); }").unwrap();
    std::fs::write(src.join("app/mod.rs"), "pub fn run() {}").unwrap();

    Command::cargo_bin("circuit")
        .unwrap()
        .arg("map")
        .arg(dir.path())
        .arg("--feature")
        .arg("main")
        .assert()
        .success()
        .stdout(predicate::str::contains("feature · main"));
}

#[test]
fn map_mermaid_exports_flowchart() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("main.rs"), "fn main() {}").unwrap();

    Command::cargo_bin("circuit")
        .unwrap()
        .arg("map")
        .arg(dir.path())
        .arg("--mermaid")
        .assert()
        .success()
        .stdout(predicate::str::contains("flowchart LR"));
}
```

- [ ] **Step 7: Run to verify CLI tests fail then pass**

Run: `cargo test --test cli map_`
Expected: initially FAIL if run before Step 5 (`unrecognized subcommand 'map'`); after Steps 5–6, PASS — 3 tests.

- [ ] **Step 8: Full gate**

Run:
```bash
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --all
```
Expected: all green; clippy clean; fmt produces no changes (or stages formatting — review before commit).

- [ ] **Step 9: Manual smoke on Circuit's own repo**

Run:
```bash
cargo run -- map .
cargo run -- map . --feature run_map
cargo run -- map . --mermaid
```
Expected: layer columns render; `--feature` shows the `feature ·` trailer; `--mermaid` emits `flowchart LR` with subgraphs.

- [ ] **Step 10: Commit**

```bash
git add src/app.rs src/main.rs tests/cli.rs
git commit -m "feat(comprehension): wire circuit map verb (app + CLI + integration tests)"
```

---

## Notes for the executor

- **Determinism check:** every rendered collection must be sorted before output. `g.edges()` is already sorted; columns use a fixed order + name sort; overlay uses `BTreeSet`. Do not introduce `HashMap` iteration into any rendered string.
- **Do not modify** `analyze`, `layer_of`, or `render::mermaid::render` (the existing `graph TD` renderer). The layered renderer is additive.
- **Clippy is run once at the Task 5 gate** (Step 8), consistent with the impact slice. If a per-task clippy surfaces the unused `use std::fmt::Write;` in Task 1, it resolves itself in Task 3 — don't churn it.
