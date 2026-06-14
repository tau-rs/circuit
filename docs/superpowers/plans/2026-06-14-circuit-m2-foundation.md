# Circuit M2 — Foundation (slice 0) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The pure, fully-unit-tested shared contracts every other M2 slice depends on — session record + ULID identity, delivery facts, the pure stage machine, the health roll-up, and the IO port traits (signatures only) — with no external IO.

**Architecture:** Hexagonal, extending M1/M2a. New pure domain modules (`src/session/`, `src/flow/`, `src/cockpit/`) and the outbound port traits (`src/ports.rs`) carry types and one pure function (`derive_stage`) with no IO. The single IO touchpoint is extending M2a's `Workspace` (`src/model/store.rs`) with session load/save/list, exactly mirroring `list_dag_nodes`. Adapters and CLI that *use* these contracts are out of scope (later slices).

**Tech Stack:** Rust 2021; `ulid` (0.x→1) with the `serde` feature for identity; `serde` + `toml` (already present) for the record schema; `thiserror` (already present) reserved for the adapter boundary; dev: `tempfile` (already present). `#![forbid(unsafe_code)]` continues.

**Source of truth:** `docs/superpowers/specs/2026-06-14-circuit-m2-foundation-design.md` (and its parent `2026-06-14-circuit-m2-session-model-design.md`).

**Commit convention:** Conventional commits, imperative. Every commit appends the trailer:
`Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>` (omitted from the `-m` snippets below for brevity — add it to each).

**Parallelization (task DAG):** After Task 1, Tasks 2 (`session`), 3 (`flow/facts`), 4 (`cockpit/health`) are independent and parallel-eligible. Task 5 (`flow/stage`) depends on Tasks 2 + 3. Task 6 (`ports`) depends on Task 3. Task 7 (`Workspace` sessions) depends on Task 2. Tasks 5–7 do not share a file, so they may also run in parallel once their deps land.

---

## File structure

| File | Responsibility |
|---|---|
| `Cargo.toml` | Add `ulid` dep (serde feature) |
| `src/lib.rs` | Declare `cockpit`, `flow`, `ports`, `session` modules |
| `src/session/mod.rs` | `SessionId` (ULID identity), `SessionKind`, `SessionRecord` (serde) |
| `src/flow/mod.rs` | Declare `facts`, `stage` submodules |
| `src/flow/facts.rs` | `BranchFacts`, `ReviewState`, `DeliveryFacts` (plain data) |
| `src/flow/stage.rs` | `Stage`, `StageView`, `derive_stage` (pure fn) |
| `src/cockpit/mod.rs` | Declare `health` submodule |
| `src/cockpit/health.rs` | `Health` (Ord), `SessionHealth::rollup`, `rollup_children` |
| `src/ports.rs` | `GitPort`, `ForgePort`, `CheckpointStore` traits + `Worktree` (signatures only) |
| `src/model/store.rs` | `Workspace` session methods (filesystem IO), mirroring `list_dag_nodes` |

---

## Task 1: Dependencies and module scaffolding

**Files:**
- Modify: `Cargo.toml`, `src/lib.rs`
- Create: `src/session/mod.rs`, `src/flow/mod.rs`, `src/flow/facts.rs`, `src/flow/stage.rs`, `src/cockpit/mod.rs`, `src/cockpit/health.rs`, `src/ports.rs`

- [ ] **Step 1: Add the `ulid` dependency**

In `Cargo.toml`, under `[dependencies]`, add after the existing `toml` line:

```toml
ulid = { version = "1", features = ["serde"] }
```

- [ ] **Step 2: Declare the new modules in the library root**

In `src/lib.rs`, extend the module block so it reads (alphabetical):

```rust
pub mod builder;
pub mod cockpit;
pub mod dag;
pub mod flow;
pub mod graph;
pub mod indicators;
pub mod lang;
pub mod layer;
pub mod model;
pub mod ports;
pub mod render;
pub mod session;
```

- [ ] **Step 3: Create the submodule declarations and empty stubs**

Create `src/flow/mod.rs`:

```rust
pub mod facts;
pub mod stage;
```

Create `src/cockpit/mod.rs`:

```rust
pub mod health;
```

Create these as **empty** files (an empty file is a valid Rust module; later tasks fill them):
- `src/session/mod.rs`
- `src/flow/facts.rs`
- `src/flow/stage.rs`
- `src/cockpit/health.rs`
- `src/ports.rs`

- [ ] **Step 4: Verify the crate still compiles**

Run: `cargo build`
Expected: success (downloads `ulid` on first run). Empty modules are valid Rust.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock src/lib.rs src/session/ src/flow/ src/cockpit/ src/ports.rs
git commit -m "chore: add ulid dep and session/flow/cockpit/ports scaffolding (M2 foundation)"
```

---

## Task 2: Session domain — `SessionId`, `SessionKind`, `SessionRecord`  _(parallel-eligible with Tasks 3, 4)_

**Files:**
- Modify: `src/session/mod.rs`

- [ ] **Step 1: Write the schema and its tests**

Replace `src/session/mod.rs` with:

```rust
//! Session identity and the authored session record (`.circuit/sessions/<id>.toml`).
//! Pure: serde + a single clock-reading id generator, nothing else.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use ulid::Ulid;

/// A session's stable, ULID-style identity. It **precedes the branch**: a session
/// exists at `Draft` before any branch is cut, which is why the branch name
/// cannot be the identity (§4 of the M2 design).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SessionId(Ulid);

impl SessionId {
    /// Mint a fresh id. This is the ONLY clock-reading call in the foundation
    /// slice; the impurity is isolated here so everything else stays pure.
    pub fn generate() -> Self {
        Self(Ulid::new())
    }
}

impl fmt::Display for SessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Canonical 26-char Crockford base32 form.
        write!(f, "{}", self.0)
    }
}

impl FromStr for SessionId {
    type Err = ulid::DecodeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ulid::from_string(s).map(Self)
    }
}

/// The three session kinds (the fractal model of §4.2). Serializes lowercase
/// (`"spec" | "impl" | "fix"`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionKind {
    Spec,
    Impl,
    Fix,
}

/// `.circuit/sessions/<id>.toml` — a session's authored intent. Only intent is
/// stored: no stage, no worktree path, no branch *state* (all derived, §3.3).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionRecord {
    pub schema_version: u32,
    pub id: SessionId,
    pub kind: SessionKind,
    /// Spec id (impl/fix sessions); `None` for a spec session.
    #[serde(default)]
    pub parent: Option<String>,
    /// DAG node id this session executes (impl/fix); `None` for a spec session.
    #[serde(default)]
    pub dag_node: Option<String>,
    /// Authored branch bridge; `None` until spawned (a Draft session, or a spec
    /// session, owns no branch). The worktree path is never stored.
    #[serde(default)]
    pub branch: Option<String>,
    /// For fix sessions: the non-green sub-indicator this session targets.
    #[serde(default)]
    pub fixes_indicator: Option<String>,
}

impl SessionRecord {
    /// A spec session: owns the DAG, writes no code, has no branch.
    pub fn spec(id: SessionId) -> Self {
        Self {
            schema_version: 1,
            id,
            kind: SessionKind::Spec,
            parent: None,
            dag_node: None,
            branch: None,
            fixes_indicator: None,
        }
    }

    /// An implementation session executing one DAG node on its own branch.
    pub fn impl_(
        id: SessionId,
        parent: impl Into<String>,
        dag_node: impl Into<String>,
        branch: impl Into<String>,
    ) -> Self {
        Self {
            schema_version: 1,
            id,
            kind: SessionKind::Impl,
            parent: Some(parent.into()),
            dag_node: Some(dag_node.into()),
            branch: Some(branch.into()),
            fixes_indicator: None,
        }
    }

    /// A fix session: a scoped child targeting one non-green sub-indicator.
    pub fn fix(
        id: SessionId,
        parent: impl Into<String>,
        dag_node: impl Into<String>,
        branch: impl Into<String>,
        fixes_indicator: impl Into<String>,
    ) -> Self {
        Self {
            schema_version: 1,
            id,
            kind: SessionKind::Fix,
            parent: Some(parent.into()),
            dag_node: Some(dag_node.into()),
            branch: Some(branch.into()),
            fixes_indicator: Some(fixes_indicator.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // A canonical, valid 26-char ULID for hand-authored parse tests.
    const SAMPLE_ULID: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";

    #[test]
    fn session_id_round_trips_through_string() {
        let id = SessionId::generate();
        let s = id.to_string();
        assert_eq!(s.len(), 26);
        let parsed: SessionId = s.parse().unwrap();
        assert_eq!(parsed, id);
    }

    #[test]
    fn session_id_rejects_an_invalid_string() {
        assert!("not-a-ulid".parse::<SessionId>().is_err());
    }

    #[test]
    fn spec_session_has_no_parent_dag_node_or_branch() {
        let s = SessionRecord::spec(SessionId::generate());
        assert_eq!(s.kind, SessionKind::Spec);
        assert!(s.parent.is_none());
        assert!(s.dag_node.is_none());
        assert!(s.branch.is_none());
        assert!(s.fixes_indicator.is_none());
    }

    #[test]
    fn impl_session_round_trips_through_toml() {
        let s = SessionRecord::impl_(
            SessionId::generate(),
            "checkout",
            "auth-slice",
            "impl/checkout-auth",
        );
        let text = toml::to_string_pretty(&s).unwrap();
        let parsed: SessionRecord = toml::from_str(&text).unwrap();
        assert_eq!(parsed, s);
    }

    #[test]
    fn fix_session_records_its_indicator() {
        let s = SessionRecord::fix(
            SessionId::generate(),
            "checkout",
            "auth-slice",
            "fix/checkout-auth-cycles",
            "cycles",
        );
        assert_eq!(s.kind, SessionKind::Fix);
        assert_eq!(s.fixes_indicator.as_deref(), Some("cycles"));
    }

    #[test]
    fn parses_a_hand_authored_impl_session() {
        let text = format!(
            r#"
            schema_version = 1
            id = "{SAMPLE_ULID}"
            kind = "impl"
            parent = "checkout"
            dag_node = "auth-slice"
            branch = "impl/checkout-auth"
            "#
        );
        let s: SessionRecord = toml::from_str(&text).unwrap();
        assert_eq!(s.kind, SessionKind::Impl);
        assert_eq!(s.parent.as_deref(), Some("checkout"));
        assert_eq!(s.branch.as_deref(), Some("impl/checkout-auth"));
        assert!(s.fixes_indicator.is_none());
    }

    #[test]
    fn parses_a_spec_session_with_options_omitted() {
        let text = format!(
            "schema_version = 1\nid = \"{SAMPLE_ULID}\"\nkind = \"spec\"\n"
        );
        let s: SessionRecord = toml::from_str(&text).unwrap();
        assert_eq!(s.kind, SessionKind::Spec);
        assert!(s.parent.is_none());
        assert!(s.branch.is_none());
    }
}
```

- [ ] **Step 2: Run the tests**

Run: `cargo test --lib session`
Expected: PASS (7 tests).

- [ ] **Step 3: Commit**

```bash
git add src/session/mod.rs
git commit -m "feat: session identity and record schema (spec/impl/fix)"
```

---

## Task 3: Delivery facts — `BranchFacts`, `ReviewState`, `DeliveryFacts`  _(parallel-eligible with Tasks 2, 4)_

**Files:**
- Modify: `src/flow/facts.rs`

- [ ] **Step 1: Write the types and their tests**

Replace `src/flow/facts.rs` with:

```rust
//! Plain delivery facts consumed by the pure stage machine. No IO lives here;
//! the git/forge adapters populate these and `derive_stage` reads them (§5.2).

/// Git-derived facts about a session's branch relative to the base branch.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BranchFacts {
    /// The branch ref exists in git.
    pub exists: bool,
    /// `git rev-list base..branch` — informational rail decoration, not a stage gate.
    pub commits_ahead_of_base: usize,
    /// Non-empty diff vs the merge-base — the Project -> Implement boundary.
    pub has_substantive_changes: bool,
    /// The branch tip is an ancestor of base (git-only, offline-derivable).
    pub merged_into_base: bool,
}

/// Review state of a session's branch, from the forge OR a local checkpoint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReviewState {
    /// No PR / checkpoint exists — a *known* fact (distinct from undeterminable).
    None,
    /// A PR (or `self-review` checkpoint) is open.
    Open,
    /// Approved / mergeable, not yet landed.
    Approved,
    /// Merged via the forge.
    Merged,
    /// Closed without merging.
    Closed,
}

/// Everything `derive_stage` needs. `review` is `Option` so "undeterminable"
/// (forge unreachable AND no checkpoint) is distinct from a known `None` — the
/// determinism-honesty primitive (§5.3).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DeliveryFacts {
    pub branch: BranchFacts,
    /// `Some(state)` when known; `None` when review state is undeterminable.
    pub review: Option<ReviewState>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn branch_facts_default_is_empty() {
        let b = BranchFacts::default();
        assert!(!b.exists);
        assert_eq!(b.commits_ahead_of_base, 0);
        assert!(!b.has_substantive_changes);
        assert!(!b.merged_into_base);
    }

    #[test]
    fn delivery_facts_default_has_undeterminable_review() {
        let d = DeliveryFacts::default();
        assert_eq!(d.branch, BranchFacts::default());
        assert_eq!(d.review, None);
    }

    #[test]
    fn known_no_pr_differs_from_undeterminable() {
        let known_no_pr: Option<ReviewState> = Some(ReviewState::None);
        let undeterminable: Option<ReviewState> = None;
        assert_ne!(known_no_pr, undeterminable);
    }
}
```

- [ ] **Step 2: Run the tests**

Run: `cargo test --lib flow::facts`
Expected: PASS (3 tests).

- [ ] **Step 3: Commit**

```bash
git add src/flow/facts.rs
git commit -m "feat: delivery facts (branch facts, review state, Option-honest review)"
```

---

## Task 4: Health roll-up — `Health`, `SessionHealth`, `rollup_children`  _(parallel-eligible with Tasks 2, 3)_

**Files:**
- Modify: `src/cockpit/health.rs`

- [ ] **Step 1: Write the roll-up and its tests**

Replace `src/cockpit/health.rs` with:

```rust
//! Health roll-up. Health is derived (run M1's indicators against a branch's
//! worktree), never stored (§9). This module is the pure roll-up logic only.

/// Session health. `Ord` is derived from declaration order, so the ascending
/// chain `Sound < Warn < Critical < Unknown` makes `Unknown` sort highest;
/// `children.max()` then lets an unmeasurable child dominate a spec's roll-up.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Health {
    /// No violations.
    Sound,
    /// Reserved: no advisory-only sub-indicator produces this yet.
    Warn,
    /// At least one cycle or dependency-rule violation.
    Critical,
    /// Unmeasurable (e.g. no worktree to run indicators against).
    Unknown,
}

/// The two M1 indicator counts for one impl session's worktree.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SessionHealth {
    pub cycles: usize,
    pub dep_violations: usize,
}

impl SessionHealth {
    /// Impl-session roll-up: any cycle or dependency-rule violation => Critical,
    /// else Sound. Never yields Unknown (the adapter supplies that when a
    /// worktree is absent) and never Warn (no producer yet).
    pub fn rollup(&self) -> Health {
        if self.cycles > 0 || self.dep_violations > 0 {
            Health::Critical
        } else {
            Health::Sound
        }
    }
}

/// Spec-session roll-up: worst-of-children = `children.max()`. Empty => Sound
/// (a spec with no children is vacuously sound); an `Unknown` child dominates.
pub fn rollup_children(children: &[Health]) -> Health {
    children.iter().copied().max().unwrap_or(Health::Sound)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_sorts_highest() {
        assert!(Health::Unknown > Health::Critical);
        assert!(Health::Critical > Health::Warn);
        assert!(Health::Warn > Health::Sound);
    }

    #[test]
    fn rollup_is_sound_when_no_violations() {
        assert_eq!(SessionHealth::default().rollup(), Health::Sound);
    }

    #[test]
    fn rollup_is_critical_on_any_violation() {
        assert_eq!(
            SessionHealth { cycles: 1, dep_violations: 0 }.rollup(),
            Health::Critical
        );
        assert_eq!(
            SessionHealth { cycles: 0, dep_violations: 3 }.rollup(),
            Health::Critical
        );
    }

    #[test]
    fn rollup_children_empty_is_sound() {
        assert_eq!(rollup_children(&[]), Health::Sound);
    }

    #[test]
    fn rollup_children_takes_the_worst() {
        assert_eq!(
            rollup_children(&[Health::Sound, Health::Critical, Health::Sound]),
            Health::Critical
        );
    }

    #[test]
    fn rollup_children_unknown_dominates_critical() {
        assert_eq!(
            rollup_children(&[Health::Sound, Health::Critical, Health::Unknown]),
            Health::Unknown
        );
    }
}
```

- [ ] **Step 2: Run the tests**

Run: `cargo test --lib cockpit::health`
Expected: PASS (6 tests).

- [ ] **Step 3: Commit**

```bash
git add src/cockpit/health.rs
git commit -m "feat: health roll-up (Unknown sorts highest, worst-of-children)"
```

---

## Task 5: Stage machine — `Stage`, `StageView`, `derive_stage`  _(depends on Tasks 2 + 3)_

**Files:**
- Modify: `src/flow/stage.rs`

- [ ] **Step 1: Write the stage machine and its exhaustive truth-table tests**

Replace `src/flow/stage.rs` with:

```rust
//! The pure flow-stage derivation. Stage is never stored; it is a function of
//! delivery facts (§5). Forge-gated stages are reported honestly: when review
//! state is undeterminable we return the git-floor stage with `forge_certain`
//! false rather than faking Review/Merge/Done (§5.3).

use crate::flow::facts::{DeliveryFacts, ReviewState};
use crate::session::SessionRecord;

/// The six-stage flow spine. No `Unknown` variant: stage uncertainty lives in
/// `StageView.forge_certain`, keeping `Stage` the clean spine.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Draft,
    Project,
    Implement,
    Review,
    Merge,
    Done,
}

/// A derived stage plus whether forge-gated refinement could be confirmed.
/// `forge_certain == false` means "at least `stage`, but the Review/Merge/Done
/// refinement is undeterminable" — we never fake a forge-gated verdict.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StageView {
    pub stage: Stage,
    pub forge_certain: bool,
}

impl StageView {
    fn certain(stage: Stage) -> Self {
        Self {
            stage,
            forge_certain: true,
        }
    }
}

/// Derive a session's flow stage from delivery facts. Pure; never stored.
///
/// `_session` is reserved: M3's projection-approved marker will refine the
/// Project -> Implement gate using session/projection state (§5.1). Unused in M2.
pub fn derive_stage(_session: &SessionRecord, facts: &DeliveryFacts) -> StageView {
    let b = &facts.branch;

    // 1. No branch in git yet -> Draft (git-settled).
    if !b.exists {
        return StageView::certain(Stage::Draft);
    }
    // 2. Merged into base -> Done (git-only, offline-confident).
    if b.merged_into_base {
        return StageView::certain(Stage::Done);
    }
    // 3. Branch exists but no substantive commits -> Project.
    if !b.has_substantive_changes {
        return StageView::certain(Stage::Project);
    }
    // 4-9. Substantive changes, not merged: refine by review state.
    match facts.review {
        // Undeterminable: honest git-floor Implement, forge refinement Unknown.
        None => StageView {
            stage: Stage::Implement,
            forge_certain: false,
        },
        Some(ReviewState::None) => StageView::certain(Stage::Implement),
        Some(ReviewState::Open) => StageView::certain(Stage::Review),
        Some(ReviewState::Approved) => StageView::certain(Stage::Merge),
        Some(ReviewState::Merged) => StageView::certain(Stage::Done),
        Some(ReviewState::Closed) => StageView::certain(Stage::Implement),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flow::facts::BranchFacts;
    use crate::session::SessionId;

    fn session() -> SessionRecord {
        SessionRecord::impl_(
            SessionId::generate(),
            "checkout",
            "auth-slice",
            "impl/checkout-auth",
        )
    }

    /// Build facts from the four axes that drive the truth table.
    fn facts(
        exists: bool,
        substantive: bool,
        merged: bool,
        review: Option<ReviewState>,
    ) -> DeliveryFacts {
        DeliveryFacts {
            branch: BranchFacts {
                exists,
                commits_ahead_of_base: if substantive { 3 } else { 0 },
                has_substantive_changes: substantive,
                merged_into_base: merged,
            },
            review,
        }
    }

    // Row 1
    #[test]
    fn no_branch_is_draft() {
        let v = derive_stage(&session(), &facts(false, false, false, None));
        assert_eq!(v, StageView { stage: Stage::Draft, forge_certain: true });
    }

    // Row 2 — git-only, even with undeterminable review.
    #[test]
    fn merged_is_done_even_offline() {
        let v = derive_stage(&session(), &facts(true, true, true, None));
        assert_eq!(v, StageView { stage: Stage::Done, forge_certain: true });
    }

    // Row 3
    #[test]
    fn branch_without_substantive_changes_is_project() {
        let v = derive_stage(&session(), &facts(true, false, false, Some(ReviewState::None)));
        assert_eq!(v, StageView { stage: Stage::Project, forge_certain: true });
    }

    // Row 4 — the one honest-Unknown case.
    #[test]
    fn substantive_with_undeterminable_review_is_implement_uncertain() {
        let v = derive_stage(&session(), &facts(true, true, false, None));
        assert_eq!(v, StageView { stage: Stage::Implement, forge_certain: false });
    }

    // Row 5
    #[test]
    fn substantive_no_pr_is_implement() {
        let v = derive_stage(&session(), &facts(true, true, false, Some(ReviewState::None)));
        assert_eq!(v, StageView { stage: Stage::Implement, forge_certain: true });
    }

    // Row 6
    #[test]
    fn open_pr_is_review() {
        let v = derive_stage(&session(), &facts(true, true, false, Some(ReviewState::Open)));
        assert_eq!(v, StageView { stage: Stage::Review, forge_certain: true });
    }

    // Row 7
    #[test]
    fn approved_pr_is_merge() {
        let v = derive_stage(&session(), &facts(true, true, false, Some(ReviewState::Approved)));
        assert_eq!(v, StageView { stage: Stage::Merge, forge_certain: true });
    }

    // Row 8
    #[test]
    fn forge_merged_is_done() {
        let v = derive_stage(&session(), &facts(true, true, false, Some(ReviewState::Merged)));
        assert_eq!(v, StageView { stage: Stage::Done, forge_certain: true });
    }

    // Row 9
    #[test]
    fn closed_pr_is_implement() {
        let v = derive_stage(&session(), &facts(true, true, false, Some(ReviewState::Closed)));
        assert_eq!(v, StageView { stage: Stage::Implement, forge_certain: true });
    }
}
```

- [ ] **Step 2: Run the tests**

Run: `cargo test --lib flow::stage`
Expected: PASS (9 tests — one per truth-table row).

- [ ] **Step 3: Commit**

```bash
git add src/flow/stage.rs
git commit -m "feat: pure stage machine with forge-honest Unknown refinement"
```

---

## Task 6: Ports — `GitPort`, `ForgePort`, `CheckpointStore` (signatures only)  _(depends on Task 3)_

**Files:**
- Modify: `src/ports.rs`

- [ ] **Step 1: Write the trait signatures and fake-impl tests**

Replace `src/ports.rs` with:

```rust
//! Outbound port traits — the IO boundary the domain depends on. Signatures
//! only; implementations live in the adapter slices (git/`gh` shell-out,
//! checkpoint store). Each port carries an associated `Error` so adapters bring
//! their own `thiserror` type without the foundation guessing failure modes (§6).

use std::path::{Path, PathBuf};

use crate::flow::facts::{BranchFacts, ReviewState};

/// One entry from `git worktree list`. The path is derived at runtime and never
/// stored; `branch` is `None` for a detached-HEAD worktree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Worktree {
    pub path: PathBuf,
    pub branch: Option<String>,
}

/// Git operations: branch facts plus worktree orchestration. Offline-capable.
pub trait GitPort {
    type Error: std::error::Error + Send + Sync + 'static;

    fn branch_facts(&self, branch: &str, base: &str) -> Result<BranchFacts, Self::Error>;
    fn create_branch(&self, branch: &str, base: &str) -> Result<(), Self::Error>;
    fn add_worktree(&self, branch: &str, path: &Path) -> Result<(), Self::Error>;
    fn list_worktrees(&self) -> Result<Vec<Worktree>, Self::Error>;
}

/// Forge operations (GitHub via `gh` in M2). `review_state` returning `Err`
/// (forge unreachable) is mapped by the caller into `DeliveryFacts.review = None`.
pub trait ForgePort {
    type Error: std::error::Error + Send + Sync + 'static;

    fn review_state(&self, branch: &str) -> Result<ReviewState, Self::Error>;
    fn create_pr(
        &self,
        branch: &str,
        base: &str,
        title: &str,
        body: &str,
    ) -> Result<(), Self::Error>;
    fn merge(&self, branch: &str) -> Result<(), Self::Error>;
    fn update_from_base(&self, branch: &str, base: &str) -> Result<(), Self::Error>;
}

/// Local synthetic-PR review state from `.circuit/checkpoints/`, the no-remote
/// fallback. Returns `Ok(ReviewState::None)` when no checkpoint exists.
pub trait CheckpointStore {
    type Error: std::error::Error + Send + Sync + 'static;

    fn review_state(&self, session: &str) -> Result<ReviewState, Self::Error>;
}

#[cfg(test)]
mod tests {
    use super::*;

    // A trivial error type satisfying the associated-error bound.
    #[derive(Debug)]
    struct FakeError;

    impl std::fmt::Display for FakeError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "fake error")
        }
    }
    impl std::error::Error for FakeError {}

    struct FakeGit;
    impl GitPort for FakeGit {
        type Error = FakeError;
        fn branch_facts(&self, _branch: &str, _base: &str) -> Result<BranchFacts, Self::Error> {
            Ok(BranchFacts::default())
        }
        fn create_branch(&self, _branch: &str, _base: &str) -> Result<(), Self::Error> {
            Ok(())
        }
        fn add_worktree(&self, _branch: &str, _path: &Path) -> Result<(), Self::Error> {
            Ok(())
        }
        fn list_worktrees(&self) -> Result<Vec<Worktree>, Self::Error> {
            Ok(vec![Worktree {
                path: PathBuf::from("/tmp/wt"),
                branch: Some("impl/x".to_string()),
            }])
        }
    }

    struct FakeForge;
    impl ForgePort for FakeForge {
        type Error = FakeError;
        fn review_state(&self, _branch: &str) -> Result<ReviewState, Self::Error> {
            Ok(ReviewState::Open)
        }
        fn create_pr(
            &self,
            _branch: &str,
            _base: &str,
            _title: &str,
            _body: &str,
        ) -> Result<(), Self::Error> {
            Ok(())
        }
        fn merge(&self, _branch: &str) -> Result<(), Self::Error> {
            Ok(())
        }
        fn update_from_base(&self, _branch: &str, _base: &str) -> Result<(), Self::Error> {
            Ok(())
        }
    }

    struct FakeCheckpoints;
    impl CheckpointStore for FakeCheckpoints {
        type Error = FakeError;
        fn review_state(&self, _session: &str) -> Result<ReviewState, Self::Error> {
            Ok(ReviewState::None)
        }
    }

    #[test]
    fn ports_are_implementable_and_callable() {
        let g = FakeGit;
        assert_eq!(g.branch_facts("b", "main").unwrap(), BranchFacts::default());
        assert!(g.create_branch("b", "main").is_ok());
        assert!(g.add_worktree("b", Path::new("/tmp/wt")).is_ok());
        assert_eq!(g.list_worktrees().unwrap().len(), 1);

        let f = FakeForge;
        assert_eq!(f.review_state("b").unwrap(), ReviewState::Open);
        assert!(f.create_pr("b", "main", "t", "body").is_ok());
        assert!(f.merge("b").is_ok());
        assert!(f.update_from_base("b", "main").is_ok());

        let c = FakeCheckpoints;
        assert_eq!(c.review_state("session-id").unwrap(), ReviewState::None);
    }
}
```

- [ ] **Step 2: Run the tests**

Run: `cargo test --lib ports`
Expected: PASS (1 test; the value is that the fake impls compile against the trait signatures).

- [ ] **Step 3: Commit**

```bash
git add src/ports.rs
git commit -m "feat: GitPort/ForgePort/CheckpointStore trait contracts (signatures only)"
```

---

## Task 7: `Workspace` session methods  _(depends on Task 2)_

**Files:**
- Modify: `src/model/store.rs`

- [ ] **Step 1: Add the session import**

In `src/model/store.rs`, add below the existing `use super::{...};` block:

```rust
use crate::session::SessionRecord;
```

- [ ] **Step 2: Add the session methods to `impl Workspace`**

Inside `impl Workspace { ... }`, after the `list_dag_nodes` method (just before the closing `}` of the impl block), add:

```rust
    pub fn sessions_dir(&self) -> PathBuf {
        self.circuit_dir().join("sessions")
    }

    pub fn session_path(&self, id: &str) -> PathBuf {
        self.sessions_dir().join(format!("{id}.toml"))
    }

    pub fn load_session(&self, id: &str) -> Result<SessionRecord, ModelError> {
        load_toml(&self.session_path(id))
    }

    pub fn save_session(&self, s: &SessionRecord) -> Result<(), ModelError> {
        save_toml(&self.session_path(&s.id.to_string()), s)
    }

    /// All session records, sorted by file path for deterministic order.
    pub fn list_sessions(&self) -> Result<Vec<SessionRecord>, ModelError> {
        let dir = self.sessions_dir();
        let mut sessions = Vec::new();
        if dir.is_dir() {
            let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)
                .map_err(|source| ModelError::Io {
                    path: dir.display().to_string(),
                    source,
                })?
                // Best-effort: skip entries we can't stat (the open dir already succeeded).
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("toml"))
                .collect();
            paths.sort();
            for p in paths {
                sessions.push(load_toml(&p)?);
            }
        }
        Ok(sessions)
    }
```

- [ ] **Step 3: Add the tests**

In `src/model/store.rs`, inside the existing `#[cfg(test)] mod tests { ... }` block, append these two tests (after `list_dag_nodes_returns_sorted_and_empty_when_absent`):

```rust
    #[test]
    fn session_round_trips_through_disk() {
        use crate::session::{SessionId, SessionRecord};
        let dir = tempfile::tempdir().unwrap();
        let ws = Workspace::new(dir.path());

        let s = SessionRecord::impl_(
            SessionId::generate(),
            "checkout",
            "auth-slice",
            "impl/checkout-auth",
        );
        ws.save_session(&s).unwrap();
        assert_eq!(ws.load_session(&s.id.to_string()).unwrap(), s);
    }

    #[test]
    fn list_sessions_is_sorted_and_empty_when_absent() {
        use crate::session::{SessionId, SessionRecord};
        let dir = tempfile::tempdir().unwrap();
        let ws = Workspace::new(dir.path());
        assert!(ws.list_sessions().unwrap().is_empty());

        // Spec sessions exercise the all-`None` options serialization path too.
        let a = SessionRecord::spec(SessionId::generate());
        let b = SessionRecord::spec(SessionId::generate());
        ws.save_session(&a).unwrap();
        ws.save_session(&b).unwrap();

        let got = ws.list_sessions().unwrap();
        assert_eq!(got.len(), 2);

        let mut expected_ids = vec![a.id.to_string(), b.id.to_string()];
        expected_ids.sort();
        let got_ids: Vec<String> = got.iter().map(|s| s.id.to_string()).collect();
        assert_eq!(got_ids, expected_ids);
    }
```

- [ ] **Step 4: Run the store tests**

Run: `cargo test --lib model::store`
Expected: PASS (5 tests — the 3 M2a tests plus the 2 new session tests).

- [ ] **Step 5: Run the full suite**

Run: `cargo test`
Expected: PASS (all M1 + M2a + foundation unit and integration tests). Then run `cargo clippy --all-targets -- -D warnings` and expect no warnings.

- [ ] **Step 6: Commit**

```bash
git add src/model/store.rs
git commit -m "feat: Workspace session load/save/list mirroring DAG nodes"
```

---

## Self-Review

**Spec coverage (against `2026-06-14-circuit-m2-foundation-design.md`):**
- §3.1 `SessionId` ULID identity (generate/FromStr/Display, serde-transparent) → Task 2 ✓
- §3.2 `SessionKind` (spec/impl/fix, lowercase) → Task 2 ✓
- §3.3 `SessionRecord` schema (schema_version, Option parent/dag_node/branch/fixes_indicator) → Task 2 ✓
- §4 `BranchFacts`, `ReviewState`, `DeliveryFacts` with `review: Option` honesty → Task 3 ✓
- §5 `Stage` (no Unknown), `StageView { stage, forge_certain }`, `derive_stage` truth table (9 rows) → Task 5 ✓
- §6 `Health` (Ord, Unknown highest), `SessionHealth::rollup`, `rollup_children` → Task 4 ✓
- §7 `GitPort`/`ForgePort`/`CheckpointStore` traits + `Worktree`, associated errors, signatures only → Task 6 ✓
- §8 `Workspace` session methods mirroring `list_dag_nodes` → Task 7 ✓
- §9 wiring (`ulid` dep, lib.rs/flow/cockpit module decls) → Task 1 ✓
- §12 testing: pure unit tests with literals, fake-impl port test, tempdir round-trip → every task ✓
- **Out of scope (later slices, correctly absent):** any port *impl*, git/`gh` shell-out, spawn/worktree orchestration, flow rail / DAG-board renderers, CLI commands, computing `DeliveryFacts` from a real repo.

**Placeholder scan:** none — every step has complete code and exact commands. The single `_session` unused parameter in `derive_stage` is a documented reserved hook (§5.1), not a placeholder. The `Warn` health variant is a documented reserved value, not a stub.

**Type consistency (across tasks):**
- `SessionId` (Task 2): `generate()`, `Display`, `FromStr`; used by Tasks 5 (test fixtures) and 7 (`s.id.to_string()` for the filename). Consistent.
- `SessionRecord` constructors `spec(id)` / `impl_(id, parent, dag_node, branch)` / `fix(id, parent, dag_node, branch, fixes_indicator)` (Task 2) match every call site in Tasks 5 and 7. Consistent.
- `BranchFacts` fields (`exists`, `commits_ahead_of_base`, `has_substantive_changes`, `merged_into_base`) and `ReviewState` variants (Task 3) match the `facts(..)` helper and the `match` in Task 5, and the `BranchFacts`/`ReviewState` use in Task 6. Consistent.
- `DeliveryFacts { branch, review: Option<ReviewState> }` (Task 3) matches `derive_stage` (Task 5). Consistent.
- `derive_stage(&SessionRecord, &DeliveryFacts) -> StageView` (Task 5) — sole consumer is its own tests in M2; no external call site in this slice. Consistent.
- `Health` / `SessionHealth::rollup` / `rollup_children(&[Health]) -> Health` (Task 4) — self-contained; no cross-task call. Consistent.
- Port traits + `Worktree` (Task 6) reference only `flow::facts` types (Task 3). No impl elsewhere in this slice. Consistent.
- `Workspace` new methods (Task 7) reuse the existing M2a `circuit_dir`, `load_toml`, `save_toml`, `ModelError` — no new error variant. Consistent.

**Scope:** one shippable slice — the pure contracts plus their single filesystem touchpoint. Bounded; produces a fully-tested foundation. Slices A (git adapter), B (forge/checkpoint), and C (spawn/renderers/CLI) build on these types without revising them.
```
