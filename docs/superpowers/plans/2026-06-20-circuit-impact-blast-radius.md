# Impact / Blast-Radius View Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a `circuit impact <target>` verb that reports a function's blast radius — its dependents (callers-of-callers) and dependencies (callees), each ranked by call-hop distance.

**Architecture:** Mirror the existing `comprehend` slice: a pure core (`comprehension::impact`) over the existing `CallGraph`, wrapped by an `app::impact` use-case and a thin CLI verb. The one new graph primitive is depth-annotated *reverse* reachability; the forward cone reuses existing reachability with hop labels.

**Tech Stack:** Rust, tree-sitter (already wired), clap (CLI), assert_cmd/predicates (CLI tests), tempfile.

## Global Constraints

- `#![forbid(unsafe_code)]` — already at crate root; do not introduce `unsafe`.
- Deterministic output only: every list sorted; no timestamps, no map-iteration order in output.
- Zero-LLM, Rust-only; no new external dependencies.
- thiserror at boundaries / anyhow internally — the use-case returns `anyhow::Result<String>` (matches `app::comprehend`).
- Hop distance type is `u32`; function ids are `FnId` (= `usize`, defined in `callgraph.rs`).
- Cones are sorted by `(hop, qualified_name)`; targets are hop 0 and excluded from both cones.

Spec: `docs/superpowers/specs/2026-06-20-circuit-impact-blast-radius-design.md`

---

### Task 1: Depth-annotated forward + reverse reachability on `CallGraph`

**Files:**
- Modify: `src/comprehension/callgraph.rs`
- Test: `src/comprehension/callgraph.rs` (inline `#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: existing `CallGraph` (`nodes`, `node`, `edges`, `build`), `pub type FnId = usize`.
- Produces:
  - `pub fn reachable_with_depth(&self, starts: &[FnId]) -> Vec<(FnId, u32)>` — multi-source forward min-hop BFS; start set is hop 0; ascending by `FnId`.
  - `pub fn reverse_reachable_with_depth(&self, starts: &[FnId]) -> Vec<(FnId, u32)>` — same over reversed edges.
  - `pub fn reachable(&self, start: FnId) -> Vec<FnId>` — unchanged behaviour, now delegating to `reachable_with_depth`.

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module in `src/comprehension/callgraph.rs`:

```rust
    #[test]
    fn forward_and_reverse_depth() {
        let decls = vec![
            ("m".to_string(), decl("run", false, true, &["mid"])),
            ("m".to_string(), decl("mid", false, false, &["leaf"])),
            ("m".to_string(), decl("leaf", false, false, &[])),
        ];
        let g = CallGraph::build(&decls);
        // ids: run=0, mid=1, leaf=2
        assert_eq!(g.reachable_with_depth(&[0]), vec![(0, 0), (1, 1), (2, 2)]);
        assert_eq!(g.reverse_reachable_with_depth(&[2]), vec![(0, 2), (1, 1), (2, 0)]);
    }

    #[test]
    fn cycle_terminates_with_shortest_hops() {
        let decls = vec![
            ("m".to_string(), decl("a", false, false, &["b"])),
            ("m".to_string(), decl("b", false, false, &["a"])),
        ];
        let g = CallGraph::build(&decls);
        assert_eq!(g.reachable_with_depth(&[0]), vec![(0, 0), (1, 1)]);
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p circuit --lib comprehension::callgraph`
Expected: FAIL — `no method named reachable_with_depth` / `reverse_reachable_with_depth`.

- [ ] **Step 3: Implement the methods and refactor `reachable`**

In `src/comprehension/callgraph.rs`, replace the existing `reachable` method (lines ~72-92) with the three definitions below, and add the free `bfs_depths` helper after the `impl CallGraph` block:

```rust
    /// Forward min-hop BFS from any of `starts` (the start set is hop 0).
    /// Returns (FnId, hop) ascending by FnId.
    pub fn reachable_with_depth(&self, starts: &[FnId]) -> Vec<(FnId, u32)> {
        let mut adj: HashMap<FnId, Vec<FnId>> = HashMap::new();
        for &(f, t) in &self.edges {
            adj.entry(f).or_default().push(t);
        }
        bfs_depths(&adj, starts)
    }

    /// Reverse min-hop BFS (callers-of-callers) from any of `starts`.
    pub fn reverse_reachable_with_depth(&self, starts: &[FnId]) -> Vec<(FnId, u32)> {
        let mut adj: HashMap<FnId, Vec<FnId>> = HashMap::new();
        for &(f, t) in &self.edges {
            adj.entry(t).or_default().push(f);
        }
        bfs_depths(&adj, starts)
    }

    /// All functions reachable from `start` (inclusive), in ascending id order.
    pub fn reachable(&self, start: FnId) -> Vec<FnId> {
        self.reachable_with_depth(&[start])
            .into_iter()
            .map(|(id, _)| id)
            .collect()
    }
```

After the closing `}` of `impl CallGraph`, add:

```rust
/// Multi-source min-hop BFS over `adj`. Start set is hop 0; each node is
/// visited once (cycles terminate). Returns (id, hop) ascending by id.
fn bfs_depths(adj: &HashMap<FnId, Vec<FnId>>, starts: &[FnId]) -> Vec<(FnId, u32)> {
    let mut seen: BTreeSet<FnId> = BTreeSet::new();
    let mut depth: HashMap<FnId, u32> = HashMap::new();
    let mut q = VecDeque::new();
    for &s in starts {
        if seen.insert(s) {
            depth.insert(s, 0);
            q.push_back(s);
        }
    }
    while let Some(n) = q.pop_front() {
        let d = depth[&n];
        if let Some(next) = adj.get(&n) {
            for &t in next {
                if seen.insert(t) {
                    depth.insert(t, d + 1);
                    q.push_back(t);
                }
            }
        }
    }
    seen.into_iter().map(|id| (id, depth[&id])).collect()
}
```

(The `use std::collections::{BTreeSet, HashMap, VecDeque};` at the top already covers these.)

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p circuit --lib comprehension::callgraph`
Expected: PASS — including the pre-existing `builds_edges_and_reachability_by_name`.

- [ ] **Step 5: Commit**

```bash
git add src/comprehension/callgraph.rs
git commit -m "feat(comprehension): depth-annotated forward/reverse reachability on CallGraph"
```

---

### Task 2: `comprehension::impact` pure core

**Files:**
- Create: `src/comprehension/impact.rs`
- Modify: `src/comprehension/mod.rs:1-2` (add `pub mod impact;`)
- Test: `src/comprehension/impact.rs` (inline `#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: `CallGraph`, `reachable_with_depth`, `reverse_reachable_with_depth`, `FnId` (Task 1); `crate::lang::FnDecl`.
- Produces:
  - `pub struct ImpactReport { pub selector: String, pub targets: Vec<String>, pub dependents: Vec<(u32, String)>, pub dependencies: Vec<(u32, String)> }`
  - `pub fn impact(decls: &[(String, FnDecl)], target: &str, max_depth: Option<u32>) -> ImpactReport`
  - `pub fn render_text(r: &ImpactReport) -> String`

> Note: `selector` (the raw `target` string) is added to the struct vs. the spec sketch so `render_text` can print the header/notice without the caller passing the selector separately. Pure refinement; no behavioural change.

- [ ] **Step 1: Register the module**

In `src/comprehension/mod.rs`, change the top:

```rust
pub mod callgraph;
pub mod impact;
pub mod scan;
```

- [ ] **Step 2: Write the failing tests**

Create `src/comprehension/impact.rs` with ONLY the test module first (so it fails to compile against missing items):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::FnDecl;

    fn decl(name: &str, calls: &[&str]) -> FnDecl {
        FnDecl {
            name: name.into(),
            is_pub: false,
            is_test: false,
            is_main: name == "main",
            calls: calls.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn chain() -> Vec<(String, FnDecl)> {
        vec![
            ("a".to_string(), decl("run", &["mid"])),
            ("a".to_string(), decl("mid", &["leaf"])),
            ("a".to_string(), decl("leaf", &[])),
        ]
    }

    #[test]
    fn dependents_cone_with_hops() {
        let r = impact(&chain(), "leaf", None);
        assert_eq!(r.targets, vec!["a::leaf"]);
        assert_eq!(
            r.dependents,
            vec![(1, "a::mid".to_string()), (2, "a::run".to_string())]
        );
        assert!(r.dependencies.is_empty());
    }

    #[test]
    fn dependencies_cone_with_hops() {
        let r = impact(&chain(), "run", None);
        assert_eq!(
            r.dependencies,
            vec![(1, "a::mid".to_string()), (2, "a::leaf".to_string())]
        );
        assert!(r.dependents.is_empty());
    }

    #[test]
    fn max_depth_caps_both_cones() {
        let r = impact(&chain(), "run", Some(1));
        assert_eq!(r.dependencies, vec![(1, "a::mid".to_string())]);
    }

    #[test]
    fn bare_name_unions_all_matches() {
        let decls = vec![
            ("x".to_string(), decl("build", &[])),
            ("y".to_string(), decl("build", &[])),
            ("z".to_string(), decl("caller", &["build"])),
        ];
        let r = impact(&decls, "build", None);
        assert_eq!(r.targets, vec!["x::build", "y::build"]);
        assert_eq!(r.dependents, vec![(1, "z::caller".to_string())]);
    }

    #[test]
    fn no_match_renders_notice() {
        let r = impact(&chain(), "nope", None);
        assert!(r.targets.is_empty());
        assert!(render_text(&r).contains("no function matches 'nope'"));
    }

    #[test]
    fn render_shows_both_cones() {
        let out = render_text(&impact(&chain(), "mid", None));
        assert!(out.contains("impact: mid"));
        assert!(out.contains("dependents"));
        assert!(out.contains("·1  a::run"));
        assert!(out.contains("dependencies"));
        assert!(out.contains("·1  a::leaf"));
    }
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test -p circuit --lib comprehension::impact`
Expected: FAIL — `cannot find function impact` / `cannot find type ImpactReport`.

- [ ] **Step 4: Write the implementation**

Prepend to `src/comprehension/impact.rs` (above the test module):

```rust
use std::fmt::Write;

use crate::lang::FnDecl;

use super::callgraph::{CallGraph, FnId};

#[derive(Clone, Debug, Default)]
pub struct ImpactReport {
    /// The raw target selector the user passed.
    pub selector: String,
    /// Matched qualified names (sorted, deduped; empty when nothing matched).
    pub targets: Vec<String>,
    /// (hop, qualified) upstream cone — callers-of-callers.
    pub dependents: Vec<(u32, String)>,
    /// (hop, qualified) downstream cone — resolved internal callees.
    pub dependencies: Vec<(u32, String)>,
}

/// Pure core. Resolve `target` by bare name or `module::name`, union all
/// matches, then walk both directions with min-hop BFS. Deterministic:
/// cones sorted by (hop, qualified); targets (hop 0) excluded from cones.
pub fn impact(decls: &[(String, FnDecl)], target: &str, max_depth: Option<u32>) -> ImpactReport {
    let g = CallGraph::build(decls);

    let mut start: Vec<FnId> = Vec::new();
    for (id, node) in g.nodes().iter().enumerate() {
        if node.name == target || node.qualified() == target {
            start.push(id);
        }
    }

    let mut targets: Vec<String> = start.iter().map(|&id| g.node(id).qualified()).collect();
    targets.sort();
    targets.dedup();

    let cone = |raw: Vec<(FnId, u32)>| -> Vec<(u32, String)> {
        let mut out: Vec<(u32, String)> = raw
            .into_iter()
            .filter(|&(_, hop)| hop > 0 && max_depth.map_or(true, |m| hop <= m))
            .map(|(id, hop)| (hop, g.node(id).qualified()))
            .collect();
        out.sort();
        out.dedup();
        out
    };

    let (dependents, dependencies) = if start.is_empty() {
        (Vec::new(), Vec::new())
    } else {
        (
            cone(g.reverse_reachable_with_depth(&start)),
            cone(g.reachable_with_depth(&start)),
        )
    };

    ImpactReport {
        selector: target.to_string(),
        targets,
        dependents,
        dependencies,
    }
}

/// Deterministic plain-text render: header + two hop-grouped cones.
pub fn render_text(r: &ImpactReport) -> String {
    let mut out = String::new();
    if r.targets.is_empty() {
        let _ = writeln!(out, "no function matches '{}'", r.selector);
        return out;
    }
    if r.targets.len() > 1 {
        let _ = writeln!(
            out,
            "note: '{}' matches {} functions; reporting union blast radius:",
            r.selector,
            r.targets.len()
        );
        for t in &r.targets {
            let _ = writeln!(out, "        {t}");
        }
    }
    let _ = writeln!(out, "impact: {}  ({} target(s))", r.selector, r.targets.len());
    write_cone(&mut out, "▲ dependents (affected if changed)", &r.dependents);
    write_cone(&mut out, "▼ dependencies (what it relies on)", &r.dependencies);
    out
}

fn write_cone(out: &mut String, title: &str, cone: &[(u32, String)]) {
    let _ = writeln!(out, "\n{} — {} unit(s)", title, cone.len());
    for (hop, name) in cone {
        let _ = writeln!(out, "  ·{hop}  {name}");
    }
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p circuit --lib comprehension::impact`
Expected: PASS (all six tests).

- [ ] **Step 6: Commit**

```bash
git add src/comprehension/impact.rs src/comprehension/mod.rs
git commit -m "feat(comprehension): impact/blast-radius pure core (bidirectional cones + hops)"
```

---

### Task 3: `app::impact` use-case + `circuit impact` CLI + integration test

**Files:**
- Modify: `src/app.rs` (after `comprehend`, ~line 663)
- Modify: `src/main.rs` (Command enum ~line 29-33; dispatch ~line 186; helper ~line 201)
- Test: `tests/cli.rs` (append)

**Interfaces:**
- Consumes: `comprehension::scan::scan_functions`, `comprehension::impact::{impact, render_text}` (Task 2).
- Produces: `pub fn impact(path: &std::path::Path, target: &str, max_depth: Option<u32>) -> anyhow::Result<String>`.

- [ ] **Step 1: Write the failing integration test**

Append to `tests/cli.rs`:

```rust
#[test]
fn impact_reports_dependents() {
    let dir = tempdir().unwrap();
    let src = dir.path().join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("main.rs"), "fn main() { greet(); }\nfn greet() {}").unwrap();

    Command::cargo_bin("circuit")
        .unwrap()
        .arg("impact")
        .arg("greet")
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("dependents"))
        .stdout(predicate::str::contains("root::main"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p circuit --test cli impact_reports_dependents`
Expected: FAIL — clap errors on unknown subcommand `impact` (non-zero exit).

- [ ] **Step 3: Add the use-case in `src/app.rs`**

Immediately after the `comprehend` function (after line ~664), add:

```rust
/// Structural impact / blast radius (deterministic, no LLM): the dependents
/// and dependencies cones of a target function, ranked by hop distance.
pub fn impact(
    path: &std::path::Path,
    target: &str,
    max_depth: Option<u32>,
) -> anyhow::Result<String> {
    let decls = crate::comprehension::scan::scan_functions(path)?;
    let report = crate::comprehension::impact::impact(&decls, target, max_depth);
    Ok(crate::comprehension::impact::render_text(&report))
}
```

- [ ] **Step 4: Add the CLI verb in `src/main.rs`**

Add this variant to the `Command` enum, right after the `Comprehend { .. }` variant (after line ~33):

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

Add this arm to the `match cli.command` block, after the `Comprehend` arm (line ~186):

```rust
        Command::Impact { target, path, max_depth } => run_impact(&target, &path, max_depth),
```

Add this helper after `run_comprehend` (after line ~204):

```rust
fn run_impact(target: &str, path: &Path, max_depth: Option<u32>) -> Result<()> {
    println!("{}", circuit::app::impact(path, target, max_depth)?);
    Ok(())
}
```

- [ ] **Step 5: Run the integration test to verify it passes**

Run: `cargo test -p circuit --test cli impact_reports_dependents`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src/app.rs src/main.rs tests/cli.rs
git commit -m "feat(comprehension): wire circuit impact CLI verb + app use-case"
```

---

### Task 4: Bundled `lang/rust.rs` cleanups (`is_pub`, `is_test`)

**Files:**
- Modify: `src/lang/rust.rs` (`is_test_fn` ~line 66-84; `collect_fns` visibility loop ~line 94-100)
- Test: `src/lang/rust.rs` (inline `#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: nothing new.
- Produces: no signature changes — only corrects `FnDecl.is_pub` / `FnDecl.is_test` accuracy.

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module in `src/lang/rust.rs`:

```rust
    #[test]
    fn restricted_visibility_is_not_public() {
        let src = "pub(crate) fn a() {}\npub(super) fn b() {}\npub fn c() {}\nfn d() {}";
        let decls = fn_decls_in_source(src);
        let f = |n: &str| decls.iter().find(|d| d.name == n).unwrap();
        assert!(!f("a").is_pub);
        assert!(!f("b").is_pub);
        assert!(f("c").is_pub);
        assert!(!f("d").is_pub);
    }

    #[test]
    fn cfg_test_attr_is_not_a_test_fn() {
        let src = "#[cfg(test)]\nfn under_cfg() {}\n#[test]\nfn real_test() {}";
        let decls = fn_decls_in_source(src);
        let under = decls.iter().find(|d| d.name == "under_cfg").unwrap();
        let real = decls.iter().find(|d| d.name == "real_test").unwrap();
        assert!(!under.is_test);
        assert!(real.is_test);
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p circuit --lib lang::rust`
Expected: FAIL — `restricted_visibility_is_not_public` (a/b currently mis-marked `is_pub`) and `cfg_test_attr_is_not_a_test_fn` (`under_cfg` currently mis-marked `is_test`).

- [ ] **Step 3: Fix `is_pub` (exact `pub` only)**

In `collect_fns`, replace the visibility loop (lines ~94-100):

```rust
            let mut is_pub = false;
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == "visibility_modifier" {
                    is_pub = child
                        .utf8_text(src.as_bytes())
                        .map(|t| t.trim() == "pub")
                        .unwrap_or(false);
                }
            }
```

- [ ] **Step 4: Fix `is_test` (test-path attribute, not `cfg(test)`)**

Add this helper above `is_test_fn` (before line ~66):

```rust
/// True for test-marker attributes (`#[test]`, `#[tokio::test]`, …) but not
/// `#[cfg(test)]`: compares the attribute path's last segment to `test`,
/// ignoring any `(..)` argument list.
fn is_test_attr(text: &str) -> bool {
    let inner = text.trim().trim_start_matches("#[").trim_end_matches(']');
    let path = inner.split('(').next().unwrap_or(inner).trim();
    path.rsplit("::").next().map(|s| s == "test").unwrap_or(false)
}
```

In `is_test_fn`, replace the attribute check (the `if s.utf8_text(...).map(|t| t.contains("test"))...` block, lines ~71-76) with:

```rust
                if s.utf8_text(src.as_bytes())
                    .map(is_test_attr)
                    .unwrap_or(false)
                {
                    return true;
                }
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p circuit --lib lang::rust`
Expected: PASS — including the pre-existing `extracts_functions_with_flags_and_calls` (`#[test]` still marks a test, `pub fn greet` still public).

- [ ] **Step 6: Commit**

```bash
git add src/lang/rust.rs
git commit -m "fix(lang): is_pub excludes pub(crate)/pub(super); is_test ignores cfg(test)"
```

---

### Task 5: Full-suite green + clippy gate

**Files:** none (verification only).

- [ ] **Step 1: Run the whole test suite**

Run: `cargo test -p circuit`
Expected: PASS — all prior tests plus the new ones; 0 failed.

- [ ] **Step 2: Clippy with warnings denied**

Run: `cargo clippy --all-targets -- -D warnings`
Expected: no warnings.

- [ ] **Step 3: Manual smoke on Circuit's own repo**

Run: `cargo run -q -- impact build`
Expected: a `note:` listing the several `build` matches, then `impact: build (N target(s))`, then non-empty `dependents` / `dependencies` cones with `·N` hop labels. Deterministic across repeated runs.

- [ ] **Step 4: Commit (only if Step 1-3 surfaced a fix)**

```bash
git add -A
git commit -m "chore(comprehension): suite green + clippy clean for impact slice"
```

---

## Self-Review

**Spec coverage:**
- CLI `circuit impact <target> [path] [--max-depth N]` → Task 3 (Step 4).
- Bidirectional cones + hops (Q1-C) → Task 2 (`impact`), Task 1 (depth BFS).
- Union over all matches (Q2-A) → Task 2 (`bare_name_unions_all_matches`).
- Two-cone hop-grouped render + `--max-depth` (Q3) → Task 2 (`render_text`, `max_depth_caps_both_cones`).
- Reverse reachability + adjacency-rebuild cleanup → Task 1.
- `is_pub` / `is_test` cleanups (§4) → Task 4.
- No-match message, external-call limitation (inherent — downstream cone uses only resolved nodes via `reachable_with_depth`) → Task 2.
- Determinism → sorts in Task 1 (`bfs_depths` ascending) and Task 2 (`cone` sorts by `(hop, qualified)`).

**Placeholder scan:** none — every code/step is concrete.

**Type consistency:** `reachable_with_depth` / `reverse_reachable_with_depth` return `Vec<(FnId, u32)>` (Task 1) consumed verbatim by `cone` in Task 2; `ImpactReport` fields (`selector`, `targets`, `dependents`, `dependencies`) match across `impact`, `render_text`, and Task 3's use-case; `app::impact(path, target, max_depth)` signature matches `run_impact` call in Task 3.
</content>
