# Circuit `map --html` — Interactive Layered Graph (Option-2) — Design

**Date:** 2026-07-18
**Status:** Approved (brainstorm), pre-plan
**Pillar:** Comprehension (M1.5 structural)
**Predecessors:**
- `docs/superpowers/specs/2026-06-20-circuit-comprehension-pillar-design.md` (§9 visual language — "layered interactive graph B", the daily surface)
- `docs/superpowers/specs/2026-06-29-circuit-map-layered-graph-design.md` (the deterministic `LayeredGraph` model + CLI/mermaid view this slice hydrates)

## 1. Summary

Add `circuit map <path> --html [--feature <selector>]`: the **self-contained, no-server,
interactive HTML artifact** that hydrates the exact `LayeredGraph` model shipped in the map
slice (PR #18). This is the comprehension pillar's "daily surface" (spec §9): an interactive
layered graph with zoom/pan, hover, and **click-to-light a feature's induced subgraph**, plus a
one-hop **drill to the real file**.

The whole slice **consumes the existing structural core unchanged** — `layered()`, `overlay()`,
and `comprehend()` — and adds only a presentation layer (an HTML renderer) plus one small IO pass
(`module → file` map). No LLM, no Tau. Determinism is preserved in Rust; the emitted JavaScript is
deliberately "dumb" (it looks up precomputed overlays, it does not recompute reachability).

Mermaid remains an export-only format (spec §9); this HTML surface is the interactive default.

## 2. Decisions locked (brainstorm)

| # | Decision | Rationale |
|---|----------|-----------|
| 1 | **Bespoke SVG column layout — zero vendored layout lib** (no dagre/ELK) | The model *already* assigns nodes to layer columns and fixes within-column order (name-sorted). dagre/ELK exist to solve layer-assignment + crossing-minimization — problems already solved in Rust. Vendoring ~100KB+ of JS for ~13 nodes in fixed columns is overkill. Bespoke layout is tiny, self-contained, deterministic, and trivially testable. Diverges from the spec's literal "dagre/ELK" wording, intentionally. |
| 2 | **Precompute every overlay in Rust; JS is dumb** | Keeps determinism in Rust; JS just toggles a CSS class on a precomputed id set. No reachability logic in the browser. |
| 3 | **Feature catalog = `comprehend()` groups, Main + Public only** (exclude Test) | Reuses the existing entry-point detector. Tests are not "features" and would swamp the dropdown on a test-heavy repo. Catalog opens on a "— none —" default (un-lit). |
| 4 | **View-model struct in `render/html.rs` + add `serde_json`** | Serialization is a presentation concern → lives in the renderer, not the pure core. Names are resolved to strings (no internal `ModuleId` leaks into JS). `serde` (derive) is already a dep; `serde_json` is a tiny, ubiquitous, audited addition scoped to the renderer. |
| 5 | **Minimal drill-to-code: path-as-text** (hover tooltip + click-to-pin), no editor launch | A static/downloaded HTML file cannot reliably launch an editor (`file://` is browser-blocked, `vscode://` is non-portable). Showing the copy-pasteable path is the honest v1 of the spec's "one hop to the real file". |
| 6 | **Emit to stdout** (redirect to a file), like `--mermaid` | Keeps the app fn returning a `String` → golden-testable; keeps the CLI thin. `circuit map . --html > map.html`. |

## 3. Architecture (hexagonal — dependencies point inward)

```
CLI    Command::Map { path, feature, mermaid, html }   ── html conflicts_with mermaid
         └─ run_map ──► html ? app::map_html : app::map

App    app::map_html(path: &Path, feature: Option<&str>) -> Result<String>
         ├─ builder::build_graph(path)                       -> ArchGraph          [#18 core, unchanged]
         ├─ scan::scan_functions(path) -> CallGraph::build   -> CallGraph          [#18 core, unchanged]
         ├─ layered::layered(&graph)                         -> LayeredGraph       [#18 core, unchanged]
         ├─ comprehension::comprehend(&decls).groups         -> catalog (Main+Public)
         ├─ for each catalog entry: layered::overlay(...)    -> FeatureOverlay     [#18 core, unchanged]
         └─ builder::module_files(path)                      -> BTreeMap<mod, Vec<rel>>   [NEW IO pass]

Render render::html::render(g, lg, catalog, files, initial) -> String
         └─ build MapView (Serialize) → serde_json::to_string → embed into a static shell
```

- **Pure core is consumed unchanged.** `layered.rs` and `overlay()` are not touched, and no
  `serde` derive is added to them. `EdgeDir` → JSON string mapping lives in the renderer.
- **New IO pass — `builder::module_files(root) -> Result<BTreeMap<String, Vec<String>>>`:** one
  `WalkDir` over `<root>/src` (or `<root>`), keyed by `module_name_from_rel(rel)`, values are the
  sorted, deduped relative `.rs` paths that contributed to that module. Mirrors the walk already in
  `build_graph`/`scan_functions`. Kept next to `build_graph` because it is the same IO shape.
- **Module-identity invariant (load-bearing, carried from #18):** a `CallGraph` node's `module`
  string equals its `ArchGraph` node name (both from `lang::module_name_from_rel`); the same string
  keys the `files` map. Feature → module → file resolution is a clean lookup with no identity drift.

## 4. Feature catalog + precomputed overlays

```rust
// derived in app::map_html, passed to the renderer
struct CatalogEntry {
    selector: String,        // FeatureGroup.entry, e.g. "app::run"  (also the overlay key)
    kind: EntryKind,         // Main | Public  (Test excluded)
    overlay: FeatureOverlay, // layered::overlay(&g, &calls, &selector, &lg)
}
```

- Catalog source: `comprehend(&decls).groups`, filtered to `EntryKind::{Main, Public}`, already
  sorted by `entry`.
- For each entry, call the existing `overlay(&g, &calls, &entry, &lg)`. Because the selector is a
  fully-qualified name, resolution is exact (one group ↔ one overlay). Empty overlays (no reachable
  modules) are still emitted so the dropdown entry exists but lights nothing.
- Dropdown ordering: `main` entries first, then `pub`, name-sorted within each kind. A leading
  "— none —" option is the default selection.

## 5. JSON payload (the contract between Rust and JS)

Emitted by `render::html::render` via a `#[derive(Serialize)]` view-model. Names resolved; every map
is a `BTreeMap` and every list is pre-sorted, so the payload is byte-stable.

```json
{
  "columns": [
    { "layer": "Adapter",     "modules": ["cli", "lang", "render"] },
    { "layer": "Application", "modules": ["app"] },
    { "layer": "Domain",      "modules": [] },
    { "layer": "Unknown",     "modules": ["builder", "comprehension", "graph"] }
  ],
  "edges": [
    { "from": "cli",    "to": "app", "dir": "inward" },
    { "from": "render", "to": "app", "dir": "outward" }
  ],
  "overlays": {
    "root::main": { "nodes": ["app", "cli", "comprehension"], "edges": [0, 3] }
  },
  "files": {
    "app": ["app.rs"],
    "render": ["render/html.rs", "render/mermaid.rs"]
  },
  "initial": "root::main"
}
```

- `columns` — fixed outside-in order; empty columns retained (rendered as an empty column slot).
- `edges` — sorted by `(from, to)`; `dir ∈ {inward, outward, lateral, unranked}` (outward = the
  dependency-rule violation `analyze` flags; styled distinctly, e.g. red).
- `overlays` — keyed by catalog selector; `nodes` are module names, `edges` are indices into the
  top-level `edges` array (so JS can light both endpoints and the connector).
- `files` — module → sorted relative `.rs` paths.
- `initial` — the `--feature` selector if it matched a catalog entry, else `null`.

## 6. The emitted file (bespoke SVG, no vendored libs)

- **Authoring:** the static shell (inline `<style>` + `<script>`) lives as a real, editable file at
  `src/render/html/template.html`, pulled in with `include_str!`. `render()` replaces a
  `/*__CIRCUIT_DATA__*/` token with the serialized JSON. Output is one self-contained document; the
  JS/CSS stays a first-class in-repo file (syntax highlighting, reviewable).
- **Layout (JS, deterministic):** x of a node = its column index; y = its sorted position within the
  column. Both are pure functions of the sorted payload — no randomness, no async layout engine.
  Connectors are drawn as SVG paths between the known node-box anchors; outward (violation) edges are
  styled distinctly.
- **Interaction (v1 scope):**
  - dropdown (catalog) select → toggle `.lit` on `overlays[selector].nodes` + `.edges`;
  - node **hover** → tooltip: `name · layer · file(s)`;
  - node **click** → pin that info in a detail panel (copy-pasteable path);
  - **pan/zoom** → drag + wheel over the SVG `viewBox`.
- **Empty / no-match:** if `--feature` doesn't match, the file still emits with `initial: null`
  (catalog present, nothing pre-lit). A repo with zero modules emits a valid, empty-state document.

## 7. CLI surface

```
$ circuit map . --html > map.html                 # interactive file, opens un-lit
$ circuit map . --html --feature root::main > map.html   # opens with root::main pre-lit
$ circuit map . --html --mermaid                  # ERROR: --html conflicts with --mermaid
```

- `Command::Map` gains `html: bool` with `#[arg(long, conflicts_with = "mermaid")]`.
- `run_map` dispatches to `app::map_html(path, feature)` when `html`, else the existing
  `app::map(path, feature, mermaid)`.
- `app::map` (text/mermaid) is unchanged; `app::map_html` is a new sibling that owns the extra
  catalog + `files` assembly (distinct enough to warrant its own function).

## 8. Testing (part of done)

- **Renderer unit (`render/html.rs`):**
  - `MapView` JSON contains `columns`, `edges`, `overlays` keyed by the catalog entries, with the
    right module names and `dir` strings;
  - the wrapper starts with `<!DOCTYPE html>`, contains a `<script>`, and the `/*__CIRCUIT_DATA__*/`
    token is fully replaced (absent in output) by the payload.
- **IO unit (`builder.rs`):** `module_files` on a temp repo returns the expected `module → paths`
  map, sorted and deduped.
- **App (`app.rs`):** `map_html` on Circuit's own repo emits a document containing the present layer
  labels and at least one overlay key.
- **CLI integration (`tests/cli.rs`):** `circuit map . --html` emits `<!DOCTYPE html>`;
  `circuit map . --html --mermaid` exits non-zero (clap conflict).
- **Flagged honestly:** the live SVG layout, pan/zoom, hover, and click-to-light **behaviors** are
  **manually verified, not unit-tested**. Only the emitted JSON payload and the HTML wrapper string
  are asserted. This is an accepted limitation of a static-file DOM/layout layer.

## 9. Determinism & caveats

- Everything sorted before render: fixed column order, modules by name, edges by `(from, to)`,
  every payload map is a `BTreeMap`. No `HashMap` iteration reaches output. The HTML byte-stream is
  stable across runs.
- JS layout positions are deterministic functions of the sorted payload, but this is presentational
  and **not asserted** by tests.
- **Layer caveat (carried, not fixed here):** `layer_of` only recognizes top-level conventional
  names, so nested modules land in `Unknown` — identical to `analyze`/`map`. No drive-by change.

## 10. Out of scope (explicit)

- Tri-pane catalog ⇄ map ⇄ code live application (Option-3).
- Function-level **trace ribbon** (spec option D) — a separate single-feature view.
- Force-directed clustering lens (spec option C) and any clustering pre-pass.
- Dynamic test-trace seeding.
- Editor-launch / deep-link drill (`file://`, `vscode://`).
- Any Tau / LLM involvement.

## 11. Done criteria

- `circuit map . --html` and `--html --feature <entry>` emit a valid, self-contained interactive
  document on Circuit's own repo and a fixture repo; opening it in a browser shows the layered graph
  with working zoom/pan, hover, click-to-light, and drill-to-path (manually verified).
- `--html --mermaid` is rejected by the CLI.
- New Rust code: pure/renderer units + IO unit + CLI integration all green; `cargo test` green;
  `cargo clippy --all-targets -- -D warnings` clean; `cargo fmt` clean.
- `analyze`, `map` (text), and `map --mermaid` output unchanged (byte-stable).
- Zero LLM calls; `serde_json` is the only new dependency.
