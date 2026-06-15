# M2 Slice C — DAG Board + Health Roll-up — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Render the spec-level DAG board as its own deterministic mermaid renderer (flow colorless, health as a separate glyph) and derive each node's health by running M1 indicators against its worktree, wired through a `circuit board <spec>` CLI.

**Architecture:** A pure presentation module `render::dag_board` over a view model decoupled from foundation types (`stage: Option<StageView>` carries git-unknown that `Stage` can't). A `cockpit::rollup` module computes `Health` from a worktree discovered via `GitPort` (Unknown when absent) and a traceability `m/n` count. The CLI injects a no-op `UnknownGit` adapter until the git-adapter slice merges, so the board is honest (`?` everywhere) without faking verdicts.

**Tech Stack:** Rust, `clap` CLI, `assert_cmd`/`predicates` + `tempfile` for tests, mermaid text output. `#![forbid(unsafe_code)]`, `thiserror` at boundaries, `anyhow` in the CLI.

**Design doc:** `docs/superpowers/specs/2026-06-14-circuit-m2-dag-board-design.md`

**Conventions consumed (already on `main`, do not modify):**
- `crate::cockpit::health::{Health, SessionHealth, rollup_children}` — `Health` is `Sound|Warn|Critical|Unknown` (Ord: Unknown highest). `SessionHealth{cycles, dep_violations}.rollup()` → Sound/Critical.
- `crate::flow::stage::{Stage, StageView, derive_stage}` — `Stage` has no Unknown; `StageView{stage, forge_certain}`. `derive_stage(&SessionRecord, &DeliveryFacts)` ignores its session arg in M2.
- `crate::flow::facts::{DeliveryFacts, BranchFacts, ReviewState}` — `DeliveryFacts{branch: BranchFacts, review: Option<ReviewState>}`.
- `crate::ports::{GitPort, Worktree}` — `GitPort` has assoc `Error`, `branch_facts(branch, base)`, `list_worktrees()->Vec<Worktree{path: PathBuf, branch: Option<String>}>`, `create_branch`, `add_worktree`.
- `crate::model::node::DagNode{id, spec, depends_on, branch}`, `crate::model::store::Workspace`, `crate::model::config::Config.base_branch`.
- `crate::session::{SessionRecord, SessionId}`.
- `crate::builder::build_graph`, `crate::indicators::cycles::find_cycles`, `crate::indicators::dependency_rule::violations`.

**Edge direction (locked):** the board renders **precedence** edges. For a node `n` with `n.depends_on = [d]`, emit `d --> n` (prerequisite points to dependent), matching the design doc §5.2 example (`auth_slice --> pay_slice` when pay depends on auth).

---

## File Structure

- Create `src/render/dag_board.rs` — `Board`, `BoardNode`, `render`, public presentation helpers `glyph`/`stage_cell`. Pure, no IO.
- Modify `src/render/mod.rs` — add `pub mod dag_board;`.
- Create `src/cockpit/rollup.rs` — `health_at_worktree`, `node_health`, `Traceability`, `traceability`. Generic over `GitPort`.
- Modify `src/cockpit/mod.rs` — add `pub mod rollup;`.
- Modify `src/main.rs` — add `Board` subcommand, `run_board`, and the `UnknownGit` no-op adapter (additive only).
- Create `tests/board.rs` — `assert_cmd` end-to-end test.

---

## Task 1: DAG board renderer — view model + node labels

**Files:**
- Create: `src/render/dag_board.rs`
- Modify: `src/render/mod.rs`
- Test: in `src/render/dag_board.rs` (`#[cfg(test)] mod tests`)

- [ ] **Step 1: Register the module**

In `src/render/mod.rs`, append:

```rust
pub mod dag_board;
```

- [ ] **Step 2: Write the failing tests for the view model + node labels**

Create `src/render/dag_board.rs` with:

```rust
//! The spec-level DAG board: a deterministic mermaid renderer whose nodes are
//! sessions/slices and whose edges are task dependencies. Flow never wears a
//! health color (design §8.2) — health appears only as a glyph in the label,
//! never in styling. A separate renderer from `render::mermaid` by design.

use crate::cockpit::health::Health;
use crate::flow::stage::{Stage, StageView};

/// One board node: a DAG node decorated with its derived stage and health.
/// `stage` is `Option` so a git-undeterminable stage (`None`) is distinct from
/// `Draft` — honesty the foundation `Stage` (no Unknown variant) cannot express.
pub struct BoardNode {
    pub id: String,
    pub depends_on: Vec<String>,
    pub stage: Option<StageView>,
    pub health: Health,
}

/// A whole board for one spec.
pub struct Board {
    pub nodes: Vec<BoardNode>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: &str, deps: &[&str], stage: Option<StageView>, health: Health) -> BoardNode {
        BoardNode {
            id: id.to_string(),
            depends_on: deps.iter().map(|s| s.to_string()).collect(),
            stage,
            health,
        }
    }

    fn certain(s: Stage) -> Option<StageView> {
        Some(StageView { stage: s, forge_certain: true })
    }

    #[test]
    fn renders_header_and_sorted_node_labels() {
        let board = Board {
            nodes: vec![
                node("pay-slice", &[], certain(Stage::Review), Health::Sound),
                node("auth-slice", &[], certain(Stage::Implement), Health::Critical),
            ],
        };
        let out = render(&board);
        assert!(out.starts_with("graph TD\n"));
        // sorted by id: auth before pay
        let auth = out.find("auth-slice").unwrap();
        let pay = out.find("pay-slice").unwrap();
        assert!(auth < pay);
        assert!(out.contains(r#"auth_slice["auth-slice · Implement · ◍"]"#));
        assert!(out.contains(r#"pay_slice["pay-slice · Review · ●"]"#));
    }

    #[test]
    fn glyph_covers_every_health() {
        assert_eq!(glyph(Health::Sound), '●');
        assert_eq!(glyph(Health::Warn), '◐');
        assert_eq!(glyph(Health::Critical), '◍');
        assert_eq!(glyph(Health::Unknown), '?');
    }

    #[test]
    fn stage_cell_renders_unknown_uncertain_and_certain() {
        assert_eq!(stage_cell(&None), "?");
        assert_eq!(
            stage_cell(&Some(StageView { stage: Stage::Implement, forge_certain: true })),
            "Implement"
        );
        // forge-gated refinement unconfirmed -> trailing '?'
        assert_eq!(
            stage_cell(&Some(StageView { stage: Stage::Review, forge_certain: false })),
            "Review?"
        );
    }

    #[test]
    fn unknown_stage_and_health_render_as_question_marks() {
        let board = Board { nodes: vec![node("x", &[], None, Health::Unknown)] };
        let out = render(&board);
        assert!(out.contains(r#"x["x · ? · ?"]"#));
    }
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test --lib render::dag_board 2>&1 | tail -20`
Expected: FAIL — `cannot find function `render`/`glyph`/`stage_cell``.

- [ ] **Step 4: Implement the view model rendering (no edges/styling yet)**

In `src/render/dag_board.rs`, above the `#[cfg(test)]` block, add:

```rust
/// Sanitize an id into a mermaid-safe node id (alphanumerics kept, rest `_`).
/// Reimplemented locally — the board is a separate renderer from `render::mermaid`.
fn node_id(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect()
}

/// Health glyph (design §9). Total over `Health`; `Warn` is reserved (no producer
/// yet) so `◐` never actually appears, but the mapping stays exhaustive.
pub fn glyph(h: Health) -> char {
    match h {
        Health::Sound => '●',
        Health::Warn => '◐',
        Health::Critical => '◍',
        Health::Unknown => '?',
    }
}

fn stage_name(s: Stage) -> &'static str {
    match s {
        Stage::Draft => "Draft",
        Stage::Project => "Project",
        Stage::Implement => "Implement",
        Stage::Review => "Review",
        Stage::Merge => "Merge",
        Stage::Done => "Done",
    }
}

/// The stage cell for a label. `None` -> `?` (git-undeterminable); a stage whose
/// forge-gated refinement is unconfirmed gets a trailing `?`.
pub fn stage_cell(stage: &Option<StageView>) -> String {
    match stage {
        None => "?".to_string(),
        Some(v) if v.forge_certain => stage_name(v.stage).to_string(),
        Some(v) => format!("{}?", stage_name(v.stage)),
    }
}

/// Render the board as a deterministic mermaid `graph TD`.
pub fn render(board: &Board) -> String {
    let mut nodes: Vec<&BoardNode> = board.nodes.iter().collect();
    nodes.sort_by(|a, b| a.id.cmp(&b.id));

    let mut out = String::from("graph TD\n");
    for n in &nodes {
        out.push_str(&format!(
            "  {}[\"{} · {} · {}\"]\n",
            node_id(&n.id),
            n.id,
            stage_cell(&n.stage),
            glyph(n.health),
        ));
    }
    out
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test --lib render::dag_board 2>&1 | tail -20`
Expected: PASS (4 tests).

- [ ] **Step 6: Commit**

```bash
git add src/render/mod.rs src/render/dag_board.rs
git commit -m "feat(render): DAG board view model + node labels"
```

---

## Task 2: DAG board renderer — edges + colorless styling + invariant test

**Files:**
- Modify: `src/render/dag_board.rs`
- Test: in `src/render/dag_board.rs` (`#[cfg(test)] mod tests`)

- [ ] **Step 1: Write the failing tests for edges, styling, and the colorless invariant**

In `src/render/dag_board.rs`, inside `mod tests`, add these tests:

```rust
    #[test]
    fn renders_precedence_edges_sorted() {
        // pay depends on auth -> precedence edge auth --> pay
        let board = Board {
            nodes: vec![
                node("auth", &[], certain(Stage::Done), Health::Sound),
                node("pay", &["auth"], certain(Stage::Implement), Health::Sound),
            ],
        };
        let out = render(&board);
        assert!(out.contains("auth --> pay"));
        assert!(!out.contains("pay --> auth"));
    }

    #[test]
    fn applies_one_colorless_class_to_all_nodes() {
        let board = Board {
            nodes: vec![
                node("a", &[], certain(Stage::Implement), Health::Critical),
                node("b", &["a"], certain(Stage::Review), Health::Sound),
            ],
        };
        let out = render(&board);
        let defs: Vec<&str> = out.lines().filter(|l| l.trim_start().starts_with("classDef")).collect();
        assert_eq!(defs.len(), 1);
        assert!(defs[0].contains("classDef flow"));
        assert!(out.contains("class a,b flow;"));
    }

    /// The headline invariant: styling is a function of structure only, never of
    /// health. Flipping every node's health leaves the classDef/class/style lines
    /// byte-identical (design §8.2 / §5.3).
    #[test]
    fn styling_never_varies_with_health() {
        let mk = |h: Health| Board {
            nodes: vec![
                node("a", &[], certain(Stage::Implement), h),
                node("b", &["a"], None, h),
            ],
        };
        let style_lines = |s: &str| {
            s.lines()
                .filter(|l| {
                    let t = l.trim_start();
                    t.starts_with("classDef") || t.starts_with("class ") || t.starts_with("style")
                })
                .map(|l| l.to_string())
                .collect::<Vec<_>>()
        };
        let sound = render(&mk(Health::Sound));
        let critical = render(&mk(Health::Critical));
        let unknown = render(&mk(Health::Unknown));
        assert_eq!(style_lines(&sound), style_lines(&critical));
        assert_eq!(style_lines(&sound), style_lines(&unknown));
        // Sanity: health DID change the output — just not the styling (it's in the label).
        assert_ne!(sound, critical);
    }

    #[test]
    fn empty_board_is_just_the_header() {
        let board = Board { nodes: vec![] };
        assert_eq!(render(&board), "graph TD\n");
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib render::dag_board 2>&1 | tail -20`
Expected: FAIL — edges and `classDef`/`class` lines are not yet emitted.

- [ ] **Step 3: Implement edges and colorless styling**

In `src/render/dag_board.rs`, replace the body of `render` with:

```rust
pub fn render(board: &Board) -> String {
    let mut nodes: Vec<&BoardNode> = board.nodes.iter().collect();
    nodes.sort_by(|a, b| a.id.cmp(&b.id));

    let mut out = String::from("graph TD\n");
    for n in &nodes {
        out.push_str(&format!(
            "  {}[\"{} · {} · {}\"]\n",
            node_id(&n.id),
            n.id,
            stage_cell(&n.stage),
            glyph(n.health),
        ));
    }

    // Precedence edges: for `node.depends_on = [dep]`, emit `dep --> node`.
    let mut edges: Vec<(String, String)> = Vec::new();
    for n in &nodes {
        for dep in &n.depends_on {
            edges.push((dep.clone(), n.id.clone()));
        }
    }
    edges.sort();
    for (from, to) in &edges {
        out.push_str(&format!("  {} --> {}\n", node_id(from), node_id(to)));
    }

    // Colorless styling: a single fixed class applied to every node regardless of
    // health. Health lives only in the label glyph, never here.
    if !nodes.is_empty() {
        out.push_str("classDef flow fill:#fff,stroke:#999;\n");
        let ids: Vec<String> = nodes.iter().map(|n| node_id(&n.id)).collect();
        out.push_str(&format!("class {} flow;\n", ids.join(",")));
    }

    out
}
```

- [ ] **Step 4: Run the full renderer test module to verify it passes**

Run: `cargo test --lib render::dag_board 2>&1 | tail -20`
Expected: PASS (8 tests).

- [ ] **Step 5: Commit**

```bash
git add src/render/dag_board.rs
git commit -m "feat(render): DAG board edges + colorless styling with invariant test"
```

---

## Task 3: Cockpit — health from a worktree path

**Files:**
- Create: `src/cockpit/rollup.rs`
- Modify: `src/cockpit/mod.rs`
- Test: in `src/cockpit/rollup.rs` (`#[cfg(test)] mod tests`)

- [ ] **Step 1: Register the module**

In `src/cockpit/mod.rs`, append:

```rust
pub mod rollup;
```

- [ ] **Step 2: Write the failing tests for `health_at_worktree`**

Create `src/cockpit/rollup.rs` with:

```rust
//! Health roll-up from a worktree (design §9). The pure roll-up math lives in
//! `cockpit::health`; this module discovers a node's worktree via `GitPort` and
//! runs M1's indicators against it. No worktree => `Unknown` (no bare-git
//! reconstruction). Also computes traceability `m/n` (design §8.3).

use std::path::Path;

use crate::builder::build_graph;
use crate::cockpit::health::{Health, SessionHealth};
use crate::indicators::cycles::find_cycles;
use crate::indicators::dependency_rule::violations;
use crate::model::node::DagNode;
use crate::ports::GitPort;

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, rel: &str, body: &str) {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    }

    #[test]
    fn clean_layered_worktree_is_sound() {
        let dir = tempfile::tempdir().unwrap();
        // adapters -> domain : outer depends on inner, no cycle, no violation.
        write(dir.path(), "src/domain.rs", "pub struct Order;");
        write(dir.path(), "src/adapters.rs", "use crate::domain::Order;");
        assert_eq!(health_at_worktree(dir.path()), Health::Sound);
    }

    #[test]
    fn worktree_with_a_cycle_is_critical() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "src/a.rs", "use crate::b::X;");
        write(dir.path(), "src/b.rs", "use crate::a::Y;");
        assert_eq!(health_at_worktree(dir.path()), Health::Critical);
    }

    #[test]
    fn worktree_with_a_dependency_violation_is_critical() {
        let dir = tempfile::tempdir().unwrap();
        // domain -> adapters : inner depends on outer = violation.
        write(dir.path(), "src/domain.rs", "use crate::adapters::Db;");
        write(dir.path(), "src/adapters.rs", "pub struct Db;");
        assert_eq!(health_at_worktree(dir.path()), Health::Critical);
    }

    #[test]
    fn nonexistent_path_is_unknown() {
        assert_eq!(
            health_at_worktree(Path::new("/no/such/worktree/xyz")),
            Health::Unknown
        );
    }
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test --lib cockpit::rollup 2>&1 | tail -20`
Expected: FAIL — `cannot find function `health_at_worktree``.

- [ ] **Step 4: Implement `health_at_worktree`**

In `src/cockpit/rollup.rs`, above the `#[cfg(test)]` block, add:

```rust
/// Run M1's indicators against a worktree and roll the counts up. A build error
/// (e.g. the path vanished) yields `Unknown` — we measured nothing, claim nothing.
pub fn health_at_worktree(path: &Path) -> Health {
    match build_graph(path) {
        Ok(graph) => SessionHealth {
            cycles: find_cycles(&graph).len(),
            dep_violations: violations(&graph).len(),
        }
        .rollup(),
        Err(_) => Health::Unknown,
    }
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test --lib cockpit::rollup 2>&1 | tail -20`
Expected: PASS (4 tests).

- [ ] **Step 6: Commit**

```bash
git add src/cockpit/mod.rs src/cockpit/rollup.rs
git commit -m "feat(cockpit): derive health from a worktree via M1 indicators"
```

---

## Task 4: Cockpit — `node_health` (worktree discovery via GitPort)

**Files:**
- Modify: `src/cockpit/rollup.rs`
- Test: in `src/cockpit/rollup.rs` (`#[cfg(test)] mod tests`)

- [ ] **Step 1: Write the failing tests for `node_health` with a fake `GitPort`**

In `src/cockpit/rollup.rs`, inside `mod tests`, add the fake port and tests:

```rust
    use crate::flow::facts::BranchFacts;
    use crate::ports::{GitPort, Worktree};
    use std::collections::HashMap;
    use std::path::{Path as StdPath, PathBuf};

    #[derive(Debug)]
    struct FakeError;
    impl std::fmt::Display for FakeError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "fake error")
        }
    }
    impl std::error::Error for FakeError {}

    /// Configurable fake: a list of worktrees, a branch->merged map for facts,
    /// and a switch to force `branch_facts` to error.
    struct FakeGit {
        worktrees: Vec<Worktree>,
        merged: HashMap<String, bool>,
        facts_err: bool,
    }
    impl FakeGit {
        fn new() -> Self {
            Self { worktrees: vec![], merged: HashMap::new(), facts_err: false }
        }
    }
    impl GitPort for FakeGit {
        type Error = FakeError;
        fn branch_facts(&self, branch: &str, _base: &str) -> Result<BranchFacts, Self::Error> {
            if self.facts_err {
                return Err(FakeError);
            }
            Ok(BranchFacts {
                exists: true,
                merged_into_base: *self.merged.get(branch).unwrap_or(&false),
                ..Default::default()
            })
        }
        fn create_branch(&self, _branch: &str, _base: &str) -> Result<(), Self::Error> {
            Err(FakeError)
        }
        fn add_worktree(&self, _branch: &str, _path: &StdPath) -> Result<(), Self::Error> {
            Err(FakeError)
        }
        fn list_worktrees(&self) -> Result<Vec<Worktree>, Self::Error> {
            Ok(self.worktrees.clone())
        }
    }

    #[test]
    fn node_health_measures_the_matching_worktree() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "src/a.rs", "use crate::b::X;");
        write(dir.path(), "src/b.rs", "use crate::a::Y;");
        let mut git = FakeGit::new();
        git.worktrees = vec![Worktree {
            path: dir.path().to_path_buf(),
            branch: Some("impl/cycle".to_string()),
        }];
        assert_eq!(node_health(&git, "impl/cycle"), Health::Critical);
    }

    #[test]
    fn node_health_is_unknown_when_no_worktree_matches() {
        let mut git = FakeGit::new();
        git.worktrees = vec![Worktree {
            path: PathBuf::from("/tmp/other"),
            branch: Some("impl/other".to_string()),
        }];
        assert_eq!(node_health(&git, "impl/missing"), Health::Unknown);
    }

    #[test]
    fn node_health_is_unknown_when_list_worktrees_errors() {
        // A port that errors on list_worktrees.
        struct ErrGit;
        impl GitPort for ErrGit {
            type Error = FakeError;
            fn branch_facts(&self, _b: &str, _base: &str) -> Result<BranchFacts, Self::Error> {
                Err(FakeError)
            }
            fn create_branch(&self, _b: &str, _base: &str) -> Result<(), Self::Error> {
                Err(FakeError)
            }
            fn add_worktree(&self, _b: &str, _p: &StdPath) -> Result<(), Self::Error> {
                Err(FakeError)
            }
            fn list_worktrees(&self) -> Result<Vec<Worktree>, Self::Error> {
                Err(FakeError)
            }
        }
        assert_eq!(node_health(&ErrGit, "impl/x"), Health::Unknown);
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib cockpit::rollup 2>&1 | tail -20`
Expected: FAIL — `cannot find function `node_health``.

- [ ] **Step 3: Implement `node_health`**

In `src/cockpit/rollup.rs`, after `health_at_worktree`, add:

```rust
/// Discover the worktree for `branch` via the port and measure it. No matching
/// worktree, or a `list_worktrees` error, yields `Unknown` (design §9).
pub fn node_health<G: GitPort>(git: &G, branch: &str) -> Health {
    let worktrees = match git.list_worktrees() {
        Ok(w) => w,
        Err(_) => return Health::Unknown,
    };
    match worktrees.into_iter().find(|w| w.branch.as_deref() == Some(branch)) {
        Some(w) => health_at_worktree(&w.path),
        None => Health::Unknown,
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --lib cockpit::rollup 2>&1 | tail -20`
Expected: PASS (7 tests).

- [ ] **Step 5: Commit**

```bash
git add src/cockpit/rollup.rs
git commit -m "feat(cockpit): node_health discovers a worktree via GitPort"
```

---

## Task 5: Cockpit — traceability `m/n`

**Files:**
- Modify: `src/cockpit/rollup.rs`
- Test: in `src/cockpit/rollup.rs` (`#[cfg(test)] mod tests`)

- [ ] **Step 1: Write the failing tests for `traceability`**

In `src/cockpit/rollup.rs`, inside `mod tests`, add:

```rust
    fn dag(id: &str, branch: &str) -> DagNode {
        DagNode::new(id, "checkout", id, branch)
    }

    #[test]
    fn traceability_counts_merged_nodes() {
        let mut git = FakeGit::new();
        git.merged.insert("impl/a".to_string(), true);
        git.merged.insert("impl/b".to_string(), false);
        let nodes = vec![dag("a", "impl/a"), dag("b", "impl/b")];
        let t = traceability(&git, &nodes, "main");
        assert_eq!(t, Traceability { merged: Some(1), total: 2 });
    }

    #[test]
    fn traceability_merged_is_none_when_facts_undeterminable() {
        let mut git = FakeGit::new();
        git.facts_err = true;
        let nodes = vec![dag("a", "impl/a")];
        let t = traceability(&git, &nodes, "main");
        assert_eq!(t, Traceability { merged: None, total: 1 });
    }

    #[test]
    fn traceability_of_empty_dag_is_zero_over_zero() {
        let git = FakeGit::new();
        let t = traceability(&git, &[], "main");
        assert_eq!(t, Traceability { merged: Some(0), total: 0 });
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib cockpit::rollup 2>&1 | tail -20`
Expected: FAIL — `cannot find type `Traceability` / function `traceability``.

- [ ] **Step 3: Implement `Traceability` + `traceability`**

In `src/cockpit/rollup.rs`, after `node_health`, add:

```rust
/// Traceability `m / n` (design §8.3): how many DAG nodes are merged into base.
/// `merged` is `None` when git cannot answer — we never report a partial count.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Traceability {
    pub merged: Option<usize>,
    pub total: usize,
}

/// Count DAG nodes whose branch is merged into `base`. Any `branch_facts` error
/// collapses the whole count to `None` (undeterminable), keeping it honest.
pub fn traceability<G: GitPort>(git: &G, nodes: &[DagNode], base: &str) -> Traceability {
    let total = nodes.len();
    let mut merged = 0usize;
    for n in nodes {
        match git.branch_facts(&n.branch, base) {
            Ok(facts) => {
                if facts.merged_into_base {
                    merged += 1;
                }
            }
            Err(_) => return Traceability { merged: None, total },
        }
    }
    Traceability { merged: Some(merged), total }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --lib cockpit::rollup 2>&1 | tail -20`
Expected: PASS (10 tests).

- [ ] **Step 5: Commit**

```bash
git add src/cockpit/rollup.rs
git commit -m "feat(cockpit): traceability m/n merged-into-base count"
```

---

## Task 6: CLI — `circuit board <spec>` + no-op git adapter + e2e

**Files:**
- Modify: `src/main.rs`
- Create: `tests/board.rs`

- [ ] **Step 1: Write the failing end-to-end test**

Create `tests/board.rs` with:

```rust
use assert_cmd::Command;
use predicates::prelude::*;

/// Drive the exit-criteria walk for slice C: init, spec, two DAG nodes (one
/// depending on the other), then `circuit board`. With the no-op git adapter the
/// board is honest — `?` stages, `?` health, `?/n` tasks — and flow stays colorless.
#[test]
fn board_renders_colorless_mermaid_and_honest_unknowns() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path();

    let run = |args: &[&str]| {
        Command::cargo_bin("circuit").unwrap().args(args).current_dir(path).assert().success();
    };

    run(&["init"]);
    run(&["spec", "new", "checkout", "--title", "Checkout", "--intent", "Pay."]);
    run(&["dag", "add-node", "auth", "--spec", "checkout", "--title", "Auth", "--branch", "impl/auth"]);
    run(&[
        "dag", "add-node", "pay", "--spec", "checkout", "--title", "Pay",
        "--branch", "impl/pay", "--depends-on", "auth",
    ]);

    Command::cargo_bin("circuit")
        .unwrap()
        .args(["board", "checkout"])
        .current_dir(path)
        .assert()
        .success()
        .stdout(predicate::str::contains("graph TD"))
        // precedence edge: prerequisite -> dependent
        .stdout(predicate::str::contains("auth --> pay"))
        // colorless styling present
        .stdout(predicate::str::contains("classDef flow"))
        // no-op adapter => honest unknowns in the labels
        .stdout(predicate::str::contains("auth · ? · ?"))
        // traceability undeterminable without git
        .stdout(predicate::str::contains("Tasks: ?/2"));
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --test board 2>&1 | tail -20`
Expected: FAIL — `error: unrecognized subcommand 'board'`.

- [ ] **Step 3: Add the `Board` subcommand variant**

In `src/main.rs`, add a variant to `enum Command` (after the `Dag { .. }` arm):

```rust
    /// Spec-level DAG board (mermaid) with stage + health
    Board {
        /// Spec id whose DAG to render
        spec: String,
        #[arg(long, default_value = ".")]
        path: PathBuf,
    },
```

- [ ] **Step 4: Route the subcommand in `main`**

In `src/main.rs`, in the `match cli.command` block, add the arm:

```rust
        Command::Board { spec, path } => run_board(&spec, &path),
```

- [ ] **Step 5: Add imports, the no-op adapter, and `run_board`**

In `src/main.rs`, add to the `use circuit::...` imports at the top:

```rust
use circuit::flow::facts::{BranchFacts, DeliveryFacts};
use circuit::flow::stage::derive_stage;
use circuit::ports::{GitPort, Worktree};
use circuit::render::dag_board::{self, Board, BoardNode};
use circuit::session::{SessionId, SessionRecord};
```

At the bottom of `src/main.rs`, add the no-op adapter and the command handler:

```rust
/// No-op `GitPort` for the period before the git-adapter slice merges: it answers
/// honestly that it knows nothing. `branch_facts` errors (=> `?` stage, `?/n`
/// tasks); `list_worktrees` is empty (=> `Unknown` health). PR NOTE: when the git
/// adapter lands, swap this for the real adapter at the one `run_board` wiring
/// point — `cockpit`/`render` are already generic over `GitPort`.
struct UnknownGit;

#[derive(Debug)]
struct GitUnavailable;
impl std::fmt::Display for GitUnavailable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "git adapter not yet available")
    }
}
impl std::error::Error for GitUnavailable {}

impl GitPort for UnknownGit {
    type Error = GitUnavailable;
    fn branch_facts(&self, _branch: &str, _base: &str) -> Result<BranchFacts, Self::Error> {
        Err(GitUnavailable)
    }
    fn create_branch(&self, _branch: &str, _base: &str) -> Result<(), Self::Error> {
        Err(GitUnavailable)
    }
    fn add_worktree(&self, _branch: &str, _path: &Path) -> Result<(), Self::Error> {
        Err(GitUnavailable)
    }
    fn list_worktrees(&self) -> Result<Vec<Worktree>, Self::Error> {
        Ok(vec![])
    }
}

fn run_board(spec: &str, path: &Path) -> Result<()> {
    let ws = Workspace::new(path);
    require_initialized(&ws)?;
    let base = ws.load_config().context("reading config.toml")?.base_branch;
    let nodes: Vec<DagNode> = ws
        .list_dag_nodes()
        .context("reading dag nodes")?
        .into_iter()
        .filter(|n| n.spec == spec)
        .collect();
    let sessions = ws.list_sessions().context("reading sessions")?;

    let git = UnknownGit;

    let mut board_nodes = Vec::new();
    for n in &nodes {
        let stage = match git.branch_facts(&n.branch, &base) {
            Ok(branch) => {
                let session = sessions
                    .iter()
                    .find(|s| s.dag_node.as_deref() == Some(n.id.as_str()))
                    .cloned()
                    // derive_stage ignores the session in M2, so a synthesized
                    // record (with a throwaway id, never rendered) is sound here.
                    .unwrap_or_else(|| {
                        SessionRecord::impl_(SessionId::generate(), &n.spec, &n.id, &n.branch)
                    });
                let facts = DeliveryFacts { branch, review: None };
                Some(derive_stage(&session, &facts))
            }
            Err(_) => None,
        };
        let health = circuit::cockpit::rollup::node_health(&git, &n.branch);
        board_nodes.push(BoardNode {
            id: n.id.clone(),
            depends_on: n.depends_on.clone(),
            stage,
            health,
        });
    }

    let board = Board { nodes: board_nodes };
    print!("{}", dag_board::render(&board));

    // Per-node readout, sorted by id (matches the board's node order).
    println!("\n--- nodes ---");
    let mut sorted: Vec<&BoardNode> = board.nodes.iter().collect();
    sorted.sort_by(|a, b| a.id.cmp(&b.id));
    let healths: Vec<_> = sorted.iter().map(|n| n.health).collect();
    for n in &sorted {
        println!(
            "  {}  {}  {}",
            n.id,
            dag_board::stage_cell(&n.stage),
            dag_board::glyph(n.health)
        );
    }

    let spec_health = circuit::cockpit::health::rollup_children(&healths);
    let trace = circuit::cockpit::rollup::traceability(&git, &nodes, &base);
    let m = trace.merged.map(|m| m.to_string()).unwrap_or_else(|| "?".to_string());
    println!("\nSpec health: {}", dag_board::glyph(spec_health));
    println!("Tasks: {}/{} done", m, trace.total);
    Ok(())
}
```

- [ ] **Step 6: Run the e2e test to verify it passes**

Run: `cargo test --test board 2>&1 | tail -20`
Expected: PASS (1 test).

- [ ] **Step 7: Run the whole suite + clippy**

Run: `cargo test 2>&1 | tail -15 && cargo clippy --all-targets -- -D warnings 2>&1 | tail -15`
Expected: all tests PASS; clippy clean (no warnings).

- [ ] **Step 8: Commit**

```bash
git add src/main.rs tests/board.rs
git commit -m "feat(cli): circuit board <spec> with no-op git adapter (honest Unknown)"
```

---

## Self-Review Notes (author)

- **Spec coverage:** §9 health-from-worktree → Tasks 3-4; §8.3 traceability → Task 5; §8.2 colorless board + glyph + invariant test → Tasks 1-2; §7/§6 CLI → Task 6; no-op adapter honesty → Task 6 (`UnknownGit`). All design sections map to a task.
- **Determinism honesty:** `Option<StageView>` (`?`), `Health::Unknown`, `Traceability.merged = None` cover every "can't tell" path without faking a verdict.
- **Type consistency:** `glyph`/`stage_cell`/`render`/`Board`/`BoardNode` (Tasks 1-2) reused unchanged in Task 6; `health_at_worktree`/`node_health`/`Traceability`/`traceability` (Tasks 3-5) reused in Task 6. Signatures match across tasks.
- **Additive-only:** `render/mod.rs` and `cockpit/mod.rs` get one `pub mod` line each; `main.rs` gets one enum variant, one match arm, imports, and new fns. No foundation/M1 edits.
- **PR note required:** swap `UnknownGit` for the real git adapter (one wiring point in `run_board`) once the git-adapter slice merges.
```
