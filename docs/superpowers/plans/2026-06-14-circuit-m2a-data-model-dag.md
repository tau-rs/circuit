# Circuit M2a — Authored Data Model & DAG — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A serde-backed `.circuit/` authored data model (config, glossary, spec, DAG nodes) with round-trip persistence, plus CLI commands to `init` a project, create a spec session, author a task DAG, and validate it (acyclic / refs resolve / unique branches) — reusing M1's Tarjan cycle detector.

**Architecture:** Hexagonal, extending M1. Pure data types (`src/model/*`) carry serde schemas with no IO. A persistence boundary (`src/model/store.rs`, the `Workspace`) does filesystem IO. Pure DAG validation (`src/dag/`) builds an `ArchGraph` from nodes and reuses `indicators::cycles::find_cycles`. The CLI (`src/main.rs`) wires commands over these. Everything authored lives under `.circuit/`; nothing derived is stored.

**Tech Stack:** Rust 2021, `serde` (derive) + `toml` (0.8) for the data model, `thiserror` at the persistence boundary, `clap` (already present) for the CLI; dev: `assert_cmd`, `predicates`, `tempfile` (already present).

**Parallelization (task DAG):** After Task 1, Tasks 2–5 (the four schema types) are independent and parallel-eligible. Task 6 (Workspace) depends on 2–5. Task 7 (DAG validation) depends on Task 5 + M1's `graph`/`indicators::cycles`. Tasks 8–11 (CLI) depend on Task 6 (and Task 7 for Task 11) and are sequential because they share `src/main.rs`.

---

## File structure

| File | Responsibility |
|---|---|
| `Cargo.toml` | Add `serde`, `toml` deps |
| `src/lib.rs` | Add `pub mod model;` and `pub mod dag;` |
| `src/model/mod.rs` | `ModelError`; generic `load_toml`/`save_toml` helpers; submodule decls |
| `src/model/config.rs` | `Config`, `Tier`, `Capabilities` (serde + `Default`) |
| `src/model/glossary.rs` | `Glossary`, `Term` (serde + `Default`) |
| `src/model/spec.rs` | `SpecRecord` (serde) |
| `src/model/node.rs` | `DagNode` (serde) |
| `src/model/store.rs` | `Workspace`: `.circuit/` paths + typed load/save/list (filesystem IO) |
| `src/dag/mod.rs` | `DagError`; pure `validate(&[DagNode])` reusing `indicators::cycles` |
| `src/main.rs` | `clap`: `init`, `spec new`, `dag add-node`, `dag link`, `dag check` (keeps `analyze`) |
| `tests/data_model.rs` | End-to-end CLI integration tests for the M2a commands |

---

## Task 1: Add dependencies and module declarations

**Files:**
- Modify: `Cargo.toml`
- Modify: `src/lib.rs`
- Create: `src/model/mod.rs`, `src/model/config.rs`, `src/model/glossary.rs`, `src/model/spec.rs`, `src/model/node.rs`, `src/model/store.rs`
- Create: `src/dag/mod.rs`

- [ ] **Step 1: Add the data-model dependencies**

In `Cargo.toml`, under `[dependencies]`, add `serde` and `toml` after the existing `anyhow` line:

```toml
serde = { version = "1", features = ["derive"] }
toml = "0.8"
```

- [ ] **Step 2: Declare the new modules in the library root**

In `src/lib.rs`, add two module declarations alphabetically among the existing ones so the block reads:

```rust
pub mod builder;
pub mod dag;
pub mod graph;
pub mod indicators;
pub mod lang;
pub mod layer;
pub mod model;
pub mod render;
```

- [ ] **Step 3: Create stub files so the crate compiles**

Create `src/model/mod.rs`:

```rust
pub mod config;
pub mod glossary;
pub mod node;
pub mod spec;
pub mod store;
```

Create empty stubs (no content needed yet) so the `mod.rs` declarations resolve:
- `src/model/config.rs`
- `src/model/glossary.rs`
- `src/model/node.rs`
- `src/model/spec.rs`
- `src/model/store.rs`

Create `src/dag/mod.rs` (empty stub).

- [ ] **Step 4: Verify the crate still compiles**

Run: `cargo build`
Expected: success (downloads `serde`/`toml` on first run). Empty modules are valid Rust.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock src/lib.rs src/model/ src/dag/
git commit -m "chore: add serde/toml deps and model/dag module scaffolding (M2a)"
```

---

## Task 2: Config schema  _(parallel-eligible with Tasks 3–5)_

**Files:**
- Modify: `src/model/config.rs`

- [ ] **Step 1: Write the schema and its tests**

Replace `src/model/config.rs` with:

```rust
use serde::{Deserialize, Serialize};

/// Enforcement-rigor tier. Authored now; the rigor consumer is M3.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tier {
    Full,
    Light,
    Cli,
}

/// Project capabilities. Authored now; gating consumers (e.g. UI-match) are M3.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capabilities {
    #[serde(default)]
    pub has_ui: bool,
}

/// `.circuit/config.toml`. The `base_branch` field is the one live M2 consumer
/// (stage derivation needs it for merge-base / rev-list).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    pub schema_version: u32,
    pub tier: Tier,
    pub base_branch: String,
    #[serde(default)]
    pub capabilities: Capabilities,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            schema_version: 1,
            tier: Tier::Full,
            base_branch: "main".to_string(),
            capabilities: Capabilities::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_round_trips_through_toml() {
        let c = Config::default();
        let text = toml::to_string_pretty(&c).unwrap();
        let parsed: Config = toml::from_str(&text).unwrap();
        assert_eq!(parsed, c);
    }

    #[test]
    fn parses_a_hand_authored_file() {
        let text = r#"
            schema_version = 1
            tier = "light"
            base_branch = "develop"

            [capabilities]
            has_ui = true
        "#;
        let c: Config = toml::from_str(text).unwrap();
        assert_eq!(c.tier, Tier::Light);
        assert_eq!(c.base_branch, "develop");
        assert!(c.capabilities.has_ui);
    }

    #[test]
    fn capabilities_default_when_omitted() {
        let text = "schema_version = 1\ntier = \"full\"\nbase_branch = \"main\"\n";
        let c: Config = toml::from_str(text).unwrap();
        assert!(!c.capabilities.has_ui);
    }
}
```

- [ ] **Step 2: Run the tests**

Run: `cargo test --lib model::config`
Expected: PASS (3 tests).

- [ ] **Step 3: Commit**

```bash
git add src/model/config.rs
git commit -m "feat: config schema (tier, base_branch, capabilities)"
```

---

## Task 3: Glossary schema  _(parallel-eligible with Tasks 2, 4, 5)_

**Files:**
- Modify: `src/model/glossary.rs`

- [ ] **Step 1: Write the schema and its tests**

Replace `src/model/glossary.rs` with:

```rust
use serde::{Deserialize, Serialize};

/// A single ubiquitous-language term.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Term {
    pub name: String,
    pub definition: String,
}

/// `.circuit/glossary.toml`. Authored now; the naming-indicator consumer is M3.
/// On disk each term is a `[[term]]` array-of-tables entry.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Glossary {
    pub schema_version: u32,
    #[serde(default, rename = "term")]
    pub terms: Vec<Term>,
}

impl Default for Glossary {
    fn default() -> Self {
        Self {
            schema_version: 1,
            terms: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_round_trips_through_toml() {
        let g = Glossary::default();
        let text = toml::to_string_pretty(&g).unwrap();
        let parsed: Glossary = toml::from_str(&text).unwrap();
        assert_eq!(parsed, g);
    }

    #[test]
    fn parses_terms_as_array_of_tables() {
        let text = r#"
            schema_version = 1

            [[term]]
            name = "Order"
            definition = "A customer's confirmed basket, billed as one unit."

            [[term]]
            name = "Cart"
            definition = "A mutable basket before checkout."
        "#;
        let g: Glossary = toml::from_str(text).unwrap();
        assert_eq!(g.terms.len(), 2);
        assert_eq!(g.terms[0].name, "Order");
        assert_eq!(g.terms[1].name, "Cart");
    }

    #[test]
    fn terms_default_to_empty_when_omitted() {
        let g: Glossary = toml::from_str("schema_version = 1\n").unwrap();
        assert!(g.terms.is_empty());
    }
}
```

- [ ] **Step 2: Run the tests**

Run: `cargo test --lib model::glossary`
Expected: PASS (3 tests).

- [ ] **Step 3: Commit**

```bash
git add src/model/glossary.rs
git commit -m "feat: glossary schema with array-of-tables terms"
```

---

## Task 4: Spec record schema  _(parallel-eligible with Tasks 2, 3, 5)_

**Files:**
- Modify: `src/model/spec.rs`

- [ ] **Step 1: Write the schema and its tests**

Replace `src/model/spec.rs` with:

```rust
use serde::{Deserialize, Serialize};

/// `.circuit/specs/<id>.toml` — a spec session's authored intent.
/// A spec session writes no application code; it owns the DAG.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpecRecord {
    pub schema_version: u32,
    pub id: String,
    pub title: String,
    pub intent: String,
    #[serde(default)]
    pub bounded_contexts: Vec<String>,
}

impl SpecRecord {
    /// Construct a v1 spec record.
    pub fn new(id: impl Into<String>, title: impl Into<String>, intent: impl Into<String>) -> Self {
        Self {
            schema_version: 1,
            id: id.into(),
            title: title.into(),
            intent: intent.into(),
            bounded_contexts: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_toml() {
        let mut s = SpecRecord::new("checkout", "Checkout & payment", "Let a customer pay.");
        s.bounded_contexts = vec!["billing".to_string(), "cart".to_string()];
        let text = toml::to_string_pretty(&s).unwrap();
        let parsed: SpecRecord = toml::from_str(&text).unwrap();
        assert_eq!(parsed, s);
    }

    #[test]
    fn bounded_contexts_default_to_empty() {
        let text = r#"
            schema_version = 1
            id = "checkout"
            title = "Checkout"
            intent = "Pay for a basket."
        "#;
        let s: SpecRecord = toml::from_str(text).unwrap();
        assert!(s.bounded_contexts.is_empty());
    }
}
```

- [ ] **Step 2: Run the tests**

Run: `cargo test --lib model::spec`
Expected: PASS (2 tests).

- [ ] **Step 3: Commit**

```bash
git add src/model/spec.rs
git commit -m "feat: spec record schema"
```

---

## Task 5: DAG node schema  _(parallel-eligible with Tasks 2, 3, 4)_

**Files:**
- Modify: `src/model/node.rs`

- [ ] **Step 1: Write the schema and its tests**

Replace `src/model/node.rs` with:

```rust
use serde::{Deserialize, Serialize};

/// `.circuit/dag/<id>.toml` — one DAG node = one vertical slice.
/// `branch` is the authored bridge to git (the worktree path is never stored).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DagNode {
    pub schema_version: u32,
    pub id: String,
    pub spec: String,
    pub title: String,
    #[serde(default)]
    pub intent: String,
    #[serde(default)]
    pub depends_on: Vec<String>,
    pub branch: String,
}

impl DagNode {
    /// Construct a v1 DAG node with no dependencies and an empty intent.
    pub fn new(
        id: impl Into<String>,
        spec: impl Into<String>,
        title: impl Into<String>,
        branch: impl Into<String>,
    ) -> Self {
        Self {
            schema_version: 1,
            id: id.into(),
            spec: spec.into(),
            title: title.into(),
            intent: String::new(),
            depends_on: Vec::new(),
            branch: branch.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_toml() {
        let mut n = DagNode::new("auth-slice", "checkout", "Authentication slice", "impl/checkout-auth");
        n.depends_on = vec!["cart-slice".to_string()];
        n.intent = "Log in and gate checkout.".to_string();
        let text = toml::to_string_pretty(&n).unwrap();
        let parsed: DagNode = toml::from_str(&text).unwrap();
        assert_eq!(parsed, n);
    }

    #[test]
    fn depends_on_and_intent_default_when_omitted() {
        let text = r#"
            schema_version = 1
            id = "auth-slice"
            spec = "checkout"
            title = "Auth"
            branch = "impl/checkout-auth"
        "#;
        let n: DagNode = toml::from_str(text).unwrap();
        assert!(n.depends_on.is_empty());
        assert_eq!(n.intent, "");
    }
}
```

- [ ] **Step 2: Run the tests**

Run: `cargo test --lib model::node`
Expected: PASS (2 tests).

- [ ] **Step 3: Commit**

```bash
git add src/model/node.rs
git commit -m "feat: DAG node schema with authored branch bridge"
```

---

## Task 6: Persistence boundary (`Workspace`) + `ModelError`

**Files:**
- Modify: `src/model/mod.rs`
- Modify: `src/model/store.rs`

- [ ] **Step 1: Add `ModelError` and the generic load/save helpers**

Replace `src/model/mod.rs` with:

```rust
pub mod config;
pub mod glossary;
pub mod node;
pub mod spec;
pub mod store;

use std::path::Path;

use serde::{de::DeserializeOwned, Serialize};
use thiserror::Error;

/// Errors at the `.circuit/` persistence boundary.
#[derive(Debug, Error)]
pub enum ModelError {
    #[error("io error at {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse {path}: {source}")]
    Parse {
        path: String,
        #[source]
        source: toml::de::Error,
    },
    #[error("failed to serialize: {0}")]
    Serialize(#[from] toml::ser::Error),
}

/// Read and deserialize a TOML file.
pub(crate) fn load_toml<T: DeserializeOwned>(path: &Path) -> Result<T, ModelError> {
    let text = std::fs::read_to_string(path).map_err(|source| ModelError::Io {
        path: path.display().to_string(),
        source,
    })?;
    toml::from_str(&text).map_err(|source| ModelError::Parse {
        path: path.display().to_string(),
        source,
    })
}

/// Serialize and write a TOML file, creating parent directories as needed.
pub(crate) fn save_toml<T: Serialize>(path: &Path, value: &T) -> Result<(), ModelError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|source| ModelError::Io {
            path: parent.display().to_string(),
            source,
        })?;
    }
    let text = toml::to_string_pretty(value)?;
    std::fs::write(path, text).map_err(|source| ModelError::Io {
        path: path.display().to_string(),
        source,
    })
}
```

> This requires the `thiserror` dependency. Add it to `Cargo.toml` under `[dependencies]` (after `toml`): `thiserror = "1"`.

- [ ] **Step 2: Write the `Workspace` and its tests**

Replace `src/model/store.rs` with:

```rust
use std::path::{Path, PathBuf};

use super::{
    config::Config, glossary::Glossary, load_toml, node::DagNode, save_toml, spec::SpecRecord,
    ModelError,
};

/// The `.circuit/` persistence boundary, rooted at a repo working tree.
/// All filesystem IO for the authored model lives here.
pub struct Workspace {
    root: PathBuf,
}

impl Workspace {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn circuit_dir(&self) -> PathBuf {
        self.root.join(".circuit")
    }

    pub fn config_path(&self) -> PathBuf {
        self.circuit_dir().join("config.toml")
    }

    pub fn glossary_path(&self) -> PathBuf {
        self.circuit_dir().join("glossary.toml")
    }

    pub fn specs_dir(&self) -> PathBuf {
        self.circuit_dir().join("specs")
    }

    pub fn dag_dir(&self) -> PathBuf {
        self.circuit_dir().join("dag")
    }

    pub fn spec_path(&self, id: &str) -> PathBuf {
        self.specs_dir().join(format!("{id}.toml"))
    }

    pub fn dag_node_path(&self, id: &str) -> PathBuf {
        self.dag_dir().join(format!("{id}.toml"))
    }

    /// A workspace is initialized once its config file exists.
    pub fn is_initialized(&self) -> bool {
        self.config_path().exists()
    }

    pub fn load_config(&self) -> Result<Config, ModelError> {
        load_toml(&self.config_path())
    }

    pub fn save_config(&self, c: &Config) -> Result<(), ModelError> {
        save_toml(&self.config_path(), c)
    }

    pub fn load_glossary(&self) -> Result<Glossary, ModelError> {
        load_toml(&self.glossary_path())
    }

    pub fn save_glossary(&self, g: &Glossary) -> Result<(), ModelError> {
        save_toml(&self.glossary_path(), g)
    }

    pub fn load_spec(&self, id: &str) -> Result<SpecRecord, ModelError> {
        load_toml(&self.spec_path(id))
    }

    pub fn save_spec(&self, s: &SpecRecord) -> Result<(), ModelError> {
        save_toml(&self.spec_path(&s.id), s)
    }

    pub fn load_dag_node(&self, id: &str) -> Result<DagNode, ModelError> {
        load_toml(&self.dag_node_path(id))
    }

    pub fn save_dag_node(&self, n: &DagNode) -> Result<(), ModelError> {
        save_toml(&self.dag_node_path(&n.id), n)
    }

    /// All DAG nodes, sorted by file path for deterministic order.
    pub fn list_dag_nodes(&self) -> Result<Vec<DagNode>, ModelError> {
        let dir = self.dag_dir();
        let mut nodes = Vec::new();
        if dir.is_dir() {
            let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)
                .map_err(|source| ModelError::Io {
                    path: dir.display().to_string(),
                    source,
                })?
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("toml"))
                .collect();
            paths.sort();
            for p in paths {
                nodes.push(load_toml(&p)?);
            }
        }
        Ok(nodes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_round_trips_through_disk() {
        let dir = tempfile::tempdir().unwrap();
        let ws = Workspace::new(dir.path());
        assert!(!ws.is_initialized());

        let c = Config::default();
        ws.save_config(&c).unwrap();
        assert!(ws.is_initialized());
        assert_eq!(ws.load_config().unwrap(), c);
    }

    #[test]
    fn spec_and_dag_node_round_trip_through_disk() {
        let dir = tempfile::tempdir().unwrap();
        let ws = Workspace::new(dir.path());

        let s = SpecRecord::new("checkout", "Checkout", "Pay for a basket.");
        ws.save_spec(&s).unwrap();
        assert_eq!(ws.load_spec("checkout").unwrap(), s);

        let n = DagNode::new("auth-slice", "checkout", "Auth", "impl/checkout-auth");
        ws.save_dag_node(&n).unwrap();
        assert_eq!(ws.load_dag_node("auth-slice").unwrap(), n);
    }

    #[test]
    fn list_dag_nodes_returns_sorted_and_empty_when_absent() {
        let dir = tempfile::tempdir().unwrap();
        let ws = Workspace::new(dir.path());
        assert!(ws.list_dag_nodes().unwrap().is_empty());

        ws.save_dag_node(&DagNode::new("b-slice", "s", "B", "impl/b")).unwrap();
        ws.save_dag_node(&DagNode::new("a-slice", "s", "A", "impl/a")).unwrap();
        let ids: Vec<String> = ws.list_dag_nodes().unwrap().into_iter().map(|n| n.id).collect();
        assert_eq!(ids, vec!["a-slice".to_string(), "b-slice".to_string()]);
    }
}
```

- [ ] **Step 3: Run the tests**

Run: `cargo test --lib model::store`
Expected: PASS (3 tests). (First build downloads `thiserror`.)

- [ ] **Step 4: Commit**

```bash
git add Cargo.toml Cargo.lock src/model/mod.rs src/model/store.rs
git commit -m "feat: Workspace persistence boundary with ModelError"
```

---

## Task 7: DAG validation (reuses M1's cycle detector)

**Files:**
- Modify: `src/dag/mod.rs`

- [ ] **Step 1: Write the validator and its tests**

Replace `src/dag/mod.rs` with:

```rust
use std::collections::{BTreeMap, HashSet};

use crate::graph::ArchGraph;
use crate::indicators::cycles::find_cycles;
use crate::model::node::DagNode;

/// A reason a DAG is not yet valid. Advisory and reportable, never thrown.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DagError {
    /// A dependency cycle (the same SCC the architecture cycle indicator finds).
    Cycle(Vec<String>),
    /// `node` declares a dependency on `missing`, which is not a known node id.
    DanglingRef { node: String, missing: String },
    /// More than one node maps to the same git branch.
    DuplicateBranch { branch: String, nodes: Vec<String> },
}

/// Build an `ArchGraph` from DAG nodes (module = node id, edge = dependency),
/// adding only edges whose target is a known node so dangling refs do not create
/// phantom nodes. Reuses the M1 graph model so the M1 cycle detector applies.
fn build_graph(nodes: &[DagNode], known: &HashSet<&str>) -> ArchGraph {
    let mut g = ArchGraph::new();
    for n in nodes {
        g.ensure_module(&n.id);
    }
    for n in nodes {
        let from = g.ensure_module(&n.id);
        for dep in &n.depends_on {
            if known.contains(dep.as_str()) {
                let to = g.ensure_module(dep);
                g.add_edge(from, to);
            }
        }
    }
    g
}

/// Validate a set of DAG nodes. Returns all problems found (empty = sound).
/// Errors are sorted within each kind for deterministic output.
pub fn validate(nodes: &[DagNode]) -> Vec<DagError> {
    let mut errors = Vec::new();
    let known: HashSet<&str> = nodes.iter().map(|n| n.id.as_str()).collect();

    // Dangling references.
    for n in nodes {
        for dep in &n.depends_on {
            if !known.contains(dep.as_str()) {
                errors.push(DagError::DanglingRef {
                    node: n.id.clone(),
                    missing: dep.clone(),
                });
            }
        }
    }

    // Duplicate branches.
    let mut by_branch: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for n in nodes {
        by_branch.entry(n.branch.as_str()).or_default().push(n.id.as_str());
    }
    for (branch, ns) in by_branch {
        if ns.len() > 1 {
            let mut nodes_for_branch: Vec<String> = ns.iter().map(|s| s.to_string()).collect();
            nodes_for_branch.sort();
            errors.push(DagError::DuplicateBranch {
                branch: branch.to_string(),
                nodes: nodes_for_branch,
            });
        }
    }

    // Cycles — reuse the M1 Tarjan SCC detector.
    let g = build_graph(nodes, &known);
    for cycle in find_cycles(&g) {
        errors.push(DagError::Cycle(cycle));
    }

    errors
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: &str, branch: &str, deps: &[&str]) -> DagNode {
        let mut n = DagNode::new(id, "checkout", id, branch);
        n.depends_on = deps.iter().map(|d| d.to_string()).collect();
        n
    }

    #[test]
    fn acyclic_resolved_unique_dag_is_sound() {
        let nodes = vec![
            node("cart", "impl/cart", &[]),
            node("auth", "impl/auth", &["cart"]),
        ];
        assert!(validate(&nodes).is_empty());
    }

    #[test]
    fn detects_a_dependency_cycle() {
        let nodes = vec![
            node("a", "impl/a", &["b"]),
            node("b", "impl/b", &["a"]),
        ];
        let errors = validate(&nodes);
        assert!(errors.contains(&DagError::Cycle(vec!["a".to_string(), "b".to_string()])));
    }

    #[test]
    fn detects_a_dangling_reference() {
        let nodes = vec![node("auth", "impl/auth", &["ghost"])];
        assert_eq!(
            validate(&nodes),
            vec![DagError::DanglingRef {
                node: "auth".to_string(),
                missing: "ghost".to_string(),
            }]
        );
    }

    #[test]
    fn detects_duplicate_branches() {
        let nodes = vec![
            node("a", "impl/shared", &[]),
            node("b", "impl/shared", &[]),
        ];
        assert_eq!(
            validate(&nodes),
            vec![DagError::DuplicateBranch {
                branch: "impl/shared".to_string(),
                nodes: vec!["a".to_string(), "b".to_string()],
            }]
        );
    }
}
```

- [ ] **Step 2: Run the tests**

Run: `cargo test --lib dag`
Expected: PASS (4 tests).

- [ ] **Step 3: Commit**

```bash
git add src/dag/mod.rs
git commit -m "feat: DAG validation reusing M1 cycle detector"
```

---

## Task 8: `circuit init` command + scaffolding

**Files:**
- Modify: `src/main.rs`
- Create: `tests/data_model.rs`

- [ ] **Step 1: Write the failing integration test**

Create `tests/data_model.rs`:

```rust
use assert_cmd::Command;
use predicates::prelude::*;

/// Run `circuit` with args in a given working directory.
fn circuit(dir: &std::path::Path) -> Command {
    let mut cmd = Command::cargo_bin("circuit").unwrap();
    cmd.current_dir(dir);
    cmd
}

#[test]
fn init_scaffolds_circuit_directory() {
    let dir = tempfile::tempdir().unwrap();

    circuit(dir.path())
        .arg("init")
        .assert()
        .success()
        .stdout(predicate::str::contains("Initialized"));

    assert!(dir.path().join(".circuit/config.toml").exists());
    assert!(dir.path().join(".circuit/glossary.toml").exists());

    let gitignore = std::fs::read_to_string(dir.path().join(".gitignore")).unwrap();
    assert!(gitignore.contains(".circuit/local.toml"));
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --test data_model init_scaffolds_circuit_directory`
Expected: FAIL — `init` is not a known subcommand yet.

- [ ] **Step 3: Restructure the CLI and implement `init`**

Replace `src/main.rs` with:

```rust
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

use circuit::model::config::Config;
use circuit::model::glossary::Glossary;
use circuit::model::store::Workspace;

#[derive(Parser)]
#[command(name = "circuit", about = "Architecture derivation, sessions & flow")]
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
    /// Scaffold the `.circuit/` authored data model in the current repo
    Init {
        #[arg(default_value = ".")]
        path: PathBuf,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Analyze { path } => run_analyze(&path),
        Command::Init { path } => run_init(&path),
    }
}

fn run_analyze(path: &Path) -> Result<()> {
    let graph = circuit::builder::build_graph(path)?;
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
    Ok(())
}

fn run_init(path: &Path) -> Result<()> {
    let ws = Workspace::new(path);
    if ws.is_initialized() {
        println!("Already initialized at {}", ws.circuit_dir().display());
        return Ok(());
    }
    ws.save_config(&Config::default()).context("writing config.toml")?;
    ws.save_glossary(&Glossary::default()).context("writing glossary.toml")?;
    ensure_gitignored(path, ".circuit/local.toml").context("updating .gitignore")?;
    println!("Initialized .circuit/ at {}", ws.circuit_dir().display());
    Ok(())
}

/// Append a line to `.gitignore` if not already present (idempotent).
fn ensure_gitignored(root: &Path, entry: &str) -> Result<()> {
    let path = root.join(".gitignore");
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    if existing.lines().any(|l| l.trim() == entry) {
        return Ok(());
    }
    let mut content = existing;
    if !content.is_empty() && !content.ends_with('\n') {
        content.push('\n');
    }
    content.push_str(entry);
    content.push('\n');
    std::fs::write(&path, content)?;
    Ok(())
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test --test data_model init_scaffolds_circuit_directory`
Expected: PASS.

- [ ] **Step 5: Verify the M1 CLI test still passes**

Run: `cargo test --test cli`
Expected: PASS — `analyze` behavior is unchanged.

- [ ] **Step 6: Commit**

```bash
git add src/main.rs tests/data_model.rs
git commit -m "feat: circuit init scaffolds .circuit/ data model"
```

---

## Task 9: `circuit spec new` command

**Files:**
- Modify: `src/main.rs`
- Modify: `tests/data_model.rs`

- [ ] **Step 1: Write the failing integration test**

Append to `tests/data_model.rs`:

```rust
#[test]
fn spec_new_writes_a_spec_record() {
    let dir = tempfile::tempdir().unwrap();
    circuit(dir.path()).arg("init").assert().success();

    circuit(dir.path())
        .args(["spec", "new", "checkout"])
        .args(["--title", "Checkout & payment"])
        .args(["--intent", "Let a customer pay for a basket."])
        .args(["--context", "billing"])
        .args(["--context", "cart"])
        .assert()
        .success()
        .stdout(predicate::str::contains("checkout"));

    let text = std::fs::read_to_string(dir.path().join(".circuit/specs/checkout.toml")).unwrap();
    assert!(text.contains("title = \"Checkout & payment\""));
    assert!(text.contains("billing"));
    assert!(text.contains("cart"));
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --test data_model spec_new_writes_a_spec_record`
Expected: FAIL — `spec` is not a known subcommand.

- [ ] **Step 3: Add the `Spec` command**

In `src/main.rs`, add `SpecRecord` to the model imports so the import block reads:

```rust
use circuit::model::config::Config;
use circuit::model::glossary::Glossary;
use circuit::model::spec::SpecRecord;
use circuit::model::store::Workspace;
```

Add a `Spec` variant to the `Command` enum (after `Init`):

```rust
    /// Spec-session commands
    Spec {
        #[command(subcommand)]
        command: SpecCommand,
    },
```

Add the `SpecCommand` enum after the `Command` enum:

```rust
#[derive(Subcommand)]
enum SpecCommand {
    /// Create a new spec session
    New {
        /// Spec id (used as the filename)
        id: String,
        #[arg(long)]
        title: String,
        #[arg(long)]
        intent: String,
        /// Bounded context (repeatable)
        #[arg(long = "context")]
        contexts: Vec<String>,
        #[arg(long, default_value = ".")]
        path: PathBuf,
    },
}
```

Add the match arm in `main` (after `Command::Init`):

```rust
        Command::Spec { command } => run_spec(command),
```

Add the handler function:

```rust
fn run_spec(command: SpecCommand) -> Result<()> {
    match command {
        SpecCommand::New { id, title, intent, contexts, path } => {
            let ws = Workspace::new(&path);
            let mut spec = SpecRecord::new(&id, title, intent);
            spec.bounded_contexts = contexts;
            ws.save_spec(&spec).with_context(|| format!("writing spec {id}"))?;
            println!("Created spec session: {id}");
            Ok(())
        }
    }
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test --test data_model spec_new_writes_a_spec_record`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/main.rs tests/data_model.rs
git commit -m "feat: circuit spec new creates a spec session"
```

---

## Task 10: `circuit dag add-node` and `circuit dag link`

**Files:**
- Modify: `src/main.rs`
- Modify: `tests/data_model.rs`

- [ ] **Step 1: Write the failing integration test**

Append to `tests/data_model.rs`:

```rust
#[test]
fn dag_add_node_and_link_build_the_graph() {
    let dir = tempfile::tempdir().unwrap();
    circuit(dir.path()).arg("init").assert().success();

    circuit(dir.path())
        .args(["dag", "add-node", "cart-slice"])
        .args(["--spec", "checkout"])
        .args(["--title", "Cart slice"])
        .args(["--branch", "impl/checkout-cart"])
        .assert()
        .success();

    circuit(dir.path())
        .args(["dag", "add-node", "auth-slice"])
        .args(["--spec", "checkout"])
        .args(["--title", "Auth slice"])
        .args(["--branch", "impl/checkout-auth"])
        .args(["--depends-on", "cart-slice"])
        .assert()
        .success();

    // Link adds an extra dependency edge to an existing node.
    circuit(dir.path())
        .args(["dag", "link", "auth-slice", "cart-slice"])
        .assert()
        .success();

    let auth = std::fs::read_to_string(dir.path().join(".circuit/dag/auth-slice.toml")).unwrap();
    assert!(auth.contains("branch = \"impl/checkout-auth\""));
    assert!(auth.contains("cart-slice"));
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --test data_model dag_add_node_and_link_build_the_graph`
Expected: FAIL — `dag` is not a known subcommand.

- [ ] **Step 3: Add the `Dag` command (add-node + link)**

In `src/main.rs`, add `DagNode` to the model imports so the block reads:

```rust
use circuit::model::config::Config;
use circuit::model::glossary::Glossary;
use circuit::model::node::DagNode;
use circuit::model::spec::SpecRecord;
use circuit::model::store::Workspace;
```

Add a `Dag` variant to the `Command` enum (after `Spec`):

```rust
    /// Task-DAG commands
    Dag {
        #[command(subcommand)]
        command: DagCommand,
    },
```

Add the `DagCommand` enum after `SpecCommand`:

```rust
#[derive(Subcommand)]
enum DagCommand {
    /// Add a DAG node (one vertical slice)
    AddNode {
        /// Node id (used as the filename)
        id: String,
        #[arg(long)]
        spec: String,
        #[arg(long)]
        title: String,
        #[arg(long)]
        branch: String,
        #[arg(long, default_value = "")]
        intent: String,
        /// Dependency node id (repeatable)
        #[arg(long = "depends-on")]
        depends_on: Vec<String>,
        #[arg(long, default_value = ".")]
        path: PathBuf,
    },
    /// Add a dependency edge from one existing node to another
    Link {
        /// The dependent node
        from: String,
        /// The node it depends on
        to: String,
        #[arg(long, default_value = ".")]
        path: PathBuf,
    },
}
```

Add the match arm in `main` (after `Command::Spec`):

```rust
        Command::Dag { command } => run_dag(command),
```

Add the handler function:

```rust
fn run_dag(command: DagCommand) -> Result<()> {
    match command {
        DagCommand::AddNode { id, spec, title, branch, intent, depends_on, path } => {
            let ws = Workspace::new(&path);
            let mut node = DagNode::new(&id, spec, title, branch);
            node.intent = intent;
            node.depends_on = depends_on;
            ws.save_dag_node(&node).with_context(|| format!("writing dag node {id}"))?;
            println!("Added DAG node: {id}");
            Ok(())
        }
        DagCommand::Link { from, to, path } => {
            let ws = Workspace::new(&path);
            let mut node = ws
                .load_dag_node(&from)
                .with_context(|| format!("loading dag node {from}"))?;
            if !node.depends_on.contains(&to) {
                node.depends_on.push(to.clone());
            }
            ws.save_dag_node(&node).with_context(|| format!("writing dag node {from}"))?;
            println!("Linked {from} → {to}");
            Ok(())
        }
    }
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test --test data_model dag_add_node_and_link_build_the_graph`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/main.rs tests/data_model.rs
git commit -m "feat: circuit dag add-node and link"
```

---

## Task 11: `circuit dag check` command

**Files:**
- Modify: `src/main.rs`
- Modify: `tests/data_model.rs`

- [ ] **Step 1: Write the failing integration test**

Append to `tests/data_model.rs`:

```rust
#[test]
fn dag_check_reports_sound_and_cycles() {
    let dir = tempfile::tempdir().unwrap();
    circuit(dir.path()).arg("init").assert().success();

    circuit(dir.path())
        .args(["dag", "add-node", "cart-slice", "--spec", "checkout", "--title", "Cart", "--branch", "impl/cart"])
        .assert()
        .success();
    circuit(dir.path())
        .args(["dag", "add-node", "auth-slice", "--spec", "checkout", "--title", "Auth", "--branch", "impl/auth", "--depends-on", "cart-slice"])
        .assert()
        .success();

    // Sound DAG.
    circuit(dir.path())
        .args(["dag", "check"])
        .assert()
        .success()
        .stdout(predicate::str::contains("DAG sound"));

    // Introduce a cycle: cart-slice now depends on auth-slice.
    circuit(dir.path())
        .args(["dag", "link", "cart-slice", "auth-slice"])
        .assert()
        .success();

    circuit(dir.path())
        .args(["dag", "check"])
        .assert()
        .failure()
        .stdout(predicate::str::contains("cycle"));
}
```

> Note: `dag check` exits non-zero when problems are found so the failure is scriptable; advisory in product terms, but a non-zero exit is the honest CLI signal here.

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --test data_model dag_check_reports_sound_and_cycles`
Expected: FAIL — `check` is not a known `dag` subcommand.

- [ ] **Step 3: Add the `Check` subcommand**

In `src/main.rs`, add the DAG validation imports near the other `circuit::` imports:

```rust
use circuit::dag::{validate, DagError};
```

Add a `Check` variant to the `DagCommand` enum (after `Link`):

```rust
    /// Validate the DAG (acyclic, refs resolve, unique branches)
    Check {
        #[arg(long, default_value = ".")]
        path: PathBuf,
    },
```

Add a `Check` arm inside `run_dag`'s `match` (after the `Link` arm):

```rust
        DagCommand::Check { path } => {
            let ws = Workspace::new(&path);
            let nodes = ws.list_dag_nodes().context("reading dag nodes")?;
            let errors = validate(&nodes);
            if errors.is_empty() {
                println!("DAG sound — {} node(s), no problems", nodes.len());
                return Ok(());
            }
            for e in &errors {
                match e {
                    DagError::Cycle(c) => println!("  cycle: {}", c.join(" → ")),
                    DagError::DanglingRef { node, missing } => {
                        println!("  dangling ref: {node} → {missing} (no such node)")
                    }
                    DagError::DuplicateBranch { branch, nodes } => {
                        println!("  duplicate branch {branch}: {}", nodes.join(", "))
                    }
                }
            }
            std::process::exit(1);
        }
```

> The `run_dag` signature stays `-> Result<()>`; the `Check` arm either returns `Ok(())` or exits the process with code 1 after printing problems.

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test --test data_model dag_check_reports_sound_and_cycles`
Expected: PASS.

- [ ] **Step 5: Run the full suite**

Run: `cargo test`
Expected: PASS (all M1 + M2a unit and integration tests).

- [ ] **Step 6: Commit**

```bash
git add src/main.rs tests/data_model.rs
git commit -m "feat: circuit dag check validates the task graph"
```

---

## Task 12: Dogfood — author Circuit's own M2 DAG

**Files:** none (verification only; the written `.circuit/` is committed in M2b/M2c as the project's real authored state, not here)

- [ ] **Step 1: Initialize and author a small DAG in a scratch dir**

Run, from the repo root, in a throwaway temp location to avoid polluting the repo:

```bash
TMP=$(mktemp -d)
cargo run -- init "$TMP"
cargo run -- spec new m2 --title "M2 session model" --intent "Workflow shell" --path "$TMP"
cargo run -- dag add-node m2a --spec m2 --title "Data model & DAG" --branch m2a --path "$TMP"
cargo run -- dag add-node m2b --spec m2 --title "Sessions & git" --branch m2b --depends-on m2a --path "$TMP"
cargo run -- dag add-node m2c --spec m2 --title "Forge & cockpit" --branch m2c --depends-on m2b --path "$TMP"
cargo run -- dag check --path "$TMP"
```

Expected: `dag check` prints `DAG sound — 3 node(s), no problems`.

- [ ] **Step 2: Confirm a cycle is caught**

```bash
cargo run -- dag link m2a m2c --path "$TMP"
cargo run -- dag check --path "$TMP"
```

Expected: non-zero exit, prints a `cycle:` line containing `m2a`, `m2b`, `m2c`.

- [ ] **Step 3: Clean up**

```bash
rm -rf "$TMP"
```

No commit (verification only).

---

## Self-Review

**Spec coverage (against `2026-06-14-circuit-m2-session-model-design.md`):**
- §3.1 `.circuit/` layout (one file per entity, TOML) → Tasks 2–6 (config/glossary/spec/node + `Workspace` paths) ✓
- §3.2 schemas with `schema_version` → Tasks 2–5 ✓
- §3.2 gitignored `local.toml` → Task 8 (`ensure_gitignored`) ✓
- §3.3 derived-vs-authored (only intent stored; branch name authored, no derived state) → schemas store no stage/path ✓
- §10 DAG validation reusing M1 Tarjan SCC → Task 7 ✓
- §11 CLI: `init`, `spec new`, `dag add-node`, `dag link`, `dag check` → Tasks 8–11 ✓
- §13 testing: pure unit tests + `assert_cmd` E2E → every task ✓
- **Out of M2a scope (later plans, correctly absent):** session records + identity, stage machine, git/forge adapters, spawn/worktrees, flow rail, DAG-board renderer, health rollup, checkpoints → M2b/M2c.

**Placeholder scan:** none — every step has complete code and exact commands. The one intentional empty-string default (`DagCommand::AddNode { intent, .. }`) is a real default, not a placeholder.

**Type consistency:**
- `Workspace` API (`new`, `is_initialized`, `save_config`/`load_config`, `save_glossary`, `save_spec`/`load_spec`, `save_dag_node`/`load_dag_node`, `list_dag_nodes`, `circuit_dir`) consistent across Tasks 6, 8–11.
- `Config::default`, `Glossary::default`, `SpecRecord::new(id,title,intent)`, `DagNode::new(id,spec,title,branch)` signatures match every call site (Tasks 6, 8–10).
- `validate(&[DagNode]) -> Vec<DagError>` and the `DagError` variants (`Cycle`, `DanglingRef`, `DuplicateBranch`) match between Task 7 and Task 11's match arms.
- `ModelError` is internal to the model boundary; the CLI consumes `Result` via `anyhow` `Context`, so no cross-task type leak.

**Scope:** one shippable slice (the authored data model + DAG authoring/validation). Bounded; produces working CLI commands with full test coverage. M2b and M2c build on the `Workspace` and schemas defined here.
