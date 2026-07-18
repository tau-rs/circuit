# Circuit M3 — Slice C: Projection-Conformance Indicator

**Status:** Approved for planning
**Date:** 2026-06-29
**Milestone:** M3 (Projection engine) — slice C of four (A: system projection ✅ · B: slice projection · **C: conformance** · D: UI mockup)
**Companion to:** `2026-06-14-circuit-roadmap.md`, `2026-06-22-circuit-m3-slice-a-system-projection-design.md`

## Goal

Close the design-before-code loop: compare the code's **real** dependency graph
(M1's derived `ArchGraph`) against the **approved** system projection (Slice A's
`SystemProjection`), and report where the code broke a boundary the design
planned.

The vision's distinction (§11): *"Adding detail inside the plan is fine; breaking
a planned contract is not."* Concretely:

- A derived dependency between two **declared** components that the projection's
  `edge` allowlist does **not** sanction → a **broken contract** (red).
- Code (edges, modules) among parts the projection **did not declare** → **silent**
  (additive detail; outside the approved design surface).
- A declared component that maps to **no** module in the derived graph → **uncovered**
  (can't check) → surfaced as Unknown, **never** reported as Sound.

This is genuine *contract* conformance, not a re-skin of the dependency rule: it
judges the projection's specific promises (its allowed edges), which are not
derivable from layers alone.

## Non-goals

Explicit scope guards — each is a later slice or a deliberate follow-on:

- **No context/contract semantics.** Gating cross-context edges by declared
  `contract`/`relationship` needs component↔context membership the schema lacks.
  Deferred until the model grows that link. This slice uses only `component` +
  `edge`.
- **No slice-level (per-DAG-node) conformance.** Diffing an impl session's slice
  projection against its branch depends on Slice B. This slice is **system-level**
  only (the spec's projection vs the whole repo graph).
- **No wiring into `SessionHealth.rollup()` / the cockpit roll-up.** That rollup
  has a deliberate invariant — it never yields `Unknown` except when the adapter
  supplies it for an absent worktree (`cockpit/health.rs`). Conformance's `Unknown`
  comes from a different source (an unmapped component), so folding the two is a
  separate, careful follow-on. Slice C surfaces its verdict through its own CLI
  verb, reusing the `Health` ladder.
- **No layer-mismatch check, and no fueling the dependency rule with projected
  layers.** Using `component.layer` to resolve `Unknown` modules and widen the
  existing `dependency_rule` indicator is a legitimate but separate enhancement to
  *that* indicator — not part of conformance.
- **No auto-fix, no mermaid overlay of violations, no `--json`.**

## Architecture

Pure hexagonal extension. One additive schema field, one new pure indicator
module mirroring `indicators/dependency_rule.rs`, one app-layer use-case, one CLI
verb. The check is a pure function over two in-memory values (`ArchGraph` built by
M1's `builder`, `SystemProjection` loaded by Slice A's `ProjectionRepo`); all IO
stays at the edges.

```
main.rs (clap glue)            app.rs (port-generic)              model + indicators + adapters
──────────────────            ──────────────────────             ──────────────────────────────
conformance <spec> ─► run_conformance ─► app::conformance ─┐
                                                            ├─► SettingsRepo (require init)
                                                            ├─► ProjectionRepo.load_projection ─► SystemProjection
                                                            ├─► builder::build_graph(path)     ─► ArchGraph
                                                            └─► indicators::conformance::check(&graph, &proj) ─► Conformance
```

### Files touched

| File | Change |
|---|---|
| `src/model/projection.rs` | Add optional `Component.module: Option<String>` + `Component::effective_module()`; extend tests |
| `src/indicators/conformance.rs` | **NEW** — `Conformance`, `BrokenEdge`, `check()`, `Conformance::health()`; unit tests |
| `src/indicators/mod.rs` | Add `pub mod conformance;` |
| `src/app.rs` | Add `conformance` use-case returning a `Conformance` (+ a render helper or reuse); tests |
| `src/main.rs` | Add `Command::Conformance { spec, path }`; `run_conformance`; dispatch arm |
| `tests/conformance.rs` | **NEW** — integration exit-criteria walk |

## Data model — `src/model/projection.rs`

The design-name ↔ code-module join, declared rather than guessed. The derived
graph names modules by their top-level path segment (`module_name_from_rel` →
`model`, `adapters`, `flow`…), which rarely equals a design component name
(`billing`, `cart`). Without an explicit link the check would join on a false
hope and sit green. The field is additive and optional; when omitted, the join
falls back to the component `name` (so a projection whose component names already
match module names needs no `module` field).

```rust
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Component {
    pub name: String,
    pub layer: Layer,
    /// Which derived code module realizes this component (top-level module name,
    /// e.g. "model"). `None` ⇒ join on `name`. The design name and the code
    /// module live in different namespaces, so this link must be declared.
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

```toml
[[component]]
name   = "billing"
layer  = "domain"
module = "model"      # ← NEW (optional)
```

Back-compat: existing Slice A projections (no `module` key) parse unchanged and
join on `name`.

## Indicator — `src/indicators/conformance.rs`

Mirrors `dependency_rule.rs`: a pure function over `&ArchGraph` (plus the
projection), returning a sorted, deterministic result; never fakes a verdict.

```rust
use crate::graph::ArchGraph;
use crate::model::projection::SystemProjection;
use crate::cockpit::health::Health;

/// A derived edge between two declared components that the projection's `edge`
/// allowlist does not sanction — a broken planned boundary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BrokenEdge {
    /// Component names (design vocabulary), not module names.
    pub from: String,
    pub to: String,
    /// The derived modules they map to (for the message).
    pub from_module: String,
    pub to_module: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Conformance {
    pub violations: Vec<BrokenEdge>,
    /// Declared component names whose `effective_module` is not a node in the graph.
    pub uncovered: Vec<String>,
}

/// Diff reality (graph) against intent (projection).
pub fn check(graph: &ArchGraph, proj: &SystemProjection) -> Conformance { /* see rules */ }

impl Conformance {
    /// Verdict on the existing ladder. Reused, NOT wired into SessionHealth here.
    pub fn health(&self) -> Health {
        if !self.violations.is_empty() { Health::Critical }
        else if !self.uncovered.is_empty() { Health::Unknown }
        else { Health::Sound }
    }
}
```

### Rules (precise)

Build two derived structures from the projection:

- `module_of_component: name → effective_module` for every declared component.
- `declared_modules: Set<effective_module>` (the code modules under design control).
- `allowed: Set<(from_module, to_module)>` — translate each projection `edge`
  (which names **components**) into the module pair via `module_of_component`.
  An edge naming a component that doesn't exist is ignored for the allowlist
  (authoring slip, not a code violation; covered by Slice A-style validation later).

Then, for each derived edge `(a_mod, b_mod)` in `graph.edges()`:

- if **both** `a_mod` and `b_mod` ∈ `declared_modules` **and** `(a_mod, b_mod)` ∉
  `allowed` → push a `BrokenEdge` (look up the component name(s) for the message;
  if two components map to the same module, use the first by sorted name —
  deterministic).
- otherwise → ignored (at least one end is undeclared ⇒ additive detail).

`uncovered` = declared component names whose `effective_module` is not returned by
`graph.module_id(..)`. Both vectors sorted for determinism (mirrors
`dependency_rule::violations`).

A projection `edge` present in `allowed` but absent from the graph → **silent**
(planned-but-not-yet-built; not a violation).

## Application layer — `src/app.rs`

Port-generic, `anyhow` internally, no printing — matches `projection_show`.

```rust
/// Compute system-projection conformance for a spec against a repo worktree.
pub fn conformance<S: SettingsRepo, P: ProjectionRepo>(
    settings: &S, projections: &P, spec: &str, path: &Path,
) -> anyhow::Result<Conformance> {
    require_initialized(settings)?;
    let proj = projections.load_projection(spec)
        .with_context(|| format!("no projection for {spec} — run `circuit projection init {spec}`"))?;
    let graph = crate::builder::build_graph(path)?;   // M1 IO adapter
    Ok(crate::indicators::conformance::check(&graph, &proj))
}
```

A pure `render_conformance(&Conformance) -> String` helper (in `app.rs`, like
`render_projection`) formats the report: violation count, each broken edge
(`from [module] -> to [module]`), and uncovered components; `(none)` when empty.
`main.rs` prints it.

## CLI surface — `src/main.rs`

```
circuit conformance <spec> [--path .]
```

`run_conformance` builds a `Workspace`, calls `app::conformance`, prints the
rendered report, and **exits non-zero when `violations` is non-empty** (so it
gates CI), mirroring `dag check`'s `std::process::exit(1)` pattern. Uncovered
components print as a warning section but do **not** by themselves fail the exit
code (Unknown ≠ broken) — they are surfaced honestly, not treated as a failure.

## Error handling

- Store boundary: reuse `ModelError` via `ProjectionRepo`; `build_graph` already
  returns `anyhow::Result`.
- App layer: `anyhow::bail!`/`.with_context` for not-initialized and
  projection-absent, mirroring `projection_show`.
- No new error type.

## Worked example

Projection `checkout`: components `billing(domain, module=model)`,
`gh-adapter(adapter, module=adapters)`; allowed `edge: gh-adapter → billing`
(i.e. module pair `adapters → model`).

Derived graph edges: `adapters → model`, `adapters → flow`, `model → render`.

```
adapters → model    both declared, allowed       → OK
adapters → flow     flow undeclared              → silent (additive)
model    → render   render undeclared            → silent (additive)
```
→ `Conformance { violations: [], uncovered: [] }`, `health() = Sound`.

Now the code adds `model → adapters` (both declared, not in allowlist):
→ `violations: [BrokenEdge{ from:"billing", to:"gh-adapter", from_module:"model", to_module:"adapters" }]`,
`health() = Critical`, CLI exits non-zero.

If the projection also declares `cart(domain, module=cart)` but no `cart` module
exists: → `uncovered: ["cart"]`, `health() = Unknown` (had there been no
violation), CLI prints the uncovered warning, exit code 0.

## Testing — part of done

- **model:** `effective_module` returns `module` when set, falls back to `name`
  when `None`; a projection with `module` round-trips; a Slice A projection
  without `module` still parses.
- **indicator (`conformance.rs`):** allowed edge → no violation; forbidden edge
  between two declared components → one `BrokenEdge`; edge touching an undeclared
  module → silent; declared component with no matching module → `uncovered`;
  projected edge absent from code → silent; `health()` precedence
  (Critical > Unknown > Sound); determinism (sorted output).
- **app:** `conformance` happy path (Sound); bails when projection absent; returns
  a populated `Conformance` for a graph with a broken edge (use a tempdir repo +
  authored projection, mirroring the `build_graph` temp-repo tests).
- **integration (`tests/conformance.rs`):** exit-criteria walk — `init` → `spec new`
  → `projection init` → author a projection (write the TOML) over a tiny temp repo
  → `circuit conformance <spec>` prints the verdict; a forbidden edge yields a
  non-zero exit.

## Exit criteria

Given an approved system projection and a repo, `circuit conformance <spec>`
reports **zero** violations when the code's cross-component dependencies all sit
within the projection's `edge` allowlist (additive code among undeclared modules
stays silent), reports a **broken-contract** violation (and non-zero exit) when
the code introduces a dependency between two declared components the design didn't
allow, and reports **uncovered** (Unknown, not Sound) for any declared component
the code doesn't yet realize — never a false green.
