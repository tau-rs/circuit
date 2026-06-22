# M3 Slice A — System-Level Projection Schema & Spec Attachment — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Establish the system-level projection as a first-class authored `.circuit/` artifact attached to a spec session, round-tripped through TOML, with a minimal `projection init|show` CLI.

**Architecture:** Pure hexagonal extension mirroring the M2 authored-artifact pattern (`SpecRecord`/`SpecRepo`, `DagNode`/`DagRepo`): a new `SystemProjection` model, a `ProjectionRepo` outbound port, a `Workspace` adapter impl, two port-generic `app.rs` functions, and clap glue. Dependencies point inward; the app layer is generic over traits and does no IO or printing.

**Tech Stack:** Rust, serde + toml (persistence), clap (CLI), thiserror at the store boundary / anyhow in the app layer, assert_cmd + predicates + tempfile (integration tests).

## Global Constraints

- `Component.layer` reuses `crate::layer::Layer` — do not invent a parallel layer type.
- Storage: one file per spec at `.circuit/projections/<spec-id>.toml`; `spec` field is the FK to `SpecRecord.id`.
- `relationship.kind` is a free `String`, never a closed enum (YAGNI).
- Every projection section vec is `#[serde(default)]` so skeleton and partial files parse.
- App layer: port-generic, `anyhow` internally, no IO/printing — match `spec_new`.
- Out of scope (do NOT add): conformance/diff-against-code, granular mutation verbs, `projection check`, mermaid render, slice-level projection, UI mockup.
- Commit messages: conventional, imperative, scoped (`feat(m3): …`).

**Spec:** `docs/superpowers/specs/2026-06-22-circuit-m3-slice-a-system-projection-design.md`

---

### Task 1: Make `Layer` serde-serializable

`Layer` is currently a plain enum with no serde derives. `Component.layer` needs it to serialize as a lowercase string (`"domain"`/`"application"`/`"adapter"`/`"unknown"`).

**Files:**
- Modify: `src/layer.rs:1-7` (add derives) and its `#[cfg(test)] mod tests`
- Test: `src/layer.rs` (inline)

**Interfaces:**
- Produces: `Layer` now `impl Serialize + Deserialize`, serializing lowercase.

- [ ] **Step 1: Write the failing test**

Add to the `mod tests` block in `src/layer.rs`:

```rust
#[test]
fn layer_round_trips_as_lowercase_string() {
    #[derive(serde::Serialize, serde::Deserialize, PartialEq, Debug)]
    struct Wrap {
        layer: Layer,
    }
    for (variant, name) in [
        (Layer::Domain, "domain"),
        (Layer::Application, "application"),
        (Layer::Adapter, "adapter"),
        (Layer::Unknown, "unknown"),
    ] {
        let text = toml::to_string(&Wrap { layer: variant }).unwrap();
        assert!(
            text.contains(&format!("layer = \"{name}\"")),
            "got: {text}"
        );
        let back: Wrap = toml::from_str(&text).unwrap();
        assert_eq!(back.layer, variant);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib layer_round_trips_as_lowercase_string`
Expected: FAIL to compile — `Layer` does not implement `Serialize`/`Deserialize`.

- [ ] **Step 3: Add the derives**

Change `src/layer.rs:1` from:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layer {
```

to:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Layer {
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib layer_round_trips_as_lowercase_string`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/layer.rs
git commit -m "feat(m3): make Layer serde-serializable for projection components"
```

---

### Task 2: `SystemProjection` model

The authored projection type and its five section structs, with round-trip tests.

**Files:**
- Create: `src/model/projection.rs`
- Modify: `src/model/mod.rs:1-5` (add `pub mod projection;`)
- Test: `src/model/projection.rs` (inline)

**Interfaces:**
- Consumes: `crate::layer::Layer` (Task 1).
- Produces:
  - `SystemProjection { schema_version: u32, spec: String, component: Vec<Component>, edge: Vec<IntendedEdge>, context: Vec<Context>, relationship: Vec<Relationship>, contract: Vec<Contract> }`
  - `SystemProjection::new(spec: impl Into<String>) -> SystemProjection`
  - `Component { name: String, layer: Layer }`
  - `IntendedEdge { from: String, to: String }`
  - `Context { name: String }`
  - `Relationship { upstream: String, downstream: String, kind: String }`
  - `Contract { name: String, provider: String, consumers: Vec<String> }`

- [ ] **Step 1: Write the failing tests**

Create `src/model/projection.rs`:

```rust
use serde::{Deserialize, Serialize};

use crate::layer::Layer;

/// `.circuit/projections/<spec-id>.toml` — a spec session's system-level
/// projection: the intended architecture, context map, and inter-slice
/// contracts. Authored intent only; never diffed against code in this slice
/// (that is M3 slice C).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SystemProjection {
    pub schema_version: u32,
    /// Spec session id this projection belongs to (FK → `SpecRecord.id`).
    pub spec: String,
    #[serde(default)]
    pub component: Vec<Component>,
    #[serde(default)]
    pub edge: Vec<IntendedEdge>,
    #[serde(default)]
    pub context: Vec<Context>,
    #[serde(default)]
    pub relationship: Vec<Relationship>,
    #[serde(default)]
    pub contract: Vec<Contract>,
}

/// An intended module/component and the layer it is meant to live in. `layer`
/// reuses M1's `Layer` so slice C can diff projected layers against derived ones.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Component {
    pub name: String,
    pub layer: Layer,
}

/// An intended (allowed) dependency edge. Slice C diffs code edges against these.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntendedEdge {
    pub from: String,
    pub to: String,
}

/// A bounded context in the context map.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Context {
    pub name: String,
}

/// A directed relationship between two contexts. `kind` is a free string
/// (e.g. "customer-supplier", "conformist", "acl"), NOT a closed enum.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Relationship {
    pub upstream: String,
    pub downstream: String,
    pub kind: String,
}

/// A named inter-slice contract (a port one context provides to others).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Contract {
    pub name: String,
    pub provider: String,
    #[serde(default)]
    pub consumers: Vec<String>,
}

impl SystemProjection {
    /// A v1 skeleton: identity only, all sections empty.
    pub fn new(spec: impl Into<String>) -> Self {
        Self {
            schema_version: 1,
            spec: spec.into(),
            component: Vec::new(),
            edge: Vec::new(),
            context: Vec::new(),
            relationship: Vec::new(),
            contract: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn populated() -> SystemProjection {
        SystemProjection {
            schema_version: 1,
            spec: "checkout".into(),
            component: vec![
                Component { name: "billing".into(), layer: Layer::Domain },
                Component { name: "gh-adapter".into(), layer: Layer::Adapter },
            ],
            edge: vec![IntendedEdge { from: "gh-adapter".into(), to: "billing".into() }],
            context: vec![
                Context { name: "checkout".into() },
                Context { name: "payments".into() },
            ],
            relationship: vec![Relationship {
                upstream: "payments".into(),
                downstream: "checkout".into(),
                kind: "customer-supplier".into(),
            }],
            contract: vec![Contract {
                name: "PaymentGateway".into(),
                provider: "payments".into(),
                consumers: vec!["checkout".into()],
            }],
        }
    }

    #[test]
    fn full_projection_round_trips_through_toml() {
        let p = populated();
        let text = toml::to_string_pretty(&p).unwrap();
        let parsed: SystemProjection = toml::from_str(&text).unwrap();
        assert_eq!(parsed, p);
    }

    #[test]
    fn skeleton_round_trips_with_empty_sections() {
        let p = SystemProjection::new("checkout");
        let text = toml::to_string_pretty(&p).unwrap();
        let parsed: SystemProjection = toml::from_str(&text).unwrap();
        assert_eq!(parsed, p);
        assert!(parsed.component.is_empty());
        assert!(parsed.contract.is_empty());
    }

    #[test]
    fn hand_authored_toml_with_sections_omitted_parses() {
        let text = r#"
            schema_version = 1
            spec = "checkout"
        "#;
        let p: SystemProjection = toml::from_str(text).unwrap();
        assert_eq!(p.spec, "checkout");
        assert!(p.component.is_empty());
        assert!(p.edge.is_empty());
        assert!(p.context.is_empty());
        assert!(p.relationship.is_empty());
        assert!(p.contract.is_empty());
    }
}
```

- [ ] **Step 2: Register the module**

In `src/model/mod.rs`, add `projection` to the module list (keep alphabetical order). After:

```rust
pub mod node;
```

add:

```rust
pub mod projection;
```

- [ ] **Step 3: Run tests to verify they fail, then pass**

Run: `cargo test --lib model::projection`
Expected: PASS (3 tests). If the module wasn't registered, compilation fails first — fix Step 2.

- [ ] **Step 4: Commit**

```bash
git add src/model/projection.rs src/model/mod.rs
git commit -m "feat(m3): add SystemProjection authored model"
```

---

### Task 3: `ProjectionRepo` port + `Workspace` adapter

The outbound port and its filesystem implementation under `.circuit/projections/`.

**Files:**
- Modify: `src/ports.rs` (add import + trait, after the `DagRepo`/`SessionRepo` traits)
- Modify: `src/adapters/store.rs` (paths + load/save/exists + `impl ProjectionRepo`, plus a disk test)
- Test: `src/adapters/store.rs` (inline)

**Interfaces:**
- Consumes: `SystemProjection` (Task 2).
- Produces:
  - `trait ProjectionRepo { type Error; fn load_projection(&self, spec: &str) -> Result<SystemProjection, Self::Error>; fn save_projection(&self, p: &SystemProjection) -> Result<(), Self::Error>; fn projection_exists(&self, spec: &str) -> bool; }`
  - `Workspace::{projections_dir, projection_path, load_projection, save_projection, projection_exists}` and `impl ProjectionRepo for Workspace { type Error = ModelError; }`.

- [ ] **Step 1: Write the failing test**

Add to `mod tests` in `src/adapters/store.rs`:

```rust
#[test]
fn projection_round_trips_through_disk_and_exists_flips() {
    use crate::model::projection::{Component, SystemProjection};
    use crate::layer::Layer;
    let dir = tempfile::tempdir().unwrap();
    let ws = Workspace::new(dir.path());

    assert!(!ws.projection_exists("checkout"));

    let mut p = SystemProjection::new("checkout");
    p.component.push(Component { name: "billing".into(), layer: Layer::Domain });
    ws.save_projection(&p).unwrap();

    assert!(ws.projection_exists("checkout"));
    assert_eq!(ws.load_projection("checkout").unwrap(), p);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib projection_round_trips_through_disk_and_exists_flips`
Expected: FAIL to compile — `save_projection` / `load_projection` / `projection_exists` do not exist.

- [ ] **Step 3: Add the port trait**

In `src/ports.rs`, add to the imports near the top (after `use crate::model::node::DagNode;`):

```rust
use crate::model::projection::SystemProjection;
```

Then add this trait immediately after the `DagRepo` trait definition:

```rust
/// Persistence for a spec session's system-level projection
/// (`projections/<spec-id>.toml`). `projection_exists` gates the no-clobber
/// guard on `projection init`, mirroring `SettingsRepo::is_initialized`.
pub trait ProjectionRepo {
    type Error: std::error::Error + Send + Sync + 'static;
    fn load_projection(&self, spec: &str) -> Result<SystemProjection, Self::Error>;
    fn save_projection(&self, p: &SystemProjection) -> Result<(), Self::Error>;
    fn projection_exists(&self, spec: &str) -> bool;
}
```

- [ ] **Step 4: Add the `Workspace` inherent methods**

In `src/adapters/store.rs`, extend the `use crate::model::{…}` block to include `projection::SystemProjection`. Then add these methods inside `impl Workspace` (next to `dag_node_path` / `load_dag_node`):

```rust
pub fn projections_dir(&self) -> PathBuf {
    self.circuit_dir().join("projections")
}

pub fn projection_path(&self, spec: &str) -> PathBuf {
    self.projections_dir().join(format!("{spec}.toml"))
}

pub fn load_projection(&self, spec: &str) -> Result<SystemProjection, ModelError> {
    load_toml(&self.projection_path(spec))
}

pub fn save_projection(&self, p: &SystemProjection) -> Result<(), ModelError> {
    save_toml(&self.projection_path(&p.spec), p)
}

pub fn projection_exists(&self, spec: &str) -> bool {
    self.projection_path(spec).exists()
}
```

- [ ] **Step 5: Add the `ProjectionRepo` impl**

In `src/adapters/store.rs`, extend the `use crate::ports::{…}` line to include `ProjectionRepo`, then add the impl next to `impl DagRepo for Workspace`:

```rust
impl ProjectionRepo for Workspace {
    type Error = ModelError;
    fn load_projection(&self, spec: &str) -> Result<SystemProjection, ModelError> {
        Workspace::load_projection(self, spec)
    }
    fn save_projection(&self, p: &SystemProjection) -> Result<(), ModelError> {
        Workspace::save_projection(self, p)
    }
    fn projection_exists(&self, spec: &str) -> bool {
        Workspace::projection_exists(self, spec)
    }
}
```

- [ ] **Step 6: Run test to verify it passes**

Run: `cargo test --lib projection_round_trips_through_disk_and_exists_flips`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add src/ports.rs src/adapters/store.rs
git commit -m "feat(m3): add ProjectionRepo port and Workspace adapter"
```

---

### Task 4: App layer — `projection_init`, `projection_show`, render helper

Port-generic use-cases plus the pure text renderer, and the `MemStore` fake gains `ProjectionRepo`.

**Files:**
- Modify: `src/app.rs` (imports, two pub fns, private `render_projection`, fakes `MemStore`, tests)
- Test: `src/app.rs` (inline)

**Interfaces:**
- Consumes: `SettingsRepo`, `SpecRepo`, `ProjectionRepo`, `SystemProjection`.
- Produces:
  - `pub fn projection_init<S: SettingsRepo, R: SpecRepo, P: ProjectionRepo>(settings: &S, specs: &R, projections: &P, spec: &str) -> anyhow::Result<()>`
  - `pub fn projection_show<S: SettingsRepo, P: ProjectionRepo>(settings: &S, projections: &P, spec: &str) -> anyhow::Result<String>`

- [ ] **Step 1: Extend the `MemStore` fake with `ProjectionRepo`**

In `src/app.rs`, inside `pub(crate) mod fakes`:

Add imports at the top of the module:

```rust
use crate::model::projection::SystemProjection;
use crate::ports::ProjectionRepo;
```

Add a field to `MemStore` (after `sessions`):

```rust
pub projections: RefCell<HashMap<String, SystemProjection>>,
```

Add the impl (after `impl SessionRepo for MemStore`):

```rust
impl ProjectionRepo for MemStore {
    type Error = FakeErr;
    fn load_projection(&self, spec: &str) -> Result<SystemProjection, FakeErr> {
        self.projections
            .borrow()
            .get(spec)
            .cloned()
            .ok_or_else(|| FakeErr(format!("no projection {spec}")))
    }
    fn save_projection(&self, p: &SystemProjection) -> Result<(), FakeErr> {
        self.projections
            .borrow_mut()
            .insert(p.spec.clone(), p.clone());
        Ok(())
    }
    fn projection_exists(&self, spec: &str) -> bool {
        self.projections.borrow().contains_key(spec)
    }
}
```

(Note: `MemStore` derives `Default`; `RefCell<HashMap<…>>` is `Default`, so existing `MemStore { initialized: true, ..Default::default() }` construction is unaffected.)

- [ ] **Step 2: Write the failing tests**

Add to `mod tests` in `src/app.rs` (the `use super::*;` and `use crate::app::fakes::MemStore;` are already in scope):

```rust
fn store_with_spec(id: &str) -> MemStore {
    let store = MemStore { initialized: true, ..Default::default() };
    store
        .specs
        .borrow_mut()
        .insert(id.into(), crate::model::spec::SpecRecord::new(id, "T", "intent"));
    store
}

#[test]
fn projection_init_writes_skeleton_for_existing_spec() {
    let store = store_with_spec("checkout");
    projection_init(&store, &store, &store, "checkout").unwrap();
    assert!(store.projections.borrow().contains_key("checkout"));
}

#[test]
fn projection_init_requires_the_spec_to_exist() {
    let store = MemStore { initialized: true, ..Default::default() };
    let err = projection_init(&store, &store, &store, "checkout").unwrap_err();
    assert!(err.to_string().contains("no spec 'checkout'"), "got: {err}");
}

#[test]
fn projection_init_refuses_to_clobber() {
    let store = store_with_spec("checkout");
    projection_init(&store, &store, &store, "checkout").unwrap();
    let err = projection_init(&store, &store, &store, "checkout").unwrap_err();
    assert!(err.to_string().contains("already exists"), "got: {err}");
}

#[test]
fn projection_show_renders_populated_projection() {
    use crate::layer::Layer;
    use crate::model::projection::{Component, SystemProjection};
    let store = store_with_spec("checkout");
    let mut p = SystemProjection::new("checkout");
    p.component.push(Component { name: "billing".into(), layer: Layer::Domain });
    store.save_projection(&p).unwrap();

    let out = projection_show(&store, &store, "checkout").unwrap();
    assert!(out.contains("Projection: checkout"), "got: {out}");
    assert!(out.contains("Components (1)"), "got: {out}");
    assert!(out.contains("billing"), "got: {out}");
}

#[test]
fn projection_show_renders_empty_sections_as_none() {
    let store = store_with_spec("checkout");
    store.save_projection(&crate::model::projection::SystemProjection::new("checkout")).unwrap();
    let out = projection_show(&store, &store, "checkout").unwrap();
    assert!(out.contains("Components (0)"), "got: {out}");
    assert!(out.contains("(none)"), "got: {out}");
}

#[test]
fn projection_show_bails_when_absent() {
    let store = store_with_spec("checkout");
    let err = projection_show(&store, &store, "checkout").unwrap_err();
    assert!(err.to_string().contains("no projection for checkout"), "got: {err}");
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test --lib projection_`
Expected: FAIL to compile — `projection_init` / `projection_show` not defined.

- [ ] **Step 4: Add imports and the use-cases**

In `src/app.rs`, extend the `use crate::ports::{…}` block to include `ProjectionRepo`, and add:

```rust
use crate::model::projection::SystemProjection;
```

Then add the two functions and the renderer (place them after `spec_new`):

```rust
/// Author an empty system projection for an existing spec session.
pub fn projection_init<S: SettingsRepo, R: SpecRepo, P: ProjectionRepo>(
    settings: &S,
    specs: &R,
    projections: &P,
    spec: &str,
) -> anyhow::Result<()> {
    require_initialized(settings)?;
    // The spec is the FK target; it must exist first.
    specs
        .load_spec(spec)
        .with_context(|| format!("no spec '{spec}' — create it with `circuit spec new` first"))?;
    if projections.projection_exists(spec) {
        anyhow::bail!("a projection for {spec} already exists");
    }
    projections
        .save_projection(&SystemProjection::new(spec))
        .with_context(|| format!("writing projection {spec}"))?;
    Ok(())
}

/// Render a plain-text summary of a spec session's projection.
pub fn projection_show<S: SettingsRepo, P: ProjectionRepo>(
    settings: &S,
    projections: &P,
    spec: &str,
) -> anyhow::Result<String> {
    require_initialized(settings)?;
    let p = projections
        .load_projection(spec)
        .with_context(|| format!("no projection for {spec} — run `circuit projection init {spec}`"))?;
    Ok(render_projection(&p))
}

/// Pure text renderer for a system projection. Empty sections render `(none)`.
fn render_projection(p: &SystemProjection) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "Projection: {}", p.spec);

    let _ = writeln!(out, "Components ({}):", p.component.len());
    if p.component.is_empty() {
        let _ = writeln!(out, "  (none)");
    }
    for c in &p.component {
        let _ = writeln!(out, "  - {} [{:?}]", c.name, c.layer);
    }

    let _ = writeln!(out, "Edges ({}):", p.edge.len());
    if p.edge.is_empty() {
        let _ = writeln!(out, "  (none)");
    }
    for e in &p.edge {
        let _ = writeln!(out, "  - {} -> {}", e.from, e.to);
    }

    let _ = writeln!(out, "Contexts ({}):", p.context.len());
    if p.context.is_empty() {
        let _ = writeln!(out, "  (none)");
    }
    for c in &p.context {
        let _ = writeln!(out, "  - {}", c.name);
    }

    let _ = writeln!(out, "Relationships ({}):", p.relationship.len());
    if p.relationship.is_empty() {
        let _ = writeln!(out, "  (none)");
    }
    for r in &p.relationship {
        let _ = writeln!(out, "  - {} -> {} ({})", r.upstream, r.downstream, r.kind);
    }

    let _ = writeln!(out, "Contracts ({}):", p.contract.len());
    if p.contract.is_empty() {
        let _ = writeln!(out, "  (none)");
    }
    for c in &p.contract {
        let _ = writeln!(out, "  - {} [{}] -> {}", c.name, c.provider, c.consumers.join(", "));
    }

    out
}
```

(`use std::fmt::Write as _;` is already imported at the top of `app.rs`, so `writeln!` on a `String` works.)

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --lib projection_`
Expected: PASS (6 new tests).

- [ ] **Step 6: Commit**

```bash
git add src/app.rs
git commit -m "feat(m3): add projection_init/projection_show use-cases"
```

---

### Task 5: CLI glue — `circuit projection init|show`

Wire the use-cases to a clap subcommand group.

**Files:**
- Modify: `src/main.rs` (add `Command::Projection`, `ProjectionCommand`, dispatch arm, two `run_projection_*` fns)
- Test: covered by Task 6 (integration).

**Interfaces:**
- Consumes: `circuit::app::projection_init`, `circuit::app::projection_show`.

- [ ] **Step 1: Add the `Command::Projection` variant**

In `src/main.rs`, inside `enum Command`, add (after the `Spec` variant for grouping):

```rust
    /// System-level projection commands
    Projection {
        #[command(subcommand)]
        command: ProjectionCommand,
    },
```

- [ ] **Step 2: Add the `ProjectionCommand` enum**

In `src/main.rs`, after `enum SpecCommand { … }`:

```rust
#[derive(Subcommand)]
enum ProjectionCommand {
    /// Create a skeleton system projection for an existing spec session
    Init {
        /// Spec id the projection attaches to (used as the filename)
        spec: String,
        #[arg(long, default_value = ".")]
        path: PathBuf,
    },
    /// Print a spec session's projection as a text summary
    Show {
        /// Spec id whose projection to show
        spec: String,
        #[arg(long, default_value = ".")]
        path: PathBuf,
    },
}
```

- [ ] **Step 3: Add the dispatch arm**

In `src/main.rs`, in the `match` inside `main` (next to `Command::Spec { command } => run_spec(command),`):

```rust
        Command::Projection { command } => run_projection(command),
```

- [ ] **Step 4: Add the `run_projection` glue**

In `src/main.rs`, after `run_spec`:

```rust
fn run_projection(command: ProjectionCommand) -> Result<()> {
    match command {
        ProjectionCommand::Init { spec, path } => {
            let ws = Workspace::new(&path);
            require_initialized(&ws)?;
            circuit::app::projection_init(&ws, &ws, &ws, &spec)?;
            println!("Created projection for spec: {spec}");
            Ok(())
        }
        ProjectionCommand::Show { spec, path } => {
            let ws = Workspace::new(&path);
            require_initialized(&ws)?;
            let out = circuit::app::projection_show(&ws, &ws, &spec)?;
            print!("{out}");
            Ok(())
        }
    }
}
```

- [ ] **Step 5: Verify it compiles**

Run: `cargo build`
Expected: builds clean (no warnings about unused variants).

- [ ] **Step 6: Commit**

```bash
git add src/main.rs
git commit -m "feat(m3): wire projection init|show CLI verbs"
```

---

### Task 6: Integration test — exit-criteria walk

Drive the binary end-to-end: init → spec → projection init → projection show, the slice's exit criteria.

**Files:**
- Create: `tests/projection.rs`

**Interfaces:**
- Consumes: the `circuit` binary (via `assert_cmd`).

- [ ] **Step 1: Write the integration test**

Create `tests/projection.rs`:

```rust
use assert_cmd::Command;
use predicates::prelude::*;

/// Exit-criteria walk for M3 slice A: init the workspace, create a spec session,
/// init its projection, then `projection show` round-trips the skeleton.
#[test]
fn projection_init_then_show_round_trips() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path();

    let run = |args: &[&str]| {
        Command::cargo_bin("circuit")
            .unwrap()
            .args(args)
            .current_dir(path)
            .assert()
            .success();
    };

    run(&["init"]);
    run(&["spec", "new", "checkout", "--title", "Checkout", "--intent", "Pay."]);
    run(&["projection", "init", "checkout"]);

    // The file landed where we expect.
    assert!(path.join(".circuit/projections/checkout.toml").exists());

    // `show` reports the skeleton honestly.
    Command::cargo_bin("circuit")
        .unwrap()
        .args(["projection", "show", "checkout"])
        .current_dir(path)
        .assert()
        .success()
        .stdout(predicate::str::contains("Projection: checkout"))
        .stdout(predicate::str::contains("Components (0)"))
        .stdout(predicate::str::contains("(none)"));
}

/// `projection init` refuses when the spec session does not exist.
#[test]
fn projection_init_without_spec_fails() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path();

    Command::cargo_bin("circuit")
        .unwrap()
        .args(["init"])
        .current_dir(path)
        .assert()
        .success();

    Command::cargo_bin("circuit")
        .unwrap()
        .args(["projection", "init", "ghost"])
        .current_dir(path)
        .assert()
        .failure()
        .stderr(predicate::str::contains("no spec 'ghost'"));
}
```

- [ ] **Step 2: Run the integration test**

Run: `cargo test --test projection`
Expected: PASS (2 tests).

- [ ] **Step 3: Run the full suite + fmt + clippy**

Run: `cargo test && cargo fmt --check && cargo clippy --all-targets -- -D warnings`
Expected: all green.

- [ ] **Step 4: Commit**

```bash
git add tests/projection.rs
git commit -m "test(m3): exit-criteria walk for projection init|show"
```

---

## Self-Review

**Spec coverage:**
- Schema (3 parts, lean fields, `Layer` reuse) → Tasks 1–2. ✔
- Storage keyed by spec id under `.circuit/projections/` → Task 3. ✔
- `ProjectionRepo` port (load/save/exists) → Task 3. ✔
- App `projection_init` (require-init, spec-exists, no-clobber) + `projection_show` (plain text, `(none)`) → Task 4. ✔
- CLI `projection init|show` → Task 5. ✔
- Exit criteria walk → Task 6. ✔
- Non-goals (no conformance, no mutation verbs, no `check`, no mermaid, no slice/UI projection) → none introduced. ✔

**Placeholder scan:** No TBD/TODO; every code step shows full code. ✔

**Type consistency:** `SystemProjection`, `Component { name, layer }`, `IntendedEdge { from, to }`, `Context { name }`, `Relationship { upstream, downstream, kind }`, `Contract { name, provider, consumers }`, and `load_projection`/`save_projection`/`projection_exists` names match across Tasks 2–6. Render field accesses (`c.name`, `c.layer`, `e.from`, `r.kind`, `c.provider`, `c.consumers`) match the Task 2 definitions. ✔
