# Circuit M2 — Slice C: DAG Board + Health Roll-up — Design

**Status:** Draft for review
**Date:** 2026-06-14
**Type:** Slice design (M2, slice C)
**Parent spec:** `2026-06-14-circuit-m2-session-model-design.md` (§7 surfaces, §8.2 colorless
DAG board, §8.3 traceability, §9 health roll-up)
**Builds on:** M2 foundation (slice 0, PR #5, merged) — `flow::stage`, `flow::facts`,
`cockpit::health`, `ports::GitPort`, `session`, M2a `.circuit/` model; M1 `builder` + `indicators`.

---

## 1. Goal

Render the spec-level **DAG board** as its own deterministic mermaid renderer, and **derive
each node's health** by running M1's indicators against the node's worktree. Flow never wears
a health color (§8.2). Everything is derived, nothing stored. CLI-first, deterministically
testable, no GUI.

Scope is exactly: `render/dag_board.rs`, `cockpit/` roll-up-from-worktree, traceability count,
and `circuit board <spec>`. **Out of scope:** git/forge/checkpoint adapter *impls*, spawn,
stage-machine internals, the per-session text rail (that is the git-adapter slice).

---

## 2. What foundation already provides (consumed, not rebuilt)

| Need | Foundation symbol |
|---|---|
| Health vocabulary + math | `cockpit::health::{Health, SessionHealth, rollup_children}` |
| Stage derivation | `flow::stage::{Stage, StageView, derive_stage}` |
| Delivery facts | `flow::facts::{DeliveryFacts, BranchFacts, ReviewState}` |
| Git boundary | `ports::{GitPort, Worktree}` |
| Authored model | `model::{node::DagNode, store::Workspace}`, `model::config::Config.base_branch` |
| Sessions | `session::{SessionRecord, SessionId, SessionKind}` |
| Indicators (M1) | `builder::build_graph`, `indicators::cycles::find_cycles`, `indicators::dependency_rule::violations` |

`cockpit::health` already has the pure roll-up math (`SessionHealth.rollup()`,
`rollup_children`). This slice adds the **IO-adjacent** step that *computes* a `SessionHealth`
by running the M1 indicators against a worktree, plus the board renderer and the CLI.

---

## 3. Cockpit: health from a worktree (`src/cockpit/`)

A new module `cockpit::rollup` (sibling to the existing `cockpit::health`) computes health for
a node by discovering its worktree and running the indicators there.

```rust
// Pure given a discovered path: run M1 build_graph + indicators, count, roll up.
fn health_at_worktree(path: &Path) -> Health            // Critical/Sound (never Unknown)

// Discover the worktree for a branch via the port, then measure it.
// No worktree for that branch  => Health::Unknown (no bare-git reconstruction, §9).
fn node_health<G: GitPort>(git: &G, branch: &str) -> Health
```

- `node_health` calls `git.list_worktrees()`, finds the entry whose `branch == Some(branch)`.
  - **Found:** run `build_graph(path)` + `find_cycles` + `violations`, fold counts into
    `SessionHealth{cycles, dep_violations}`, return `.rollup()` (Sound or Critical).
  - **Not found** (or `list_worktrees` errors): `Health::Unknown`. Honest — §9 forbids
    reconstructing a tree from bare git in M2.
- `build_graph` returning `Err` (path vanished mid-run) also folds to `Unknown` — we measured
  nothing, so we claim nothing.

**Spec roll-up** reuses the existing `rollup_children(&[Health])` (worst-of-children, Unknown
dominates). The board computes each impl node's `node_health`, then the spec line is
`rollup_children` over them. No new math.

**Testing:** a fake `GitPort` whose `list_worktrees` returns a `tempdir` containing a tiny
crate source tree — clean (acyclic, layered) → Sound; with a cycle or an inner→outer edge →
Critical; returning `[]` → Unknown. No real git.

---

## 4. Traceability `m / n` (`src/cockpit/`)

`§8.3`: count DAG nodes whose branch is **merged into base**.

```rust
struct Traceability { merged: Option<usize>, total: usize }   // m / n
fn traceability<G: GitPort>(git: &G, nodes: &[DagNode], base: &str) -> Traceability
```

- `total` = `nodes.len()` — always known from `.circuit/`.
- `merged` = count of nodes where `git.branch_facts(node.branch, base)?.merged_into_base`.
  - If **any** `branch_facts` call errors (git undeterminable, e.g. no-op adapter),
    `merged = None` → rendered `?/n`. We never report a partial/faked merged count.

This is a derived roll-up, so it lives in cockpit next to health. The board CLI prints it; the
per-session rail (other slice) may also consume it later.

---

## 5. The DAG board renderer (`src/render/dag_board.rs`)

A **new** deterministic mermaid renderer — not a reuse of `render::mermaid` (different graph:
nodes are sessions/slices, edges are task deps). Same *house style* (sorted output, local
`node_id` sanitizer reimplemented here), different code.

### 5.1 View model (presentation-owned, decoupled from foundation)

```rust
pub struct BoardNode {
    pub id: String,                 // DAG node id (stable, always present)
    pub depends_on: Vec<String>,    // edges
    pub stage: Option<StageView>,   // None = stage undeterminable (no git) -> "?"
    pub health: Health,             // foundation enum (already has Unknown)
}
pub struct Board { pub nodes: Vec<BoardNode> }

pub fn render(board: &Board) -> String
```

`stage: Option<StageView>` carries the honest git-unknown that `Stage` (no Unknown variant)
cannot. `None` renders `?`; a `StageView{forge_certain:false}` renders the stage name with a
trailing `?` (forge-gated refinement unconfirmed) — distinct presentations for distinct
honesties.

### 5.2 Output shape (deterministic, sorted by node id)

```
graph TD
  auth_slice["auth-slice · Implement · ◍"]
  pay_slice["pay-slice · Review? · ●"]
  auth_slice --> pay_slice
classDef flow fill:#fff,stroke:#999;
class auth_slice,pay_slice flow;
```

- Nodes sorted by id; edges sorted by `(from, to)`. No nondeterminism.
- Label = `<id> · <stage-cell> · <glyph>`. Stage cell: stage name, `Name?` if
  `!forge_certain`, or `?` if `stage == None`.
- **Glyphs** (§9): `●` Sound · `◍` Critical · `?` Unknown · `◐` Warn (reserved; no producer
  yet, so it never appears, but the mapping is total).

### 5.3 The colorless invariant — encoded as a test

The fill/stroke is a single fixed `classDef flow` applied to **every** node regardless of
health. Health lives only in the glyph (label text), never in styling.

The renderer test asserts: **no `classDef`/`class`/`style` line varies with health.** Concrete
assertions:
1. Render two boards identical except every node's `health` flipped (Sound↔Critical↔Unknown).
   The set of `classDef`/`class`/`style` lines is **byte-identical** between them.
2. The only `classDef` is `flow`, and there is exactly one.
3. No styling line contains any per-node id-to-color mapping (health never reaches styling).

Assertion (1) is the strongest form of "flow never wears a health color": styling is provably
a function of structure only, not health.

---

## 6. CLI: `circuit board <spec>` (`src/main.rs`, additive)

```
circuit board <spec> [--path .]
```

1. Load `Workspace`, `require_initialized`, read `Config.base_branch`.
2. `list_dag_nodes()`, filter to `node.spec == <spec>`.
3. For each node build a `BoardNode`:
   - stage: `git.branch_facts(node.branch, base)` →
     - `Err` ⇒ `stage = None` (git undeterminable; short-circuits — `derive_stage` is not
       called). This is the no-op-adapter / git-failure path.
     - `Ok(branch)` ⇒ assemble `DeliveryFacts{branch, review: None}` (forge/checkpoint
       adapters are out of this slice, so review is honestly undeterminable), then
       `stage = Some(derive_stage(&session, &facts))` — yielding the git-floor stage with
       `forge_certain=false`. `session` is the matching `SessionRecord` (`dag_node == id`) or
       a synthesized minimal impl record (the param is unused by M2's `derive_stage`).
   - health: `cockpit::node_health(git, &node.branch)`.
4. Print the board mermaid, then a per-node readout (`<id>  <stage>  <glyph> health`), then the
   spec roll-up glyph (`rollup_children`) and `Tasks: m/n`.

### 6.1 No-op git adapter (until the git-adapter slice merges)

This slice does not own a real `GitPort` impl. It injects a local **`UnknownGit`** adapter:

- `branch_facts(..) -> Err(...)`  → every node's `stage = None` (`?`), traceability `?/n`.
- `list_worktrees() -> Ok(vec![])` → every node's health `Unknown`.
- `create_branch`/`add_worktree` → `Err` (unsupported; board never calls them).

Honest by construction: with no git, the board shows `?` stages, `?` health, `?/n` tasks — it
fabricates nothing. **PR note:** when the git-adapter slice lands, swap `UnknownGit` for the
real adapter at the one CLI wiring point; the board/cockpit code is already generic over
`GitPort` and needs no change. `UnknownGit` lives in `main.rs` (or a tiny `adapters` stub
clearly owned by this slice) to avoid colliding with the git-adapter slice's `adapters/git.rs`.

---

## 7. Module layout (additive only)

```
src/
  cockpit/
    mod.rs        # + pub mod rollup;
    health.rs     # foundation (unchanged)
    rollup.rs     # NEW: node_health, health_at_worktree, traceability
  render/
    mod.rs        # + pub mod dag_board;
    mermaid.rs    # M1 (unchanged)
    dag_board.rs  # NEW: Board, BoardNode, render
  main.rs         # + `Board` subcommand + UnknownGit no-op adapter (additive)
```

`src/lib.rs`, `src/main.rs`, `src/render/mod.rs`, `src/cockpit/mod.rs` are touched **additively
only** (new `pub mod` / new match arm). No edits to foundation or M1 logic.
`#![forbid(unsafe_code)]` holds. `thiserror` at boundaries, `anyhow` in the CLI.

---

## 8. Testing strategy

- **`render::dag_board`** — pure unit tests over `Board` literals: deterministic sorted output,
  label formatting (all stage cells incl. `None` and `forge_certain=false`, all glyphs), and
  the **colorless invariant** test (§5.3). The bulk of the suite.
- **`cockpit::rollup`** — fake `GitPort` + `tempdir` worktrees: Sound / Critical / Unknown
  (no worktree) / Unknown (build error); `traceability` with merged/unmerged/erroring facts.
- **End-to-end (`assert_cmd`)** — `init`, create a spec, add DAG nodes, `circuit board <spec>`;
  assert the mermaid header, a node label, the colorless `classDef`, and (with `UnknownGit`)
  `?` stages/health and `?/n`. The §1 exit-criteria walk for this slice.

No network, no real `git`/`gh` in the suite.

---

## 9. Traceability to parent spec

| Parent § | This slice |
|---|---|
| §7 DAG board surface | §5 renderer, §6 CLI |
| §8.2 colorless flow, separate health glyph | §5.2 label, §5.3 invariant test |
| §8.3 traceability `m/n` | §4 |
| §9 health roll-up, Unknown when no worktree | §3 |
| §5.3 determinism honesty (never fake a verdict) | §3 (Unknown), §5.1 (`Option<StageView>`), §6.1 (no-op adapter) |
```
