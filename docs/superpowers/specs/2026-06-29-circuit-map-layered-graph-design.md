# Circuit `map` — Layered Graph B (deterministic) — Design

**Date:** 2026-06-29
**Status:** Approved (brainstorm), pre-plan
**Pillar:** Comprehension (M1.5 structural)
**Predecessors:**
- `docs/superpowers/specs/2026-06-20-circuit-comprehension-pillar-design.md` (§9 visual language, defines "layered graph B")
- `docs/superpowers/specs/2026-06-20-circuit-impact-blast-radius-design.md` (the verb shape this mirrors)

## 1. Summary

Add `circuit map <path> [--feature <selector>] [--mermaid]`: the deterministic,
zero-LLM **layered graph B** from the comprehension pillar (spec §9). It fuses the
two existing structural substrates that have never been joined —

- `graph::ArchGraph` (modules + `layer::Layer` + dependency edges, from `analyze`), and
- `comprehension::callgraph::CallGraph` (functions + calls, from `comprehend`/`impact`) —

into one **module-level, layer-columned** view, with a `--feature` lens that lights up
the induced subgraph a feature's call-reachable functions span. Mermaid is emitted as an
**export format only**, never the daily surface (spec §9).

This slice delivers the **data model + CLI/mermaid view**. The interactive
HTML/dagre/ELK surface and the function-level "trace ribbon" are explicitly later,
separate slices (see §8).

## 2. Decisions locked (brainstorm)

| # | Decision | Rationale |
|---|----------|-----------|
| 1 | **Deliverable = deterministic model + CLI/mermaid view**, not the interactive frontend | Keeps thin-slice cadence; spec lists "MVP-A (deterministic): layered graph B" as first deliverable; interactive shell hydrates this model later |
| 2 | **Node grain = module-level** | Layers are natively a module concept (`layer_of`); ~10–15 nodes keeps mermaid legible (spec's ~20 ceiling); function detail already lives in `comprehend`/`impact` |
| 3 | **Feature overlay = induced subgraph**, not a single linear path | Features branch; honest + deterministic. The linear "ribbon" is spec option D, a separate function-level view |
| 4 | **New verb `map`**, sibling to `comprehend`/`impact` | Comprehension verbs are deliberately separate from `analyze`; the map *fuses* feature data `analyze` doesn't touch; keeps `analyze` a stable audit gate |
| 5 | **Column reading order = outside-in** (Adapter → Application → Domain) | Reads like a call entering from the CLI and descending to the core; "arrows point inward to the domain core" (spec §9) |

## 3. Architecture (hexagonal — mirrors `impact`)

- **Core (pure, deterministic):** new `src/comprehension/layered.rs`. Consumes
  `graph::ArchGraph`, `layer::{Layer, rank}`, `callgraph::CallGraph`. No new deps, zero LLM.
- **App (use-case + IO):** `app::map(path: &Path, feature: Option<&str>, mermaid: bool) -> Result<String>`.
  Owns scan/build IO (reuse `builder::build_graph` + `comprehension::scan::scan_functions`),
  calls the pure core, renders.
- **CLI (thin):** `Command::Map { path, feature: Option<String>, mermaid: bool }` → `run_map`.
- **Export:** `render::mermaid::render_layered(&LayeredGraph, Option<&FeatureOverlay>) -> String`.

**Module-identity invariant (load-bearing):** both `scan_functions` and `build_graph`
derive module names via the same `lang::module_name_from_rel`, and `build_graph`
`ensure_module`s every file's module. Therefore a `CallGraph` node's `module` string is
*exactly* its `ArchGraph` node name — overlay mapping is a clean `ArchGraph::module_id`
lookup with no identity drift, and every function's module is guaranteed to be a graph node.

## 4. Core model

```rust
pub struct LayeredGraph {
    pub columns: Vec<LayerColumn>,   // fixed outward→inward order; see §2.5
    pub edges: Vec<LgEdge>,          // sorted by (from, to)
}

pub struct LayerColumn {
    pub layer: Layer,
    pub modules: Vec<ModuleId>,      // sorted by module name
}

pub struct LgEdge {
    pub from: ModuleId,
    pub to: ModuleId,
    pub dir: EdgeDir,
}

pub enum EdgeDir { Inward, Outward, Lateral, Unranked }

pub fn layered(g: &ArchGraph) -> LayeredGraph;
```

- **Columns:** bucket `g.modules()` by their existing `layer`, emit in fixed order
  `[Adapter, Application, Domain, Unknown]` (outside-in; `Unknown` last). Modules sorted
  by name within each column. Empty columns are retained (rendered as `(none)`).
- **Edge direction** via `layer::rank` (Domain=1, Application=2, Adapter=3, Unknown=None):
  - `Inward`  — `rank(to) < rank(from)` (toward the core; the rule-abiding direction)
  - `Outward` — `rank(to) > rank(from)` (**dependency-rule violation** — same rule `analyze` flags)
  - `Lateral` — `rank(to) == rank(from)`
  - `Unranked` — either endpoint is `Unknown`

## 5. Feature overlay (`--feature <selector>`)

```rust
pub struct FeatureOverlay {
    pub selector: String,
    pub modules: Vec<ModuleId>,      // sorted, deduped; modules the feature's reachable fns live in
    pub edges: Vec<usize>,           // indices into LayeredGraph.edges, both endpoints in `modules`
}

pub fn overlay(g: &ArchGraph, calls: &CallGraph, target: &str, lg: &LayeredGraph) -> FeatureOverlay;
```

- Resolve `target` **exactly like `impact`**: match `node.name == target || node.qualified() == target`;
  **union** all matches (the by-name graph cannot disambiguate — honest, never silently picks one).
- For each matched function: `calls.reachable(id)` → `qualified()` → take the `module` segment →
  `g.module_id(module)`. Union into `modules` (sorted, deduped).
- **Induced edges:** indices of `LayeredGraph.edges` whose `from` and `to` are both in `modules`.
- **No match:** `modules` empty; CLI prints `no function matches '<selector>'` (mirrors `impact`).

Rationale: a "feature" is the downstream reachable footprint of its entry (matches `comprehend`'s
entry→members), so the overlay uses the forward (callee) cone, not the reverse cone.

## 6. CLI surface & rendering

```
$ circuit map .
layers (inward →)
  [Adapter]      cli  lang  render
  [Application]  app
  [Domain]       (none)
  [Unknown]      builder  comprehension  graph  indicators  layer  scan
edges: 23  (inward 18 · lateral 3 · outward/violation 2 · unranked 0)
  ⚠ render → app   (outward — dependency-rule violation)
```

```
$ circuit map . --feature root::main
layers (inward →)   ( * = on feature 'root::main' )
  [Adapter]      cli*  lang  render
  [Application]  app*
  [Domain]       (none)
  [Unknown]      builder  comprehension*  graph  indicators  layer  scan
feature · root::main — spans 3 modules, 2 induced edges; crosses Adapter → Application → Unknown
```

```
$ circuit map . --mermaid
flowchart LR
  subgraph Adapter ... end
  subgraph Application ... end
  ...
%% --feature bolds the induced subgraph via classDef
```

- **Text render:** deterministic; fixed column order; modules sorted; `*` marks overlay membership;
  outward edges listed as `⚠` violations. When `--feature` selector has multiple matches, print the
  union note (same shape as `impact`'s multi-match note).
- **Mermaid:** `render_layered` emits one `subgraph` per non-empty layer, edges styled by `EdgeDir`,
  overlay bolded via `classDef`. Demoted to export — not printed unless `--mermaid`.

## 7. Determinism & caveats

- Everything sorted before render (fixed column order, modules by name, edges by `(from, to)`).
  No `HashMap` iteration reaches output. Same byte-stability discipline as `comprehend`/`impact`.
- **Layer caveat (carried, not fixed here):** `layer_of` only recognizes top-level conventional
  names, so nested modules (e.g. `comprehension::callgraph`) land in `Unknown`. This is identical
  to `analyze`'s current behavior. **No drive-by change to `layer_of`** in this slice; deepening
  layer inference is a future slice.

## 8. Out of scope (explicit)

- Interactive HTML / dagre / ELK surface (the Option-2 slice that hydrates this model).
- Function-level **trace ribbon** (spec option D) — a separate single-feature view.
- Drill-to-expand / two-level nesting (spec option C) — an interactive affordance.
- Improving `layer_of` / deepening layer inference.
- Any Tau / LLM involvement.

## 9. Testing (part of done)

- **Core units (`layered.rs`, fixtures):**
  - column bucketing + fixed outside-in order, empty columns retained;
  - each `EdgeDir` case (inward / outward / lateral / unranked);
  - `overlay` induced-subgraph correctness (modules + induced edges);
  - `overlay` no-match → empty; multi-match → union.
- **App (`app.rs`):** `app::map` on Circuit's own repo emits the present layer labels + an `edges:` line.
- **CLI integration (`tests/cli.rs`):**
  - `circuit map .` shows `[Adapter]`;
  - `circuit map . --feature <entry>` shows the `feature ·` trailer;
  - `circuit map . --mermaid` emits `flowchart` and `subgraph`.
- **Mermaid unit (`render/mermaid.rs`):** one `subgraph` per non-empty layer; overlay bolds its modules.

## 10. Done criteria

- `circuit map .`, `--feature`, and `--mermaid` all work on Circuit's own repo and a fixture repo.
- All new code: pure core unit-tested; `cargo test` green; `cargo clippy --all-targets -- -D warnings` clean; `cargo fmt` clean.
- `analyze` output unchanged (byte-stable).
- Zero LLM calls.
