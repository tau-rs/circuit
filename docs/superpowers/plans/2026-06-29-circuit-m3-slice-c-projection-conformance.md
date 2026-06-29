# M3 Slice C — Projection-Conformance Indicator — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Diff the code's derived dependency graph against the approved system projection and report broken planned boundaries (red), additive detail (silent), and uncovered components (Unknown, never false-green).

**Architecture:** Pure hexagonal extension. One additive schema field (`Component.module`), one new pure indicator module (`indicators/conformance.rs`) mirroring `indicators/dependency_rule.rs`, one port-generic app use-case over the existing `ProjectionRepo` + M1's `builder::build_graph`, and one CLI verb. The check is a pure function of two in-memory values; all IO stays at the edges.

**Tech Stack:** Rust, serde + toml, clap, anyhow internally / `ModelError` (thiserror) at the store boundary, assert_cmd + predicates + tempfile (integration tests).

## Global Constraints

- This slice is **system-level only**: the spec's `SystemProjection` vs the whole-repo derived graph. No slice/per-DAG-node conformance.
- Uses only the projection's `component` + `edge`. **No** context/relationship/contract semantics.
- The design-name ↔ code-module join is **declared** via `Component.module` (defaults to `name`), never guessed.
- Reuse the existing `Health` ladder (`crate::cockpit::health::Health`) for the verdict. **Do NOT** modify `SessionHealth`/`rollup()` or the cockpit roll-up in this slice.
- "Never fake a verdict": a declared component with no matching derived module is **uncovered → Unknown**, never Sound.
- Exit-code policy for the CLI: a **violation** fails the command (non-zero exit, like `dag check`); **uncovered** prints a warning but exits 0.
- App layer is port-generic, `anyhow` internally, no IO/printing — mirror `projection_show`. The pure check never panics and returns deterministically sorted output (mirror `dependency_rule::violations`).
- Out of scope (do NOT add): layer-mismatch check, projected-layer-fueled dependency rule, auto-fix, mermaid overlay, `--json`.
- Commit messages: conventional, imperative, scoped (`feat(m3): …`).

**Spec:** `docs/superpowers/specs/2026-06-29-circuit-m3-slice-c-projection-conformance-design.md`

**Codebase facts the implementer needs (verified):**
- `crate::graph::ArchGraph`: `edges() -> Vec<(ModuleId, ModuleId)>`, `module_id(&str) -> Option<ModuleId>`, `name(ModuleId) -> &str`. `ModuleId = usize`.
- `crate::builder::build_graph(path: &Path) -> anyhow::Result<ArchGraph>` walks `<path>/src` for `.rs` files; module name = top-level path segment.
- `crate::cockpit::health::Health` = `enum { Sound, Warn, Critical, Unknown }` (derives `Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord`).
- `crate::model::projection::{SystemProjection, Component}` exist (Slice A); `Component { name: String, layer: Layer }` today.
- `crate::ports::ProjectionRepo::load_projection(&self, spec) -> Result<SystemProjection, _>`; `Workspace` implements it.
- `app.rs` already has `use std::fmt::Write as _;` and a `require_initialized(settings)` helper; `main.rs` has `Workspace`, `require_initialized(&ws)`, `Path`, `Subcommand`, and a `circuit::app::*` calling convention (see `run_spec`, `run_projection`, `run_dag`'s `DagCommand::Check` `std::process::exit(1)`).

---

### Task 1: Add `Component.module` join field

**Files:**
- Modify: `src/model/projection.rs` (the `Component` struct + a method + tests)
- Test: `src/model/projection.rs` (inline)

**Interfaces:**
- Produces: `Component.module: Option<String>` (`#[serde(default)]`); `Component::effective_module(&self) -> &str` returning `module` or falling back to `name`.

- [ ] **Step 1: Write the failing test**

Add to the `mod tests` block in `src/model/projection.rs`:

```rust
#[test]
fn effective_module_uses_module_then_falls_back_to_name() {
    let mapped = Component { name: "billing".into(), layer: Layer::Domain, module: Some("model".into()) };
    assert_eq!(mapped.effective_module(), "model");

    let unmapped = Component { name: "cart".into(), layer: Layer::Domain, module: None };
    assert_eq!(unmapped.effective_module(), "cart");
}

#[test]
fn component_without_module_key_parses_and_defaults_to_none() {
    // A Slice A projection has no `module` key on its components.
    let text = r#"
        schema_version = 1
        spec = "checkout"
        [[component]]
        name = "billing"
        layer = "domain"
    "#;
    let p: SystemProjection = toml::from_str(text).unwrap();
    assert_eq!(p.component[0].module, None);
    assert_eq!(p.component[0].effective_module(), "billing");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib model::projection`
Expected: FAIL to compile — `Component` has no `module` field / no `effective_module`.

- [ ] **Step 3: Add the field and method**

In `src/model/projection.rs`, change the `Component` struct from:

```rust
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Component {
    pub name: String,
    pub layer: Layer,
}
```

to:

```rust
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Component {
    pub name: String,
    pub layer: Layer,
    /// Which derived code module realizes this component (top-level module name,
    /// e.g. "model"). `None` ⇒ join on `name`. The design name and the code
    /// module live in different namespaces, so the link must be declared.
    #[serde(default)]
    pub module: Option<String>,
}

impl Component {
    /// The derived-graph module name this component joins to.
    pub fn effective_module(&self) -> &str {
        self.module.as_deref().unwrap_or(&self.name)
    }
}
```

- [ ] **Step 4: Fix the existing `populated()` test helper**

The existing `populated()` test constructs `Component { name, layer }` literals which now miss the `module` field. In `src/model/projection.rs`, update the two `Component { ... }` literals inside the `populated()` helper to add `module: None`:

```rust
            component: vec![
                Component { name: "billing".into(), layer: Layer::Domain, module: None },
                Component { name: "gh-adapter".into(), layer: Layer::Adapter, module: None },
            ],
```

(Leave the rest of `populated()` unchanged.)

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --lib model::projection`
Expected: PASS (existing round-trip tests + the 2 new tests). If a `Component { ... }` literal elsewhere fails to compile, add `module: None` to it.

- [ ] **Step 6: Commit**

```bash
git add src/model/projection.rs
git commit -m "feat(m3): add Component.module join field for conformance"
```

---

### Task 2: Conformance indicator (`check` + `Conformance` + `health`)

**Files:**
- Create: `src/indicators/conformance.rs`
- Modify: `src/indicators/mod.rs` (add `pub mod conformance;`)
- Test: `src/indicators/conformance.rs` (inline)

**Interfaces:**
- Consumes: `crate::graph::ArchGraph`, `crate::model::projection::{SystemProjection, Component, IntendedEdge}`, `crate::cockpit::health::Health`, `Component::effective_module` (Task 1).
- Produces:
  - `struct BrokenEdge { from: String, to: String, from_module: String, to_module: String }`
  - `struct Conformance { violations: Vec<BrokenEdge>, uncovered: Vec<String> }` (derives `Default`)
  - `fn check(graph: &ArchGraph, proj: &SystemProjection) -> Conformance`
  - `Conformance::health(&self) -> Health`

- [ ] **Step 1: Write the failing tests**

Create `src/indicators/conformance.rs`:

```rust
use std::collections::{BTreeMap, BTreeSet};

use crate::cockpit::health::Health;
use crate::graph::ArchGraph;
use crate::model::projection::{Component, SystemProjection};

/// A derived edge between two declared components that the projection's `edge`
/// allowlist does not sanction — a broken planned boundary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BrokenEdge {
    /// Component names (design vocabulary).
    pub from: String,
    pub to: String,
    /// The derived modules they map to (for the message).
    pub from_module: String,
    pub to_module: String,
}

/// Result of diffing reality (graph) against intent (projection).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Conformance {
    pub violations: Vec<BrokenEdge>,
    /// Declared component names whose `effective_module` is not a node in the graph.
    pub uncovered: Vec<String>,
}

impl Conformance {
    /// Verdict on the existing ladder. NOT wired into SessionHealth in this slice.
    pub fn health(&self) -> Health {
        if !self.violations.is_empty() {
            Health::Critical
        } else if !self.uncovered.is_empty() {
            Health::Unknown
        } else {
            Health::Sound
        }
    }
}

/// Diff the derived graph against the system projection. See the design's
/// "Rules (precise)" section. Output vectors are sorted for determinism.
pub fn check(graph: &ArchGraph, proj: &SystemProjection) -> Conformance {
    // component name -> effective module
    let module_of: BTreeMap<&str, &str> = proj
        .component
        .iter()
        .map(|c| (c.name.as_str(), c.effective_module()))
        .collect();

    // the set of modules under design control
    let declared_modules: BTreeSet<&str> = module_of.values().copied().collect();

    // allowed module pairs, translated from projection edges (which name components).
    // An edge naming an unknown component is ignored (authoring slip, not a code violation).
    let mut allowed: BTreeSet<(&str, &str)> = BTreeSet::new();
    for e in &proj.edge {
        if let (Some(&fm), Some(&tm)) =
            (module_of.get(e.from.as_str()), module_of.get(e.to.as_str()))
        {
            allowed.insert((fm, tm));
        }
    }

    // module -> a single component name for messages. When two components map to
    // the same module, pick the first by sorted component name (deterministic).
    let mut comps_sorted: Vec<&Component> = proj.component.iter().collect();
    comps_sorted.sort_by(|a, b| a.name.cmp(&b.name));
    let mut component_of_module: BTreeMap<&str, &str> = BTreeMap::new();
    for c in &comps_sorted {
        component_of_module
            .entry(c.effective_module())
            .or_insert(c.name.as_str());
    }

    // violations: derived edges between two declared modules not in the allowlist.
    let mut violations = Vec::new();
    for (f, t) in graph.edges() {
        let fm = graph.name(f);
        let tm = graph.name(t);
        if declared_modules.contains(fm)
            && declared_modules.contains(tm)
            && !allowed.contains(&(fm, tm))
        {
            violations.push(BrokenEdge {
                from: component_of_module[fm].to_string(),
                to: component_of_module[tm].to_string(),
                from_module: fm.to_string(),
                to_module: tm.to_string(),
            });
        }
    }
    violations.sort_by(|a, b| (&a.from_module, &a.to_module).cmp(&(&b.from_module, &b.to_module)));

    // uncovered: declared components whose module is absent from the graph.
    let mut uncovered: Vec<String> = proj
        .component
        .iter()
        .filter(|c| graph.module_id(c.effective_module()).is_none())
        .map(|c| c.name.clone())
        .collect();
    uncovered.sort();
    uncovered.dedup();

    Conformance { violations, uncovered }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layer::Layer;
    use crate::model::projection::IntendedEdge;

    // Build a projection with the given (name, module) components and (from,to) component edges.
    fn proj(components: &[(&str, &str)], edges: &[(&str, &str)]) -> SystemProjection {
        let mut p = SystemProjection::new("checkout");
        p.component = components
            .iter()
            .map(|(n, m)| Component {
                name: (*n).into(),
                layer: Layer::Domain,
                module: Some((*m).into()),
            })
            .collect();
        p.edge = edges
            .iter()
            .map(|(f, t)| IntendedEdge { from: (*f).into(), to: (*t).into() })
            .collect();
        p
    }

    // Build a graph with the given module->module edges.
    fn graph(edges: &[(&str, &str)]) -> ArchGraph {
        let mut g = ArchGraph::new();
        for (f, t) in edges {
            let fi = g.ensure_module(f);
            let ti = g.ensure_module(t);
            g.add_edge(fi, ti);
        }
        g
    }

    #[test]
    fn allowed_edge_is_not_a_violation() {
        let p = proj(&[("billing", "model"), ("ghx", "adapters")], &[("ghx", "billing")]);
        let g = graph(&[("adapters", "model")]); // ghx->billing == adapters->model, allowed
        let c = check(&g, &p);
        assert!(c.violations.is_empty(), "got: {:?}", c.violations);
        assert!(c.uncovered.is_empty());
        assert_eq!(c.health(), Health::Sound);
    }

    #[test]
    fn forbidden_edge_between_declared_components_is_a_violation() {
        let p = proj(&[("billing", "model"), ("ghx", "adapters")], &[("ghx", "billing")]);
        let g = graph(&[("model", "adapters")]); // billing->ghx, NOT allowed
        let c = check(&g, &p);
        assert_eq!(c.violations.len(), 1, "got: {:?}", c.violations);
        let v = &c.violations[0];
        assert_eq!(v.from, "billing");
        assert_eq!(v.to, "ghx");
        assert_eq!(v.from_module, "model");
        assert_eq!(v.to_module, "adapters");
        assert_eq!(c.health(), Health::Critical);
    }

    #[test]
    fn edge_touching_an_undeclared_module_is_silent() {
        let p = proj(&[("billing", "model")], &[]);
        let g = graph(&[("model", "flow")]); // flow undeclared
        let c = check(&g, &p);
        assert!(c.violations.is_empty(), "got: {:?}", c.violations);
    }

    #[test]
    fn declared_component_with_no_module_is_uncovered() {
        let p = proj(&[("billing", "model"), ("cart", "cart")], &[]);
        let g = graph(&[]); // no modules at all -> both uncovered
        let c = check(&g, &p);
        assert_eq!(c.uncovered, vec!["billing".to_string(), "cart".to_string()]);
        assert_eq!(c.health(), Health::Unknown);
    }

    #[test]
    fn projected_edge_absent_from_code_is_silent() {
        let p = proj(&[("billing", "model"), ("ghx", "adapters")], &[("ghx", "billing")]);
        let g = graph(&[("adapters", "model")]); // only the allowed edge exists
        let c = check(&g, &p);
        assert!(c.violations.is_empty());
        // billing(model) and ghx(adapters) are both present as graph nodes -> covered
        assert!(c.uncovered.is_empty());
        assert_eq!(c.health(), Health::Sound);
    }
}
```

- [ ] **Step 2: Register the module**

In `src/indicators/mod.rs`, add (keep alphabetical):

```rust
pub mod conformance;
pub mod cycles;
pub mod dependency_rule;
```

- [ ] **Step 3: Run tests to verify they pass**

Run: `cargo test --lib indicators::conformance`
Expected: PASS (5 tests). Compilation failure first if the module wasn't registered — fix Step 2.

- [ ] **Step 4: Commit**

```bash
git add src/indicators/conformance.rs src/indicators/mod.rs
git commit -m "feat(m3): add projection-conformance indicator (check + verdict)"
```

---

### Task 3: App-layer `conformance` use-case + renderer

**Files:**
- Modify: `src/app.rs` (imports, `conformance` fn, `render_conformance` fn, tests)
- Test: `src/app.rs` (inline)

**Interfaces:**
- Consumes: `SettingsRepo`, `ProjectionRepo`, `crate::builder::build_graph`, `crate::indicators::conformance::{check, Conformance}`.
- Produces:
  - `pub fn conformance<S: SettingsRepo, P: ProjectionRepo>(settings: &S, projections: &P, spec: &str, path: &Path) -> anyhow::Result<Conformance>`
  - `pub fn render_conformance(c: &Conformance) -> String`

- [ ] **Step 1: Write the failing tests**

Add to `mod tests` in `src/app.rs` (note: `MemStore` cannot supply a graph, so these tests build a real temp repo on disk and use a `Workspace`, mirroring the `build_graph` temp-repo tests):

```rust
#[test]
fn conformance_reports_a_broken_edge_and_renders_it() {
    use crate::adapters::store::Workspace;
    use crate::model::projection::{Component, SystemProjection};
    use crate::layer::Layer;

    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    // a tiny repo: module `model` depends on module `adapters`
    let src = root.join("src");
    std::fs::create_dir_all(src.join("model")).unwrap();
    std::fs::create_dir_all(src.join("adapters")).unwrap();
    std::fs::write(src.join("model/x.rs"), "use crate::adapters::Thing;").unwrap();
    std::fs::write(src.join("adapters/y.rs"), "pub struct Thing;").unwrap();

    let ws = Workspace::new(root);
    ws.save_config(&crate::model::config::Config::default()).unwrap();

    // projection: billing(model), ghx(adapters); allowed ghx->billing (adapters->model).
    // code has model->adapters, which is NOT allowed -> 1 violation.
    let mut p = SystemProjection::new("checkout");
    p.component = vec![
        Component { name: "billing".into(), layer: Layer::Domain, module: Some("model".into()) },
        Component { name: "ghx".into(), layer: Layer::Adapter, module: Some("adapters".into()) },
    ];
    p.edge = vec![crate::model::projection::IntendedEdge { from: "ghx".into(), to: "billing".into() }];
    ws.save_projection(&p).unwrap();

    let c = conformance(&ws, &ws, "checkout", root).unwrap();
    assert_eq!(c.violations.len(), 1, "got: {:?}", c.violations);

    let out = render_conformance(&c);
    assert!(out.contains("Violations (1)"), "got: {out}");
    assert!(out.contains("billing"), "got: {out}");
}

#[test]
fn conformance_bails_when_projection_absent() {
    use crate::adapters::store::Workspace;
    let dir = tempfile::tempdir().unwrap();
    let ws = Workspace::new(dir.path());
    ws.save_config(&crate::model::config::Config::default()).unwrap();
    let err = conformance(&ws, &ws, "checkout", dir.path()).unwrap_err();
    assert!(err.to_string().contains("no projection for checkout"), "got: {err}");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib conformance_`
Expected: FAIL to compile — `conformance` / `render_conformance` not defined.

- [ ] **Step 3: Add imports and functions**

In `src/app.rs`, add near the other `use crate::...` imports:

```rust
use crate::indicators::conformance::{check as check_conformance, Conformance};
```

Then add the two functions (place after `projection_show`):

```rust
/// Compute system-projection conformance for a spec against a repo worktree.
pub fn conformance<S: SettingsRepo, P: ProjectionRepo>(
    settings: &S,
    projections: &P,
    spec: &str,
    path: &Path,
) -> anyhow::Result<Conformance> {
    require_initialized(settings)?;
    let proj = projections.load_projection(spec).with_context(|| {
        format!("no projection for {spec} — run `circuit projection init {spec}`")
    })?;
    let graph = crate::builder::build_graph(path)?;
    Ok(check_conformance(&graph, &proj))
}

/// Plain-text report of a conformance result. Empty sections render `(none)`.
pub fn render_conformance(c: &Conformance) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "Projection conformance: {:?}", c.health());

    let _ = writeln!(out, "Violations ({}):", c.violations.len());
    if c.violations.is_empty() {
        let _ = writeln!(out, "  (none)");
    }
    for v in &c.violations {
        let _ = writeln!(
            out,
            "  - {} [{}] -> {} [{}]  (not in allowed edges)",
            v.from, v.from_module, v.to, v.to_module
        );
    }

    let _ = writeln!(out, "Uncovered ({}):", c.uncovered.len());
    if c.uncovered.is_empty() {
        let _ = writeln!(out, "  (none)");
    }
    for u in &c.uncovered {
        let _ = writeln!(out, "  - {}", u);
    }

    out
}
```

(`Conformance::health()` is in scope via the `Conformance` import; `use std::fmt::Write as _;` is already imported at the top of `app.rs`.)

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib conformance_`
Expected: PASS (2 tests).

- [ ] **Step 5: Commit**

```bash
git add src/app.rs
git commit -m "feat(m3): add conformance use-case + report renderer"
```

---

### Task 4: CLI glue — `circuit conformance`

**Files:**
- Modify: `src/main.rs` (add `Command::Conformance` variant, dispatch arm, `run_conformance` fn)
- Test: covered by Task 5 (integration).

**Interfaces:**
- Consumes: `circuit::app::conformance`, `circuit::app::render_conformance`.

- [ ] **Step 1: Add the `Command::Conformance` variant**

In `src/main.rs`, inside `enum Command` (place after the `Projection` variant):

```rust
    /// Check code against a spec's approved system projection
    Conformance {
        /// Spec id whose projection to check against
        spec: String,
        #[arg(long, default_value = ".")]
        path: PathBuf,
    },
```

- [ ] **Step 2: Add the dispatch arm**

In `src/main.rs`, in the `match` inside `main` (next to `Command::Projection { command } => run_projection(command),`):

```rust
        Command::Conformance { spec, path } => run_conformance(&spec, &path),
```

- [ ] **Step 3: Add the `run_conformance` glue**

In `src/main.rs`, after `run_projection`:

```rust
fn run_conformance(spec: &str, path: &Path) -> Result<()> {
    let ws = Workspace::new(path);
    require_initialized(&ws)?;
    let c = circuit::app::conformance(&ws, &ws, spec, path)?;
    print!("{}", circuit::app::render_conformance(&c));
    // A broken contract fails the command (gates CI); uncovered is only a warning.
    if !c.violations.is_empty() {
        std::process::exit(1);
    }
    Ok(())
}
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo build`
Expected: builds clean, no unused-variant warnings.

- [ ] **Step 5: Commit**

```bash
git add src/main.rs
git commit -m "feat(m3): wire circuit conformance CLI verb"
```

---

### Task 5: Integration test — exit-criteria walk

**Files:**
- Create: `tests/conformance.rs`

**Interfaces:**
- Consumes: the `circuit` binary (via `assert_cmd`).

- [ ] **Step 1: Write the integration test**

Create `tests/conformance.rs`:

```rust
use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;

/// Author a tiny repo whose `model` module depends on `adapters`, plus a
/// projection mapping billing->model and ghx->adapters with a single allowed edge.
/// `allowed_from`/`allowed_to` are component names for the one projection edge.
fn scaffold(path: &std::path::Path, allowed_from: &str, allowed_to: &str) {
    let src = path.join("src");
    fs::create_dir_all(src.join("model")).unwrap();
    fs::create_dir_all(src.join("adapters")).unwrap();
    // edge: model -> adapters
    fs::write(src.join("model/x.rs"), "use crate::adapters::Thing;").unwrap();
    fs::write(src.join("adapters/y.rs"), "pub struct Thing;").unwrap();

    let run = |args: &[&str]| {
        Command::cargo_bin("circuit").unwrap().args(args).current_dir(path).assert().success();
    };
    run(&["init"]);
    run(&["spec", "new", "checkout", "--title", "Checkout", "--intent", "Pay."]);
    run(&["projection", "init", "checkout"]);

    // overwrite the skeleton with an authored projection
    let toml = format!(
        r#"schema_version = 1
spec = "checkout"

[[component]]
name = "billing"
layer = "domain"
module = "model"

[[component]]
name = "ghx"
layer = "adapter"
module = "adapters"

[[edge]]
from = "{allowed_from}"
to = "{allowed_to}"
"#
    );
    fs::write(path.join(".circuit/projections/checkout.toml"), toml).unwrap();
}

#[test]
fn conformance_passes_when_edge_is_allowed() {
    let dir = tempfile::tempdir().unwrap();
    // allow billing->ghx == model->adapters, which is exactly what the code has
    scaffold(dir.path(), "billing", "ghx");

    Command::cargo_bin("circuit")
        .unwrap()
        .args(["conformance", "checkout"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Violations (0)"))
        .stdout(predicate::str::contains("Sound"));
}

#[test]
fn conformance_fails_on_a_broken_edge() {
    let dir = tempfile::tempdir().unwrap();
    // allow ghx->billing == adapters->model; code has model->adapters -> violation
    scaffold(dir.path(), "ghx", "billing");

    Command::cargo_bin("circuit")
        .unwrap()
        .args(["conformance", "checkout"])
        .current_dir(dir.path())
        .assert()
        .failure()
        .stdout(predicate::str::contains("Violations (1)"))
        .stdout(predicate::str::contains("billing"));
}
```

- [ ] **Step 2: Run the integration test**

Run: `cargo test --test conformance`
Expected: PASS (2 tests).

- [ ] **Step 3: Run the full gate**

Run: `cargo test && cargo fmt --check && cargo clippy --all-targets -- -D warnings`
Expected: all green.

- [ ] **Step 4: Commit**

```bash
git add tests/conformance.rs
git commit -m "test(m3): exit-criteria walk for conformance verb"
```

---

## Self-Review

**Spec coverage:**
- `Component.module` join + `effective_module` → Task 1. ✔
- `check()` rules (allowed/forbidden/silent/uncovered, determinism) + `Conformance`/`BrokenEdge`/`health()` → Task 2. ✔
- App use-case over `ProjectionRepo` + `build_graph`; render helper → Task 3. ✔
- CLI verb with violation→non-zero-exit, uncovered→warning/exit-0 → Task 4. ✔
- Exit-criteria walk (pass + broken-edge fail) → Task 5. ✔
- Non-goals (no SessionHealth wiring, no context/contract, no slice-level, no layer-mismatch) → none introduced. ✔
- "Never fake a verdict" (uncovered→Unknown) → Task 2 `health()` + test. ✔

**Placeholder scan:** No TBD/TODO; every code step shows full code. ✔

**Type consistency:** `Conformance { violations: Vec<BrokenEdge>, uncovered: Vec<String> }`, `BrokenEdge { from, to, from_module, to_module }`, `check(&ArchGraph, &SystemProjection) -> Conformance`, `Conformance::health() -> Health`, `Component.module: Option<String>`/`effective_module()`, `conformance(&S,&P,&str,&Path)`, `render_conformance(&Conformance)` are consistent across Tasks 1–5. The app import aliases `check as check_conformance` to avoid shadowing; call sites use `check_conformance`. ✔
