# Circuit `map --html` Interactive Layered Graph — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `circuit map <path> --html [--feature <selector>]` — a self-contained, no-server, interactive HTML artifact that hydrates the existing `LayeredGraph` model with zoom/pan, hover, click-to-light-a-feature, and drill-to-file.

**Architecture:** A new presentation layer (`render::html`) serializes a resolved view-model to JSON via `serde_json` and embeds it into a static HTML/SVG/JS shell (bespoke layout, **no** dagre/ELK). The pure core (`layered.rs`, `overlay()`, `comprehend()`) is consumed unchanged; the only new backend code is one IO pass (`builder::module_files`) and the `app::map_html` use-case that assembles the catalog of precomputed overlays.

**Tech Stack:** Rust (clap, serde, serde_json, walkdir, anyhow), vanilla HTML/SVG/JS (no vendored libraries).

## Global Constraints

- `#![forbid(unsafe_code)]` is set crate-wide (`src/main.rs`) — no `unsafe`.
- Zero LLM / Tau calls. Deterministic output: every payload map is a `BTreeMap`, every list pre-sorted; no `HashMap` iteration reaches output.
- Only one new dependency permitted: `serde_json`. The pure core (`src/comprehension/layered.rs`, `src/graph.rs`) gains **no** `serde` derives — serialization lives only in `render/html.rs`.
- `analyze`, `map` (text), and `map --mermaid` output must stay byte-for-byte unchanged.
- Final gate (owned by Task 4): `cargo test`, `cargo clippy --all-targets -- -D warnings`, and `cargo fmt --check` all clean. Clippy/fmt run **once** at the final task, not per task.
- HTML is emitted to **stdout** (user redirects to a file). `--html` `conflicts_with` `--mermaid`.
- Module-identity invariant (carried from #18): a `CallGraph` node's `module` string equals its `ArchGraph` node name and the `files`-map key (all from `lang::module_name_from_rel`).

---

### Task 1: `builder::module_files` — module → source-file map

**Files:**
- Modify: `src/builder.rs` (add import + new pub fn + test)

**Interfaces:**
- Consumes: `walkdir::WalkDir`, `crate::lang::module_name_from_rel` (already imported in the file).
- Produces: `pub fn module_files(root: &Path) -> anyhow::Result<std::collections::BTreeMap<String, Vec<String>>>` — module name → sorted, deduped relative `.rs` paths (forward-slash). Consumed by Task 3.

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)] mod tests` block at the bottom of `src/builder.rs`:

```rust
    #[test]
    fn module_files_maps_modules_to_sorted_paths() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        std::fs::create_dir_all(src.join("app")).unwrap();
        std::fs::write(src.join("main.rs"), "fn main() {}").unwrap();
        std::fs::write(src.join("app/mod.rs"), "pub fn run() {}").unwrap();

        let m = module_files(dir.path()).unwrap();

        assert_eq!(m.get("root").unwrap(), &vec!["main.rs".to_string()]);
        assert_eq!(m.get("app").unwrap(), &vec!["app/mod.rs".to_string()]);
    }

    #[test]
    fn module_files_missing_path_is_an_error() {
        assert!(module_files(std::path::Path::new("/no/such/circuit/xyz")).is_err());
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib module_files`
Expected: FAIL — `cannot find function module_files in this scope`.

- [ ] **Step 3: Add the import and the function**

At the top of `src/builder.rs`, change the std import line to include the collections:

```rust
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
```

Then add this function immediately after `build_graph` (before the `#[cfg(test)]` block):

```rust
/// IO adapter: map each module name to the sorted, deduped relative `.rs`
/// paths that contribute to it. Mirrors `build_graph`'s walk; used by the
/// interactive HTML map for drill-to-file. Keys/values are deterministic.
pub fn module_files(root: &Path) -> Result<BTreeMap<String, Vec<String>>> {
    if !root.exists() {
        anyhow::bail!("path not found: {}", root.display());
    }
    let src_root = root.join("src");
    let base = if src_root.is_dir() {
        src_root
    } else {
        root.to_path_buf()
    };

    let mut map: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for entry in WalkDir::new(&base).into_iter().filter_map(|e| e.ok()) {
        let p = entry.path();
        if p.extension().and_then(|e| e.to_str()) == Some("rs") {
            let rel = p
                .strip_prefix(&base)
                .unwrap_or(p)
                .to_string_lossy()
                .replace('\\', "/");
            let module = module_name_from_rel(&rel);
            map.entry(module).or_default().insert(rel);
        }
    }
    Ok(map
        .into_iter()
        .map(|(k, v)| (k, v.into_iter().collect()))
        .collect())
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib module_files`
Expected: PASS (2 tests).

- [ ] **Step 5: Commit**

```bash
git add src/builder.rs
git commit -m "feat(comprehension): module_files IO map for drill-to-file"
```

---

### Task 2: `render::html` — view-model + HTML/SVG/JS renderer

**Files:**
- Create: `src/render/html.rs`
- Create: `src/render/html/template.html`
- Modify: `src/render/mod.rs` (register the module)
- Modify: `Cargo.toml` (add `serde_json`)

**Interfaces:**
- Consumes: `crate::comprehension::layered::{EdgeDir, FeatureOverlay, LayeredGraph}`, `crate::graph::{ArchGraph, ModuleId}`, `serde::Serialize`, `serde_json`.
- Produces: `pub fn render(g: &ArchGraph, lg: &LayeredGraph, overlays: &[(String, FeatureOverlay)], files: &std::collections::BTreeMap<String, Vec<String>>, initial: Option<&str>) -> String`. Consumed by Task 3.

- [ ] **Step 1: Add `serde_json` to Cargo.toml**

In `[dependencies]` of `Cargo.toml`, add after the `serde` line:

```toml
serde_json = "1"
```

- [ ] **Step 2: Register the module**

In `src/render/mod.rs`, add a line so it reads:

```rust
pub mod dag_board;
pub mod html;
pub mod mermaid;
```

- [ ] **Step 3: Create the static template**

Create `src/render/html/template.html` with EXACTLY this content (the `__CIRCUIT_DATA__` token is replaced at render time):

```html
<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Circuit — Layered Map</title>
<style>
  :root {
    --bg:#0f1115; --panel:#171a21; --ink:#e6e9ef; --muted:#9aa4b2;
    --line:#3a4150; --node:#1f2530; --node-br:#3a4150;
    --inward:#4f8cff; --outward:#ff5c5c; --lateral:#b28cff;
    --lit:#ffd257; --lit-bg:#2a2410;
  }
  * { box-sizing:border-box; }
  html,body { margin:0; height:100%; background:var(--bg); color:var(--ink);
    font:14px/1.4 ui-sans-serif,system-ui,-apple-system,Segoe UI,Roboto,sans-serif; }
  #bar { display:flex; align-items:center; gap:12px; padding:10px 14px;
    border-bottom:1px solid var(--line); background:var(--panel); }
  #bar h1 { font-size:14px; font-weight:600; margin:0; letter-spacing:.02em; }
  #bar .sp { flex:1; }
  select { background:var(--node); color:var(--ink); border:1px solid var(--node-br);
    border-radius:6px; padding:5px 8px; font:inherit; }
  #legend { display:flex; gap:14px; color:var(--muted); font-size:12px; }
  #legend i { display:inline-block; width:22px; height:0; border-top:2px solid;
    vertical-align:middle; margin-right:5px; }
  #stage { position:absolute; inset:52px 0 0 0; }
  svg { width:100%; height:100%; display:block; cursor:grab; }
  svg.panning { cursor:grabbing; }
  .node rect { fill:var(--node); stroke:var(--node-br); stroke-width:1.5; }
  .node text { fill:var(--ink); font-size:13px; }
  .node.lit rect { stroke:var(--lit); stroke-width:2.5; fill:var(--lit-bg); }
  .node { cursor:pointer; }
  .col-label { fill:var(--muted); font-size:12px; font-weight:600;
    letter-spacing:.08em; text-transform:uppercase; }
  path.edge { fill:none; stroke:var(--line); stroke-width:1.5; opacity:.55; }
  path.edge.inward { stroke:var(--inward); }
  path.edge.outward { stroke:var(--outward); stroke-dasharray:5 3; }
  path.edge.lateral { stroke:var(--lateral); }
  path.edge.lit { opacity:1; stroke:var(--lit); stroke-width:2.5; }
  #tip { position:absolute; pointer-events:none; background:#000c; color:#fff;
    border:1px solid var(--line); border-radius:6px; padding:6px 9px; font-size:12px;
    max-width:280px; display:none; z-index:10; }
  #tip b { color:var(--lit); }
  #tip .f { color:var(--muted); display:block; margin-top:3px; word-break:break-all; }
  #panel { position:absolute; right:14px; bottom:14px; width:260px; background:var(--panel);
    border:1px solid var(--line); border-radius:8px; padding:12px; display:none; z-index:9; }
  #panel h2 { margin:0 0 6px; font-size:13px; }
  #panel .k { color:var(--muted); font-size:12px; }
  #panel .path { font-family:ui-monospace,Menlo,monospace; font-size:12px; color:var(--ink);
    background:var(--node); border:1px solid var(--node-br); border-radius:5px;
    padding:4px 6px; margin-top:5px; word-break:break-all; }
  #panel button { position:absolute; top:8px; right:8px; background:none; border:none;
    color:var(--muted); cursor:pointer; font-size:14px; }
</style>
</head>
<body>
<div id="bar">
  <h1>Circuit · Layered Map</h1>
  <label>Feature <select id="feat"></select></label>
  <div class="sp"></div>
  <div id="legend">
    <span><i style="border-color:var(--inward)"></i>inward</span>
    <span><i style="border-color:var(--outward)"></i>violation</span>
    <span><i style="border-color:var(--lateral)"></i>lateral</span>
  </div>
</div>
<div id="stage">
  <svg id="svg"><g id="edges"></g><g id="nodes"></g></svg>
</div>
<div id="tip"></div>
<div id="panel"><button id="pclose">✕</button><div id="pbody"></div></div>
<script>
const DATA = __CIRCUIT_DATA__;

const COL_W = 240, ROW_H = 64, NODE_W = 168, NODE_H = 40, PAD = 40, HEAD = 34;
const NS = 'http://www.w3.org/2000/svg';
const el = (n, a) => { const e = document.createElementNS(NS, n);
  for (const k in a) e.setAttribute(k, a[k]); return e; };

// layout: position is a pure function of the sorted payload
const pos = {};
DATA.columns.forEach((col, ci) => {
  const x = PAD + ci * COL_W;
  col.modules.forEach((name, ri) => {
    const y = PAD + HEAD + ri * ROW_H;
    pos[name] = { x, y, w: NODE_W, h: NODE_H, cx: x + NODE_W / 2, cy: y + NODE_H / 2 };
  });
});

const svg = document.getElementById('svg');
const gEdges = document.getElementById('edges');
const gNodes = document.getElementById('nodes');

// arrowhead marker
const marker = el('marker', { id: 'ah', viewBox: '0 0 10 10', refX: 9, refY: 5,
  markerWidth: 7, markerHeight: 7, orient: 'auto-start-reverse' });
marker.appendChild(el('path', { d: 'M0 0 L10 5 L0 10 z', fill: '#8b93a1' }));
const defs = el('defs', {});
defs.appendChild(marker);
svg.insertBefore(defs, gEdges);

// column labels
DATA.columns.forEach((col, ci) => {
  const t = el('text', { x: PAD + ci * COL_W, y: PAD, class: 'col-label' });
  t.textContent = col.layer;
  gNodes.appendChild(t);
});

// edges
const edgeEls = DATA.edges.map((e) => {
  const a = pos[e.from], b = pos[e.to];
  if (!a || !b) return null;
  const rightward = b.cx >= a.cx;
  const sx = rightward ? a.x + a.w : a.x, ex = rightward ? b.x : b.x + b.w;
  const dx = Math.max(30, Math.abs(ex - sx) / 2);
  const c1 = sx + (rightward ? dx : -dx), c2 = ex + (rightward ? -dx : dx);
  const p = el('path', { class: 'edge ' + e.dir,
    d: `M${sx} ${a.cy} C${c1} ${a.cy} ${c2} ${b.cy} ${ex} ${b.cy}`,
    'marker-end': 'url(#ah)' });
  gEdges.appendChild(p);
  return p;
});

// nodes
const nodeEls = {};
for (const name in pos) {
  const p = pos[name];
  const g = el('g', { class: 'node', 'data-name': name });
  g.appendChild(el('rect', { x: p.x, y: p.y, width: p.w, height: p.h, rx: 7 }));
  const t = el('text', { x: p.cx, y: p.cy + 4, 'text-anchor': 'middle' });
  t.textContent = name;
  g.appendChild(t);
  g.addEventListener('mousemove', (ev) => showTip(ev, name));
  g.addEventListener('mouseleave', hideTip);
  g.addEventListener('click', () => pin(name));
  gNodes.appendChild(g);
  nodeEls[name] = g;
}

// tooltip + detail panel
const tip = document.getElementById('tip');
const filesOf = (name) => DATA.files[name] || [];
function layerOf(name) {
  for (const c of DATA.columns) if (c.modules.includes(name)) return c.layer;
  return '';
}
function showTip(ev, name) {
  const fs = filesOf(name);
  tip.innerHTML = `<b>${name}</b> · ${layerOf(name)}` +
    fs.map((f) => `<span class="f">${f}</span>`).join('');
  tip.style.display = 'block';
  tip.style.left = (ev.clientX + 12) + 'px';
  tip.style.top = (ev.clientY + 12) + 'px';
}
function hideTip() { tip.style.display = 'none'; }
const panel = document.getElementById('panel'), pbody = document.getElementById('pbody');
document.getElementById('pclose').onclick = () => { panel.style.display = 'none'; };
function pin(name) {
  const fs = filesOf(name);
  pbody.innerHTML = `<h2>${name}</h2><div class="k">${layerOf(name)}</div>` +
    (fs.length ? fs.map((f) => `<div class="path">${f}</div>`).join('')
               : '<div class="k">no files</div>');
  panel.style.display = 'block';
}

// click-to-light
const feat = document.getElementById('feat');
feat.appendChild(new Option('— none —', ''));
Object.keys(DATA.overlays).sort().forEach((sel) => feat.appendChild(new Option(sel, sel)));
function light(sel) {
  for (const n in nodeEls) nodeEls[n].classList.remove('lit');
  edgeEls.forEach((e) => e && e.classList.remove('lit'));
  const ov = DATA.overlays[sel];
  if (!ov) return;
  ov.nodes.forEach((n) => nodeEls[n] && nodeEls[n].classList.add('lit'));
  ov.edges.forEach((i) => edgeEls[i] && edgeEls[i].classList.add('lit'));
}
feat.addEventListener('change', () => light(feat.value));
if (DATA.initial) { feat.value = DATA.initial; light(DATA.initial); }

// pan / zoom via viewBox
let vb;
function fitView() {
  let maxX = 400, maxY = 300;
  for (const n in pos) {
    maxX = Math.max(maxX, pos[n].x + pos[n].w + PAD);
    maxY = Math.max(maxY, pos[n].y + pos[n].h + PAD);
  }
  vb = { x: 0, y: 0, w: maxX, h: maxY };
  applyVB();
}
function applyVB() { svg.setAttribute('viewBox', `${vb.x} ${vb.y} ${vb.w} ${vb.h}`); }
fitView();

svg.addEventListener('wheel', (ev) => {
  ev.preventDefault();
  const r = svg.getBoundingClientRect();
  const mx = vb.x + (ev.clientX - r.left) / r.width * vb.w;
  const my = vb.y + (ev.clientY - r.top) / r.height * vb.h;
  const k = ev.deltaY > 0 ? 1.1 : 0.9;
  vb.x = mx - (mx - vb.x) * k; vb.y = my - (my - vb.y) * k;
  vb.w *= k; vb.h *= k; applyVB();
}, { passive: false });

let drag = null;
svg.addEventListener('mousedown', (ev) => {
  drag = { x: ev.clientX, y: ev.clientY }; svg.classList.add('panning');
});
window.addEventListener('mouseup', () => { drag = null; svg.classList.remove('panning'); });
window.addEventListener('mousemove', (ev) => {
  if (!drag) return;
  const r = svg.getBoundingClientRect();
  vb.x -= (ev.clientX - drag.x) / r.width * vb.w;
  vb.y -= (ev.clientY - drag.y) / r.height * vb.h;
  drag.x = ev.clientX; drag.y = ev.clientY; applyVB();
});
</script>
</body>
</html>
```

- [ ] **Step 4: Write the failing renderer tests**

Create `src/render/html.rs` with ONLY the test module first (so it fails to compile against a missing `render`), then add the implementation in Step 6. Write this file:

```rust
use std::collections::BTreeMap;

use serde::Serialize;

use crate::comprehension::layered::{EdgeDir, FeatureOverlay, LayeredGraph};
use crate::graph::ArchGraph;

const TEMPLATE: &str = include_str!("html/template.html");

// (implementation added in Step 6)

#[cfg(test)]
mod tests {
    use super::*;
    use crate::comprehension::callgraph::CallGraph;
    use crate::comprehension::layered::{layered, overlay};
    use crate::lang::FnDecl;

    #[test]
    fn render_wraps_document_and_embeds_payload() {
        let mut g = ArchGraph::new();
        let a = g.ensure_module("adapters");
        let d = g.ensure_module("domain");
        g.add_edge(a, d);
        let lg = layered(&g);
        let files: BTreeMap<String, Vec<String>> =
            BTreeMap::from([("adapters".to_string(), vec!["adapters.rs".to_string()])]);

        let out = render(&g, &lg, &[], &files, None);

        assert!(out.starts_with("<!DOCTYPE html>"));
        assert!(out.contains("<script"));
        assert!(!out.contains("__CIRCUIT_DATA__"));
        assert!(out.contains("\"adapters\""));
        // adapters(Adapter, rank 3) -> domain(Domain, rank 1) is inward.
        assert!(out.contains("\"dir\":\"inward\""));
    }

    #[test]
    fn render_embeds_overlay_and_initial() {
        let mut g = ArchGraph::new();
        g.ensure_module("app");
        let lg = layered(&g);
        let decls = vec![(
            "app".to_string(),
            FnDecl { name: "run".into(), is_pub: true, is_test: false, is_main: false, calls: vec![] },
        )];
        let calls = CallGraph::build(&decls);
        let ov = overlay(&g, &calls, "app::run", &lg);
        let files = BTreeMap::new();

        let out = render(&g, &lg, &[("app::run".to_string(), ov)], &files, Some("app::run"));

        assert!(out.contains("\"overlays\":{\"app::run\""));
        assert!(out.contains("\"initial\":\"app::run\""));
    }
}
```

- [ ] **Step 5: Run tests to verify they fail**

Run: `cargo test --lib render::html`
Expected: FAIL — `cannot find function render in this scope`.

- [ ] **Step 6: Implement the view-model and `render`**

In `src/render/html.rs`, replace the `// (implementation added in Step 6)` line with:

```rust
#[derive(Serialize)]
struct ColView<'a> {
    layer: String,
    modules: Vec<&'a str>,
}

#[derive(Serialize)]
struct EdgeView<'a> {
    from: &'a str,
    to: &'a str,
    dir: &'static str,
}

#[derive(Serialize)]
struct OverlayView {
    nodes: Vec<String>,
    edges: Vec<usize>,
}

#[derive(Serialize)]
struct MapView<'a> {
    columns: Vec<ColView<'a>>,
    edges: Vec<EdgeView<'a>>,
    overlays: BTreeMap<String, OverlayView>,
    files: &'a BTreeMap<String, Vec<String>>,
    initial: Option<String>,
}

fn dir_str(d: EdgeDir) -> &'static str {
    match d {
        EdgeDir::Inward => "inward",
        EdgeDir::Outward => "outward",
        EdgeDir::Lateral => "lateral",
        EdgeDir::Unranked => "unranked",
    }
}

/// Emit a self-contained interactive HTML document that hydrates the layered
/// graph. Names are resolved (no `ModuleId` leaks); every map is a `BTreeMap`
/// and every list is pre-sorted, so the output is byte-stable. Presentation
/// only — the pure core carries no `serde`.
pub fn render(
    g: &ArchGraph,
    lg: &LayeredGraph,
    overlays: &[(String, FeatureOverlay)],
    files: &BTreeMap<String, Vec<String>>,
    initial: Option<&str>,
) -> String {
    let columns = lg
        .columns
        .iter()
        .map(|c| ColView {
            layer: format!("{:?}", c.layer),
            modules: c.modules.iter().map(|&id| g.name(id)).collect(),
        })
        .collect();

    let edges = lg
        .edges
        .iter()
        .map(|e| EdgeView {
            from: g.name(e.from),
            to: g.name(e.to),
            dir: dir_str(e.dir),
        })
        .collect();

    let overlays_map = overlays
        .iter()
        .map(|(sel, ov)| {
            (
                sel.clone(),
                OverlayView {
                    nodes: ov.modules.iter().map(|&id| g.name(id).to_string()).collect(),
                    edges: ov.edges.clone(),
                },
            )
        })
        .collect();

    let view = MapView {
        columns,
        edges,
        overlays: overlays_map,
        files,
        initial: initial.map(|s| s.to_string()),
    };

    let json = serde_json::to_string(&view).expect("MapView is always serializable");
    TEMPLATE.replace("__CIRCUIT_DATA__", &json)
}
```

- [ ] **Step 7: Run tests to verify they pass**

Run: `cargo test --lib render::html`
Expected: PASS (2 tests).

- [ ] **Step 8: Commit**

```bash
git add Cargo.toml Cargo.lock src/render/mod.rs src/render/html.rs src/render/html/template.html
git commit -m "feat(comprehension): interactive HTML renderer for the layered map"
```

---

### Task 3: `app::map_html` — assemble catalog + overlays and render

**Files:**
- Modify: `src/app.rs` (add `map_html` next to `map`, add one test)

**Interfaces:**
- Consumes: `builder::{build_graph, module_files}`, `comprehension::scan::scan_functions`, `comprehension::callgraph::CallGraph`, `comprehension::layered::{layered, overlay}`, `comprehension::{comprehend, EntryKind}`, `render::html::render`.
- Produces: `pub fn map_html(path: &std::path::Path, feature: Option<&str>) -> anyhow::Result<String>`. Consumed by Task 4.

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)] mod tests` block in `src/app.rs` (near `map_self_emits_layer_columns`):

```rust
    #[test]
    fn map_html_self_emits_document_with_overlays() {
        let out = map_html(std::path::Path::new("."), None).unwrap();
        assert!(out.starts_with("<!DOCTYPE html>"));
        assert!(out.contains("Adapter"));
        assert!(out.contains("\"overlays\""));
        assert!(!out.contains("__CIRCUIT_DATA__"));
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib map_html_self_emits`
Expected: FAIL — `cannot find function map_html in this scope`.

- [ ] **Step 3: Implement `map_html`**

Add this function in `src/app.rs` immediately after the existing `map` function:

```rust
/// Interactive HTML layered map (deterministic, no LLM): hydrates the same
/// `LayeredGraph` as `map`, plus a precomputed overlay per Main/Public entry
/// point (the click-to-light catalog) and a module→file map for drill-to-file.
/// `feature` pre-lights that selector on load when it matches a catalog entry.
pub fn map_html(path: &std::path::Path, feature: Option<&str>) -> anyhow::Result<String> {
    use crate::comprehension::EntryKind;

    let graph = crate::builder::build_graph(path)?;
    let lg = crate::comprehension::layered::layered(&graph);
    let decls = crate::comprehension::scan::scan_functions(path)?;
    let calls = crate::comprehension::callgraph::CallGraph::build(&decls);

    let comp = crate::comprehension::comprehend(&decls);
    let mut overlays: Vec<(String, crate::comprehension::layered::FeatureOverlay)> = Vec::new();
    for grp in &comp.groups {
        if matches!(grp.kind, EntryKind::Main | EntryKind::Public) {
            let ov = crate::comprehension::layered::overlay(&graph, &calls, &grp.entry, &lg);
            overlays.push((grp.entry.clone(), ov));
        }
    }

    let files = crate::builder::module_files(path)?;
    let initial = feature.filter(|f| overlays.iter().any(|(sel, _)| sel == f));

    Ok(crate::render::html::render(
        &graph, &lg, &overlays, &files, initial,
    ))
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib map_html_self_emits`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/app.rs
git commit -m "feat(comprehension): app::map_html assembles catalog overlays"
```

---

### Task 4: CLI `--html` flag + integration tests + final gate

**Files:**
- Modify: `src/main.rs` (add `html` flag to `Command::Map`, thread through match arm and `run_map`)
- Modify: `tests/cli.rs` (two integration tests)

**Interfaces:**
- Consumes: `circuit::app::map_html` (Task 3), `circuit::app::map` (existing).
- Produces: the `circuit map <path> --html [--feature <s>]` CLI surface.

- [ ] **Step 1: Write the failing integration tests**

Add to `tests/cli.rs` (after `map_mermaid_exports_flowchart`):

```rust
#[test]
fn map_html_emits_self_contained_document() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("src");
    std::fs::create_dir_all(src.join("app")).unwrap();
    std::fs::write(
        src.join("main.rs"),
        "use crate::app::run;\nfn main() { run(); }",
    )
    .unwrap();
    std::fs::write(src.join("app/mod.rs"), "pub fn run() {}").unwrap();

    Command::cargo_bin("circuit")
        .unwrap()
        .arg("map")
        .arg(dir.path())
        .arg("--html")
        .assert()
        .success()
        .stdout(predicate::str::contains("<!DOCTYPE html>"))
        .stdout(predicate::str::contains("__CIRCUIT_DATA__").not());
}

#[test]
fn map_html_conflicts_with_mermaid() {
    Command::cargo_bin("circuit")
        .unwrap()
        .arg("map")
        .arg(".")
        .arg("--html")
        .arg("--mermaid")
        .assert()
        .failure();
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --test cli map_html`
Expected: FAIL — `--html` is an unknown argument (the success test fails; the conflict test may pass trivially since an unknown arg also errors — that is fine, it will pass for the right reason after Step 3).

- [ ] **Step 3: Add the `html` flag**

In `src/main.rs`, inside the `Map { ... }` variant (around line 45), add the `html` field after `mermaid`:

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
        /// Emit a self-contained interactive HTML document (to stdout)
        #[arg(long, conflicts_with = "mermaid")]
        html: bool,
    },
```

- [ ] **Step 4: Thread it through the match arm**

In `src/main.rs`, update the `Command::Map` match arm (around line 236):

```rust
        Command::Map {
            path,
            feature,
            mermaid,
            html,
        } => run_map(&path, feature.as_deref(), mermaid, html),
```

- [ ] **Step 5: Update `run_map`**

In `src/main.rs`, replace the existing `run_map` function with:

```rust
fn run_map(path: &Path, feature: Option<&str>, mermaid: bool, html: bool) -> Result<()> {
    if html {
        println!("{}", circuit::app::map_html(path, feature)?);
    } else {
        println!("{}", circuit::app::map(path, feature, mermaid)?);
    }
    Ok(())
}
```

- [ ] **Step 6: Run the integration tests to verify they pass**

Run: `cargo test --test cli map_html`
Expected: PASS (2 tests).

- [ ] **Step 7: Final gate — full suite, clippy, fmt**

Run: `cargo test`
Expected: PASS (all tests, including the pre-existing 242+).

Run: `cargo clippy --all-targets -- -D warnings`
Expected: no warnings.

Run: `cargo fmt --check`
Expected: no diff. (If it reports formatting, run `cargo fmt` and re-run the suite.)

- [ ] **Step 8: Manual verification (flagged: not unit-tested)**

Run: `cargo run -- map . --html > /tmp/circuit-map.html` then open `/tmp/circuit-map.html` in a browser. Confirm by eye: layer columns render, wheel zooms, drag pans, hovering a node shows name·layer·file(s), clicking a node pins the detail panel, and selecting a feature from the dropdown lights its nodes + edges. This behavior is manually verified, not asserted by tests.

- [ ] **Step 9: Commit**

```bash
git add src/main.rs tests/cli.rs
git commit -m "feat(comprehension): circuit map --html CLI flag + integration tests"
```

---

## Self-Review

**Spec coverage:**
- §3 architecture (map_html, module_files, render, core-unchanged) → Tasks 1–4. ✓
- §4 catalog Main+Public precomputed overlays → Task 3 (filter + per-entry `overlay`). ✓
- §5 JSON payload shape (columns/edges/overlays/files/initial, names resolved) → Task 2 `MapView`. ✓
- §6 bespoke SVG file, `include_str!` template, `__CIRCUIT_DATA__` token, interactions → Task 2 template + Step 8 manual. ✓
- §7 CLI `--html` stdout, conflicts_with mermaid → Task 4. ✓
- §8 testing (renderer unit, IO unit, app, CLI, manual flagged) → Tasks 1–4. ✓
- §9 determinism (BTreeMap, sorted) → enforced in module_files (BTreeMap/BTreeSet) and MapView. ✓
- §11 done criteria (byte-stable existing output, single new dep, final gate) → Global Constraints + Task 4 Step 7. ✓

**Placeholder scan:** No TBD/TODO; every code step carries complete code. ✓

**Type consistency:** `module_files -> BTreeMap<String, Vec<String>>` produced in Task 1 and consumed by `render(files: &BTreeMap<String, Vec<String>>)` (Task 2) and `map_html` (Task 3). `render(g, lg, overlays: &[(String, FeatureOverlay)], files, initial: Option<&str>)` signature identical across Tasks 2/3. `map_html(path, feature) -> Result<String>` identical across Tasks 3/4. `EntryKind::{Main, Public}` matches the enum in `comprehension/mod.rs`. ✓
