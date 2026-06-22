# Circuit M3 — Slice A: System-Level Projection Schema & Spec Attachment

**Status:** Approved for planning
**Date:** 2026-06-22
**Milestone:** M3 (Projection engine) — slice A of four (A: system projection · B: slice projection · C: conformance · D: UI mockup)
**Companion to:** `2026-06-14-circuit-roadmap.md`, `2026-06-14-circuit-vision-design.md`

## Goal

Establish the **system-level projection** as a first-class authored artifact under
`.circuit/`, attached to a spec session and round-tripped through TOML. A spec
session's projection captures the *shape* of the solution before any code is
written — the surface the developer steers at (vision §4.1, §4.2). This slice
delivers the schema, persistence, and a minimal author/inspect CLI. It is the
foundation the **projection-conformance indicator** (Slice C) later diffs the
derived code graph against.

Per the vision (§4.2), a system-level projection holds three things:

1. **Architecture** — intended layers and the allowed dependency edges between them.
2. **Context map** — the bounded contexts and the relationships among them.
3. **Inter-slice contracts** — named ports between contexts/slices.

## Non-goals

Explicit scope guards — each is a later slice or a deferred concern:

- **No conformance / no diff against the derived graph.** Comparing projection to
  reality (dependency-rule violations, broken contracts) is **Slice C**. This slice
  only authors and round-trips intent.
- **No granular mutation verbs** (`add-component`, `add-edge`, …). YAGNI — a
  projection is authored as a coherent document via hand-edited TOML, unlike the
  DAG which grows edge-by-edge. Mutation verbs are added only when a need is shown.
- **No `projection check` validation.** Intent-level validation (refs resolve,
  contexts align with `SpecRecord.bounded_contexts`) is deferred — a candidate
  Slice A.2 or folded into Slice C.
- **No mermaid render.** `projection show` is plain text; diagram rendering of the
  architecture/context map is deferred.
- **No slice-level (impl-session) projection.** That is **Slice B**.
- **No UI mockup / `has-ui` gating.** That is **Slice D**.

## Architecture

Pure hexagonal extension, mirroring the M2 authored-artifact pattern
(`SpecRecord`/`SpecRepo`, `DagNode`/`DagRepo`). A new model type, a new outbound
port, a `Workspace` adapter impl, two port-generic app functions, and clap glue.
Dependencies point inward; the app layer is generic over traits and does no IO or
printing.

```
main.rs (clap glue)              app.rs (port-generic)              model + ports/adapters
─────────────────                ──────────────────────             ───────────────────────
projection init <spec> ─► run_projection_init ─► app::projection_init ─┐
projection show <spec> ─► run_projection_show ─► app::projection_show ─┴─► SpecRepo (verify spec exists)
                                                                          ProjectionRepo (load/save/exists)  ← NEW
                                                                          model::projection::SystemProjection ← NEW
```

### Files touched

| File | Change |
|---|---|
| `src/model/projection.rs` | **NEW** — `SystemProjection` + `Component` / `IntendedEdge` / `Context` / `Relationship` / `Contract`; `SystemProjection::new(spec)`; round-trip + defaults tests |
| `src/model/mod.rs` | Add `pub mod projection;` |
| `src/ports.rs` | **NEW** `ProjectionRepo` trait: `load_projection`, `save_projection`, `projection_exists` |
| `src/adapters/store.rs` | `Workspace`: `projections_dir`, `projection_path`, `load_projection`, `save_projection`, `projection_exists`; `impl ProjectionRepo for Workspace`; disk round-trip test |
| `src/app.rs` | `projection_init` + `projection_show` (returns a render `String`); guard tests |
| `src/main.rs` | `Command::Projection { command: ProjectionCommand }`; `ProjectionCommand::{Init, Show}`; `run_projection_init` / `run_projection_show`; dispatch arm |

## Data model — `src/model/projection.rs`

One file per spec, keyed by spec id, under `.circuit/projections/<spec-id>.toml`.
This mirrors `.circuit/specs/` and `.circuit/dag/`: spec.toml stays lean, the
projection diffs cleanly in PRs, and the conformance engine (Slice C) has one
well-known file to diff against. The `spec` field is the foreign key to
`SpecRecord.id`, exactly like `DagNode.spec` — this is the "attachment to the spec
session."

```rust
use serde::{Deserialize, Serialize};
use crate::layer::Layer;

/// `.circuit/projections/<spec-id>.toml` — a spec session's system-level
/// projection: the intended architecture, context map, and inter-slice contracts.
/// Authored intent only; never diffed against code in this slice (that is M3-C).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SystemProjection {
    pub schema_version: u32,
    /// Spec session id this projection belongs to (FK → `SpecRecord.id`).
    pub spec: String,
    #[serde(default)] pub component: Vec<Component>,
    #[serde(default)] pub edge: Vec<IntendedEdge>,
    #[serde(default)] pub context: Vec<Context>,
    #[serde(default)] pub relationship: Vec<Relationship>,
    #[serde(default)] pub contract: Vec<Contract>,
}

/// An intended module/component and the layer it is meant to live in. `layer`
/// reuses M1's `Layer` so Slice C can diff projected layers against derived ones.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Component { pub name: String, pub layer: Layer }

/// An intended (allowed) dependency edge. Slice C diffs code edges against these.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntendedEdge { pub from: String, pub to: String }

/// A bounded context in the context map.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Context { pub name: String }

/// A directed relationship between two contexts. `kind` is a free string
/// (e.g. "customer-supplier", "conformist", "acl"), NOT a closed enum — the DDD
/// vocabulary is added only when a need is demonstrated (YAGNI).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Relationship { pub upstream: String, pub downstream: String, pub kind: String }

/// A named inter-slice contract (a port one context provides to others).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Contract { pub name: String, pub provider: String, #[serde(default)] pub consumers: Vec<String> }

impl SystemProjection {
    /// A v1 skeleton: identity only, all sections empty.
    pub fn new(spec: impl Into<String>) -> Self {
        Self { schema_version: 1, spec: spec.into(),
               component: Vec::new(), edge: Vec::new(), context: Vec::new(),
               relationship: Vec::new(), contract: Vec::new() }
    }
}
```

Every section vec is `#[serde(default)]`, so a skeleton file (only
`schema_version` + `spec`) and any partially-authored projection parse. Example
of a fully-authored file:

```toml
schema_version = 1
spec = "checkout"

[[component]]
name  = "billing"
layer = "domain"
[[component]]
name  = "gh-adapter"
layer = "adapter"

[[edge]]
from = "gh-adapter"
to   = "billing"

[[context]]
name = "checkout"
[[context]]
name = "payments"

[[relationship]]
upstream   = "payments"
downstream = "checkout"
kind       = "customer-supplier"

[[contract]]
name      = "PaymentGateway"
provider  = "payments"
consumers = ["checkout"]
```

> **Layer dependency (confirmed required):** `Component.layer` reuses
> `crate::layer::Layer`, which today is a plain enum (`Domain`/`Application`/
> `Adapter`/`Unknown`) with **no serde derives**. The implementation must add
> `#[derive(Serialize, Deserialize)]` + `#[serde(rename_all = "lowercase")]` to
> `Layer` so it serializes as `"domain"`/`"application"`/`"adapter"`/`"unknown"`,
> matching the `layer = "domain"` TOML above. This is a minimal, in-scope change to
> the type being reused — not a refactor — and a round-trip test for `Layer` should
> accompany it.

## Port — `src/ports.rs`

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

## Adapter — `src/adapters/store.rs`

`Workspace` gains, following the spec/dag methods already present:

```rust
pub fn projections_dir(&self) -> PathBuf { self.circuit_dir().join("projections") }
pub fn projection_path(&self, spec: &str) -> PathBuf { self.projections_dir().join(format!("{spec}.toml")) }
pub fn load_projection(&self, spec: &str) -> Result<SystemProjection, ModelError> { load_toml(&self.projection_path(spec)) }
pub fn save_projection(&self, p: &SystemProjection) -> Result<(), ModelError> { save_toml(&self.projection_path(&p.spec), p) }
pub fn projection_exists(&self, spec: &str) -> bool { self.projection_path(spec).exists() }
```

…plus `impl ProjectionRepo for Workspace { type Error = ModelError; … }` delegating
to the inherent methods, exactly like the existing `SpecRepo`/`DagRepo` impls.

## Application layer — `src/app.rs`

Port-generic, `anyhow` internally, no IO/printing — matching `spec_new`.

```rust
/// Author an empty system projection for an existing spec session.
pub fn projection_init<S: SettingsRepo, R: SpecRepo, P: ProjectionRepo>(
    settings: &S, specs: &R, projections: &P, spec: &str,
) -> anyhow::Result<()> {
    require_initialized(settings)?;
    // The spec is the FK target; it must exist first.
    specs.load_spec(spec)
        .with_context(|| format!("no spec '{spec}' — create it with `circuit spec new` first"))?;
    if projections.projection_exists(spec) {
        anyhow::bail!("a projection for {spec} already exists");
    }
    projections.save_projection(&SystemProjection::new(spec))
        .with_context(|| format!("writing projection {spec}"))?;
    Ok(())
}

/// Render a plain-text summary of a spec session's projection.
pub fn projection_show<S: SettingsRepo, P: ProjectionRepo>(
    settings: &S, projections: &P, spec: &str,
) -> anyhow::Result<String> {
    require_initialized(settings)?;
    let p = projections.load_projection(spec)
        .with_context(|| format!("no projection for {spec} — run `circuit projection init {spec}`"))?;
    Ok(render_projection(&p)) // pure formatter; empty sections render as "(none)"
}
```

`render_projection` is a private pure helper producing a sectioned text summary
(counts + each component/edge/context/relationship/contract listed). `main.rs`
prints the returned `String`.

## CLI surface — `src/main.rs`

```
circuit
└── projection
    ├── init  <spec>   ← NEW   create .circuit/projections/<spec>.toml (skeleton)
    └── show  <spec>   ← NEW   print the projection as a text summary
```

A new `Command::Projection { command: ProjectionCommand }` group with `Init { spec, path }`
and `Show { spec, path }` variants, plus `run_projection_init` / `run_projection_show`
glue that builds a `Workspace`, calls the app function, and prints — following the
existing `run_spec` / `run_session_*` shape.

## Error handling

- **Store boundary:** reuse `ModelError` (IO / parse / serialize) — no new error type.
- **App layer:** `anyhow::bail!` + `.with_context()` for the three guard cases —
  not-initialized (`require_initialized`), spec-absent, projection-already-exists,
  projection-absent — consistent with `spec_new` / `session_spawn` / `session_pr`.

## Testing — part of done

- **model (`projection.rs`):**
  - full round-trip through TOML with all three sections populated;
  - skeleton (`SystemProjection::new`) round-trips with empty sections omitted;
  - hand-authored TOML with sections omitted parses (defaults to empty).
- **store (`store.rs`):** projection round-trips through disk; `projection_exists`
  is `false` before save and `true` after.
- **app (`app.rs`):** `projection_init` happy path; bails when the spec is absent;
  bails on clobber when a projection already exists; `projection_show` renders both
  a populated and an empty projection; bails when the projection is absent.

## Exit criteria

`circuit projection init <spec>` writes a skeleton `.circuit/projections/<spec>.toml`
for an existing spec session (and refuses when the spec is missing or a projection
already exists); a developer fills in components, edges, contexts, relationships,
and contracts by hand; `circuit projection show <spec>` round-trips and renders
them. All authored state is committed under `.circuit/`. No code-vs-projection
diffing yet — that lands in Slice C.
