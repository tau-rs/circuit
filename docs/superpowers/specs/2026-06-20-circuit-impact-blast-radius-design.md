# Circuit Impact / Blast-Radius View — Design

**Date:** 2026-06-20
**Status:** Approved (brainstorm), pending implementation plan
**Pillar:** Comprehension (4th pillar)
**Predecessor:** [Comprehension pillar — structural slice](./2026-06-20-circuit-comprehension-pillar-design.md) (M1.5, PR #14, merged)

## Summary

A new comprehension sub-capability: point Circuit at a function and learn its
**blast radius** — what could break if you change it (dependents) and what it
relies on (dependencies), each ranked by how many call-hops away it is.

Deterministic, zero-LLM. Reuses the existing `CallGraph` substrate. The
genuinely new primitive is **reverse reachability** (callers-of-callers); the
forward cone is the same reachability `comprehend` already computes, now
annotated with hop distance.

## Motivation

`comprehend` answers "what are the entry points and what does each reach?" It is
top-down and forward-only. The complementary question a developer asks before
touching code is bottom-up: *"if I change this function, who is affected?"* That
is the reverse cone over the call graph, and it is not currently answerable.

## Scope

In scope:
- `circuit impact <target> [path] [--max-depth N]` CLI verb.
- Pure core `comprehension::impact` returning a structured report.
- `CallGraph` depth-annotated forward + reverse reachability.
- Bundled cleanups (see [Bundled cleanups](#bundled-cleanups)).

Out of scope (deferred to later slices):
- Layered-graph "B" renderer / visual surface.
- Clustering pre-pass, dynamic test-trace seeding.
- Any semantic / Tau (LLM) involvement.
- Cross-language; Rust only, as with the rest of the comprehension substrate.

## Decisions (locked in brainstorm)

| # | Decision |
|---|----------|
| Q1 | **Bidirectional + distance** (option C): report both the dependents cone (upstream) and the dependencies cone (downstream), each with hop-distance from the target. |
| Q2 | **Union over all matches** (option A): target is a bare `name` or `module::name`. Because the call graph resolves by name only, a selector may match several functions; we merge all matches into one target set and report the union. Honest about the graph's approximate-by-design nature; never silently picks one. |
| Q3 | **Two cones, grouped by hop distance ascending**, with a one-line summary header. Plus `--max-depth N` (default unlimited). |
| §4 | **Fold in** the `lang/rust.rs` `is_pub` / `is_test` cleanups (they improve the data `impact` reads). |

## CLI surface

```
circuit impact <target> [path] [--max-depth N]

  <target>        bare name (e.g. `build`) or qualified `module::name`
                  (e.g. `comprehension::callgraph::build`).
                  Union over all matching functions.
  [path]          repo root to scan. Default ".". Same semantics as `comprehend`.
  --max-depth N   cap hops in BOTH cones (e.g. `--max-depth 1` = direct only).
                  Default: unlimited.
```

### Sample output

```
impact: comprehension::callgraph::build  (1 target)

▲ dependents (affected if changed) — 4 unit(s)
  ·1  comprehension::comprehend
  ·1  comprehension::callgraph::reachable
  ·2  app::comprehend
  ·3  app::run

▼ dependencies (what it relies on) — 2 unit(s)
  ·1  comprehension::callgraph::FnNode::qualified
  ·2  lang::module_name_from_rel
```

`·N` is the minimum hop distance from the target. A bare name that matches
several functions prints a notice and reports the union:

```
$ circuit impact build
note: 'build' matches 3 functions; reporting union blast radius:
        comprehension::callgraph::build
        adapters::builder::build
        dag::build
impact: build  (3 targets)
...
```

No match:

```
$ circuit impact does_not_exist
no function matches 'does_not_exist'
```

## Architecture

Mirrors the existing `comprehend` slice exactly (pure core → app use-case → thin
CLI), so the pillar stays internally consistent.

```
main.rs  Command::Impact { target, path, max_depth }
   └─► app::impact(path, &target, max_depth) -> Result<String>
          ├─ comprehension::scan::scan_functions(path)  (reused, unchanged)
          ├─ comprehension::impact::impact(&decls, &target, max_depth) -> ImpactReport
          └─ comprehension::impact::render_text(&report) -> String
```

### `src/comprehension/impact.rs` (new)

```rust
pub struct ImpactReport {
    pub targets: Vec<String>,             // matched qualified names (≥0; union)
    pub dependents: Vec<(u32, String)>,   // (hop, qualified) — upstream cone
    pub dependencies: Vec<(u32, String)>, // (hop, qualified) — downstream cone
}

/// Pure core. Resolve target(s) by name or qualified name, walk both
/// directions with multi-source min-hop BFS, honor `max_depth`.
/// Deterministic: cones sorted by (hop, qualified name); targets excluded
/// from their own cones.
pub fn impact(decls: &[(String, FnDecl)], target: &str, max_depth: Option<u32>) -> ImpactReport;

/// Deterministic plain-text render (header + two hop-grouped cones).
pub fn render_text(r: &ImpactReport) -> String;
```

**Target resolution:** a node matches when `node.name == target` OR
`node.qualified() == target`. Collect all matching `FnId`s as the target set.
Empty set → empty cones; `render_text` prints `no function matches '<target>'`.

### `src/comprehension/callgraph.rs` (extend)

Two depth-annotated, multi-source traversals; both build the adjacency map
**once** internally:

```rust
/// Forward min-hop BFS from any of `starts`. Returns (FnId, hop), hop≥0
/// (the start set is hop 0). Used for the dependency cone + distances.
pub fn reachable_with_depth(&self, starts: &[FnId]) -> Vec<(FnId, u32)>;

/// Same over reversed edges — callers-of-callers. The dependents cone.
pub fn reverse_reachable_with_depth(&self, starts: &[FnId]) -> Vec<(FnId, u32)>;
```

`reachable()` is refactored to delegate to `reachable_with_depth(&[start])` and
drop the hop — folding in the "rebuilds adjacency per call" cleanup, since this
code now owns adjacency construction. Behaviour of `reachable()` is unchanged
(verified by its existing test).

**Distance & cycles:** BFS gives the *minimum* hop count. With a merged target
set, a function's hop is the shortest path from *any* target. Cycles terminate
normally (a node is enqueued once). Targets are hop 0 and are excluded from
both rendered cones.

**External / unresolved calls:** the downstream cone contains only resolved
*internal* functions. Calls to `std`, external crates, or anything not parsed
into a node are not edges, so they never appear. This is an inherent limitation
of the name-based graph and is stated, not hidden.

### `src/app.rs` (extend)

```rust
/// Structural impact / blast radius (deterministic, no LLM): the dependents
/// and dependencies cones of a target function, ranked by hop distance.
pub fn impact(path: &Path, target: &str, max_depth: Option<u32>) -> anyhow::Result<String> {
    let decls = comprehension::scan::scan_functions(path)?;
    let report = comprehension::impact::impact(&decls, target, max_depth);
    Ok(comprehension::impact::render_text(&report))
}
```

### `src/main.rs` (extend)

```rust
/// Impact / blast radius: dependents + dependencies of a function (no LLM)
Impact {
    /// Function name or `module::name` to analyze
    target: String,
    #[arg(default_value = ".")]
    path: PathBuf,
    /// Cap hops in both cones (default: unlimited)
    #[arg(long)]
    max_depth: Option<u32>,
},
```
```rust
Command::Impact { target, path, max_depth } => run_impact(&target, &path, max_depth),
// ...
fn run_impact(target: &str, path: &Path, max_depth: Option<u32>) -> Result<()> {
    println!("{}", circuit::app::impact(path, target, max_depth)?);
    Ok(())
}
```

## Bundled cleanups

From the predecessor handoff's "Minor cleanups" list, scoped to what this slice
touches:

1. **`CallGraph::reachable` rebuilds adjacency per call** — resolved by the
   `reachable_with_depth` refactor (in scope, Section 3).
2. **`is_pub` treats `pub(crate)` / `pub(super)` as public** (inflates entry
   points / target candidates) — fix in `lang/rust.rs`; add regression test.
3. **`is_test` uses substring match on attributes** (`#[cfg(test)]` directly on
   a fn could mis-mark) — fix in `lang/rust.rs`; add regression test.

Explicitly **not** bundled (not in this slice's path): the
`render_text_lists_entries_and_members` test that asserts only the entry line.

## Testing (TDD)

- `comprehension/impact.rs` unit tests: dependents cone, dependencies cone, hop
  distances, `max_depth` cap, multi-match union, no-match render. Reuse the
  `decl()` helper pattern from `mod.rs` / `callgraph.rs`.
- `comprehension/callgraph.rs` unit tests: forward depth BFS, reverse depth BFS,
  cycle handling (shortest path), unchanged `reachable()` behaviour.
- `lang/rust.rs` unit tests: `pub(crate)`/`pub(super)` not counted public;
  `#[cfg(test)]` on a fn not mis-marked as a test entry.
- `tests/cli.rs`: `circuit impact <fn>` on a temp repo asserts a known caller
  appears in the dependents cone (mirrors the existing `comprehend` CLI test).

## Determinism

Every output is sorted (cones by `(hop, qualified)`, target notice list sorted),
matching the rest of the comprehension pillar. No timestamps, no map iteration
order leaks into output.
</content>
</invoke>
